//! Per-image work on several threads, with the standard library only
//! (docs/spikes/008).
//!
//! The asset passes do the same independent thing to every image (read a
//! header, crop, convert an SVG): [`map`] runs one closure over a list on up
//! to [`threads`] scoped threads and returns the results **in the input's
//! order**, so whatever a caller builds from them is the same with one thread
//! or many -- the PDF comes out byte for byte identical.

use std::cell::Cell;
use std::sync::OnceLock;

/// The environment variable that caps the worker threads (a positive
/// integer). Unset or unreadable: the machine's available parallelism.
pub const THREADS_ENV: &str = "PDFCN_THREADS";

/// How many threads [`map`] may use, read once per process.
pub fn threads() -> usize {
    static THREADS: OnceLock<usize> = OnceLock::new();
    *THREADS.get_or_init(|| {
        parse_threads(std::env::var(THREADS_ENV).ok().as_deref()).unwrap_or_else(|| {
            std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
        })
    })
}

fn parse_threads(value: Option<&str>) -> Option<usize> {
    value?.trim().parse::<usize>().ok().filter(|n| *n > 0)
}

thread_local! {
    static OVERRIDE: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Runs `f` with the thread budget of every render it makes on this thread
/// set to `threads` (at least one) instead of [`threads`]. A caller that
/// already spreads documents over threads (a batch) passes 1, so each
/// document does not start threads of its own on top.
pub fn with_threads<R>(threads: usize, f: impl FnOnce() -> R) -> R {
    let previous = OVERRIDE.with(|o| o.replace(Some(threads.max(1))));
    // Restored even if `f` panics.
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|o| o.set(self.0));
        }
    }
    let _restore = Restore(previous);
    f()
}

/// `items.iter().map(f).collect()`, on up to `threads()` threads (or the
/// budget [`with_threads`] set on this thread).
pub(crate) fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let budget = OVERRIDE.with(Cell::get).unwrap_or_else(threads);
    map_with(budget, items, f)
}

/// [`map`] with an explicit thread budget. Splits `items` into contiguous
/// chunks, one per thread, and concatenates the chunks' results in order. A
/// single item, or a budget of one, runs on the calling thread. A closure that
/// panics propagates its panic, as a sequential map would.
pub(crate) fn map_with<T: Sync, R: Send>(
    budget: usize,
    items: &[T],
    f: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let workers = budget.min(items.len());
    if workers <= 1 {
        return items.iter().map(f).collect();
    }
    let chunk = items.len().div_ceil(workers);
    let f = &f;
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| scope.spawn(move || part.iter().map(f).collect::<Vec<R>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| match handle.join() {
                Ok(results) => results,
                Err(panic) => std::panic::resume_unwind(panic),
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_keep_the_input_order_on_any_number_of_threads() {
        let items: Vec<u32> = (0..103).collect();
        let expected: Vec<u32> = items.iter().map(|n| n * n).collect();
        for budget in [1, 2, 3, 4, 8, 200] {
            assert_eq!(map_with(budget, &items, |n| n * n), expected, "{budget}");
        }
        assert!(map_with(4, &[] as &[u32], |n| *n).is_empty());
    }

    #[test]
    fn work_really_spreads_over_threads() {
        let items: Vec<u32> = (0..8).collect();
        let ids = map_with(4, &items, |_| std::thread::current().id());
        let distinct: std::collections::BTreeSet<String> =
            ids.iter().map(|id| format!("{id:?}")).collect();
        assert_eq!(distinct.len(), 4);
    }

    #[test]
    fn with_threads_sets_the_budget_for_this_thread_only_and_restores_it() {
        let items: Vec<u32> = (0..8).collect();
        let distinct = |ids: Vec<std::thread::ThreadId>| {
            ids.iter()
                .map(|id| format!("{id:?}"))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        };
        let me = std::thread::current().id();
        let ids = with_threads(1, || map(&items, |_| std::thread::current().id()));
        assert_eq!(distinct(ids.clone()), 1);
        assert!(ids.iter().all(|id| *id == me));
        let nested = with_threads(2, || with_threads(1, || 7) + with_threads(0, || 1));
        assert_eq!(nested, 8);
        assert_eq!(OVERRIDE.with(Cell::get), None);
    }

    #[test]
    fn the_thread_budget_comes_from_the_environment_or_the_machine() {
        assert_eq!(parse_threads(Some("3")), Some(3));
        assert_eq!(parse_threads(Some(" 12 ")), Some(12));
        for junk in [None, Some(""), Some("0"), Some("-2"), Some("many")] {
            assert_eq!(parse_threads(junk), None, "{junk:?}");
        }
        assert!(threads() >= 1);
    }
}
