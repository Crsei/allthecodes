//! Non-cryptographic hash utilities.
//!
//! Provides fast hash functions for use in cache keys, change detection,
//! and other contexts where a fast hash is needed but cryptographic guarantees
//! are not required.
//!
//! Corresponds to Bun `src/utils/hash.ts`.

use std::hash::{Hash, Hasher};

/// Bun-compatible `djb2Hash` string hash.
///
/// Bun's helper returns a signed 32-bit integer and iterates JavaScript
/// `charCodeAt()` values. Rust mirrors that by hashing UTF-16 code units and
/// using `i32` wrapping arithmetic.
///
/// Bun equivalent: `djb2Hash()` in `src/utils/hash.ts`.
pub fn djb2_hash(input: &str) -> i32 {
    let mut hash: i32 = 0;
    for unit in input.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(unit as i32);
    }
    hash
}

/// Hash arbitrary bytes for Rust-local change detection using DefaultHasher.
///
/// This intentionally does not match Bun's `hashContent()` return type or
/// algorithm. Use only for in-process cache keys and ephemeral comparisons.
pub fn hash_content(content: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Hash two values for Rust-local cache keys without allocating an intermediate.
///
/// Feeds both values through the hasher sequentially. The order matters:
/// `hash_pair("a", "b")` differs from `hash_pair("b", "a")`.
///
/// This intentionally does not match Bun's `hashPair()` string output.
pub fn hash_pair<A: Hash, B: Hash>(a: &A, b: &B) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut hasher);
    b.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_djb2_hash_deterministic() {
        let h1 = djb2_hash("hello world");
        let h2 = djb2_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_djb2_hash_different_inputs() {
        let h1 = djb2_hash("abc");
        let h2 = djb2_hash("xyz");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_djb2_hash_empty() {
        assert_eq!(djb2_hash(""), 0);
    }

    #[test]
    fn test_hash_content_deterministic_same_run() {
        let h1 = hash_content(b"same content");
        let h2 = hash_content(b"same content");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_content_different() {
        let h1 = hash_content(b"content a");
        let h2 = hash_content(b"content b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_pair_order_matters() {
        let h1 = hash_pair(&"hello", &"world");
        let h2 = hash_pair(&"world", &"hello");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_djb2_vs_bun_reference() {
        assert_eq!(djb2_hash("abc"), 96_354);
        assert_eq!(djb2_hash("xyz"), 119_193);
        assert_eq!(djb2_hash("hello"), 99_162_322);
    }
}
