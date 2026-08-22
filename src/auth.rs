//! The optional `x-api-key` shared-secret gate common to both handlers.

use std::sync::OnceLock;

/// The optional shared-secret gate, read exactly once per isolate. Reading
/// it inside the handler would hide the dependency from tests and re-parse
/// the environment on every request; a serverless isolate is long-lived
/// enough that startup-time capture is the composition root.
///
/// Each binary linking this lib gets its own copy of this static -- the
/// same as when this code was duplicated by hand, just without the
/// duplication.
static API_KEY: OnceLock<Option<String>> = OnceLock::new();

pub fn api_key() -> Option<&'static str> {
    API_KEY
        .get_or_init(|| {
            std::env::var("PDFCN_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
        })
        .as_deref()
}

/// True iff `a` and `b` are byte-identical, in time depending only on
/// `a.len()` -- never on where the first mismatching byte falls. Comparing
/// a header value against a secret with `==` short-circuits at the first
/// differing byte, which turns "is this the right key" into a timing
/// oracle an attacker can use to recover the key one byte at a time. A
/// length mismatch is itself observable regardless (nothing here hides
/// `expected.len()`), so this only hardens the byte-content comparison
/// once the lengths are already known to match.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// The auth decision, isolated from HTTP plumbing so it's testable without
/// a live request: a request is authorized iff its x-api-key header equals
/// the configured key.
pub fn authorized(headers: &http::HeaderMap, expected_key: &str) -> bool {
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|got| constant_time_eq(got.as_bytes(), expected_key.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_regular_equality_semantics() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secre1"));
        assert!(!constant_time_eq(b"secret", b"shorter"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn authorized_requires_a_matching_header() {
        let mut headers = http::HeaderMap::new();
        headers.insert("x-api-key", "correct-key".parse().unwrap());
        assert!(authorized(&headers, "correct-key"));
        assert!(!authorized(&headers, "wrong-key"));
        assert!(!authorized(&http::HeaderMap::new(), "correct-key"));
    }
}
