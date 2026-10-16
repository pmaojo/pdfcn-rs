//! Process-wide caches keyed by a hash of their input (docs/spikes/008),
//! behind the `cache` cargo feature.
//!
//! A long-lived process (`pdfcn dev`, a server, a batch) renders the same
//! template, the same logos and the same photos over and over. Each cache here
//! maps *what went in* -- the bytes of a template, an SVG, a photo plus what
//! was asked of it -- to what came out. There is no invalidation by time or by
//! file modification: a changed input is a different key, and an unchanged
//! one can only ever produce the same result.
//!
//! Each cache has a budget in bytes and evicts the least recently used entry
//! when a new one would not fit (an LRU written out below: a map plus a use
//! counter; eviction scans for the oldest entry, which is cheap at the few
//! hundred entries a budget allows). An entry larger than the whole budget is
//! simply not kept. The budget is the `PDFCN_CACHE_MB` environment variable
//! (megabytes, per cache), 64 by default; `0` turns caching off.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

/// The environment variable holding each cache's budget, in megabytes.
pub const CACHE_MB_ENV: &str = "PDFCN_CACHE_MB";

const DEFAULT_BUDGET_MB: usize = 64;

/// What a cache is keyed by: a 64-bit hash of the input and its length, so a
/// hash collision would also have to match the length to be mistaken.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Key(u64, usize);

/// The key for `parts`, hashed in order. `len` is the length of the bulk of
/// the input (a template's text, an image's bytes).
pub(crate) fn key(parts: impl Hash, len: usize) -> Key {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut hasher);
    Key(hasher.finish(), len)
}

struct Entry<V> {
    value: Arc<V>,
    cost: usize,
    last_use: u64,
}

/// A least-recently-used map with a budget in bytes.
pub(crate) struct Lru<V> {
    entries: HashMap<Key, Entry<V>>,
    budget: usize,
    used: usize,
    clock: u64,
}

impl<V> Lru<V> {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            budget,
            used: 0,
            clock: 0,
        }
    }

    pub(crate) fn get(&mut self, key: Key) -> Option<Arc<V>> {
        self.clock += 1;
        let entry = self.entries.get_mut(&key)?;
        entry.last_use = self.clock;
        Some(Arc::clone(&entry.value))
    }

    /// Keeps `value`, which costs `cost` bytes, evicting the least recently
    /// used entries until it fits. Returns the shared handle either way.
    pub(crate) fn insert(&mut self, key: Key, value: V, cost: usize) -> Arc<V> {
        let value = Arc::new(value);
        if cost > self.budget {
            return value;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.used -= old.cost;
        }
        while self.used + cost > self.budget {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_use)
                .map(|(k, _)| *k)
            else {
                break;
            };
            if let Some(evicted) = self.entries.remove(&oldest) {
                self.used -= evicted.cost;
            }
        }
        self.clock += 1;
        self.used += cost;
        self.entries.insert(
            key,
            Entry {
                value: Arc::clone(&value),
                cost,
                last_use: self.clock,
            },
        );
        value
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Each cache's budget in bytes, read once per process.
fn budget() -> usize {
    static BUDGET: OnceLock<usize> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        std::env::var(CACHE_MB_ENV)
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_BUDGET_MB)
            .saturating_mul(1024 * 1024)
    })
}

/// A process-wide cache: `compute` runs on a miss (outside the lock, so
/// threads computing different keys never wait on each other) and its result
/// is kept at `cost(&result)` bytes.
pub(crate) struct Shared<V> {
    lru: OnceLock<Mutex<Lru<V>>>,
}

impl<V> Shared<V> {
    pub(crate) const fn new() -> Self {
        Self {
            lru: OnceLock::new(),
        }
    }

    fn lru(&self) -> &Mutex<Lru<V>> {
        self.lru.get_or_init(|| Mutex::new(Lru::new(budget())))
    }

    pub(crate) fn get(&self, key: Key) -> Option<Arc<V>> {
        self.lru().lock().ok().and_then(|mut l| l.get(key))
    }

    /// Keeps `value` at `cost` bytes; returns the shared handle either way.
    pub(crate) fn insert(&self, key: Key, value: V, cost: usize) -> Arc<V> {
        match self.lru().lock() {
            Ok(mut l) => l.insert(key, value, cost),
            // A thread panicked while holding the lock: don't cache.
            Err(_) => Arc::new(value),
        }
    }

    pub(crate) fn get_or_compute(
        &self,
        key: Key,
        compute: impl FnOnce() -> V,
        cost: impl FnOnce(&V) -> usize,
    ) -> Arc<V> {
        if let Some(hit) = self.get(key) {
            return hit;
        }
        let value = compute();
        let cost = cost(&value);
        self.insert(key, value, cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_least_recently_used_entry_goes_first() {
        let mut lru = Lru::new(30);
        let (a, b, c) = (key("a", 1), key("b", 1), key("c", 1));
        lru.insert(a, "a", 10);
        lru.insert(b, "b", 10);
        lru.insert(c, "c", 10);
        // Touch `a`: `b` is now the oldest.
        assert!(lru.get(a).is_some());
        lru.insert(key("d", 1), "d", 10);
        assert!(lru.get(b).is_none());
        assert!(lru.get(a).is_some() && lru.get(c).is_some());
        assert_eq!(lru.len(), 3);
    }

    #[test]
    fn the_budget_holds_and_an_oversized_entry_is_not_kept() {
        let mut lru = Lru::new(100);
        for i in 0..50 {
            lru.insert(key(i, 8), i, 30);
            assert!(lru.used <= 100);
        }
        assert_eq!(lru.len(), 3);
        let big = lru.insert(key("big", 8), 99, 101);
        assert_eq!(*big, 99);
        assert!(lru.get(key("big", 8)).is_none());
        // Replacing a key frees what it used.
        lru.insert(key(49, 8), 7, 10);
        assert!(lru.used <= 100);
        assert_eq!(lru.get(key(49, 8)).map(|v| *v), Some(7));
    }

    #[test]
    fn a_shared_cache_computes_once_per_key() {
        static CACHE: Shared<String> = Shared::new();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let get = |text: &str| {
            CACHE.get_or_compute(
                key(text, text.len()),
                || {
                    calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    text.to_uppercase()
                },
                String::len,
            )
        };
        assert_eq!(*get("abc"), "ABC");
        assert_eq!(*get("abc"), "ABC");
        assert_eq!(*get("xyz"), "XYZ");
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 2);
    }

    #[test]
    fn keys_differ_by_content_and_by_length() {
        assert_ne!(key("a", 1), key("b", 1));
        assert_ne!(key("a", 1), key("a", 2));
        assert_eq!(key(("svg", 3u8), 9), key(("svg", 3u8), 9));
    }
}
