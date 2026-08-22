//! Property-based tests for QueryKey and QueryKeyFilter.
//!
//! Uses proptest to verify structural properties hold for all possible inputs,
//! including edge cases like unicode, zero-width characters, and long keys.

use proptest::prelude::*;

use crate::core::*;

use super::strategies::*;

// ── 1. Equality ─────────────────────────────────────────────────────────

proptest! {
    /// key1 == key2 iff all segments match.
    #[test]
    fn key_equality_same_segments(segments in arb_key()) {
        let k1 = make_key(&segments);
        let k2 = make_key(&segments);
        prop_assert!(&k1 == &k2);
    }

    /// Different segment lists produce unequal keys.
    #[test]
    fn key_equality_different_segments(a in arb_segments(), b in arb_segments()) {
        prop_assume!(a != b);
        let k1 = make_key(&a);
        let k2 = make_key(&b);
        prop_assert!(&k1 != &k2);
    }
}

// ── 2. Hash consistency ─────────────────────────────────────────────────

proptest! {
    /// Equal keys must produce equal hashes.
    #[test]
    fn key_hash_consistency(segments in arb_key()) {
        let k1 = make_key(&segments);
        let k2 = make_key(&segments);
        prop_assert_eq!(hash_of(&k1), hash_of(&k2));
    }
}

// ── 3. Clone ────────────────────────────────────────────────────────────

proptest! {
    /// Cloning produces an equal key backed by the same Arc allocation.
    #[test]
    fn key_clone_equality(segments in arb_key()) {
        let key = make_key(&segments);
        let cloned = key.clone();
        prop_assert!(&key == &cloned);
        // Verify cheap cloning: both keys deref to the same slice pointer
        let key_ptr: *const [std::sync::Arc<str>] = &*key;
        let cloned_ptr: *const [std::sync::Arc<str>] = &*cloned;
        prop_assert_eq!(key_ptr, cloned_ptr);
    }
}

// ── 4. Serde roundtrip ──────────────────────────────────────────────────

proptest! {
    /// deserialize(serialize(key)) == key for multi-segment keys.
    #[test]
    fn key_serde_roundtrip(segments in arb_key()) {
        let key = make_key(&segments);
        let json = serde_json::to_string(&key).unwrap();
        let back: QueryKey = serde_json::from_str(&json).unwrap();
        prop_assert!(&key == &back);
    }

    /// deserialize(serialize(key)) == key for single-string keys.
    #[test]
    fn key_serde_single_string_roundtrip(s in any::<String>()) {
        let key = QueryKey::from_single(&s);
        let json = serde_json::to_string(&key).unwrap();
        let back: QueryKey = serde_json::from_str(&json).unwrap();
        prop_assert!(&key == &back);
    }
}

// ── 5. Prefix matching (starts_with) ────────────────────────────────────

proptest! {
    /// Every key is a prefix of itself.
    #[test]
    fn key_prefix_self_match(segments in arb_key()) {
        let key = make_key(&segments);
        prop_assert!(key.starts_with(&key));
    }

    /// A proper prefix always matches.
    #[test]
    fn key_proper_prefix_always_matches(
        prefix in arb_segments(),
        suffix in arb_segments(),
    ) {
        let mut full_segs = prefix.clone();
        full_segs.extend(suffix);
        let full = make_key(&full_segs);
        let prefix_key = make_key(&prefix);
        prop_assert!(full.starts_with(&prefix_key));
    }

    /// Keys that diverge at the tail are not prefixes of each other.
    #[test]
    fn key_different_tail_does_not_match(
        common in arb_segments(),
        diff_a in any::<String>(),
        diff_b in any::<String>(),
    ) {
        prop_assume!(diff_a != diff_b);
        let mut segs_a = common.clone();
        segs_a.push(diff_a.clone());
        let mut segs_b = common.clone();
        segs_b.push(diff_b);
        let key_a = make_key(&segs_a);
        let key_b = make_key(&segs_b);
        prop_assert!(!key_a.starts_with(&key_b));
        prop_assert!(!key_b.starts_with(&key_a));
        let common_key = make_key(&common);
        prop_assert!(key_a.starts_with(&common_key));
        prop_assert!(key_b.starts_with(&common_key));
    }

    /// A longer key is never a prefix of a shorter key.
    #[test]
    fn key_longer_never_prefix_of_shorter(
        short in arb_segments(),
        extra in arb_segments(),
    ) {
        let mut long_segs = short.clone();
        long_segs.extend(extra);
        let long_key = make_key(&long_segs);
        let short_key = make_key(&short);
        prop_assert!(long_key.starts_with(&short_key));
        prop_assert!(!short_key.starts_with(&long_key));
    }
}

// ── 6. to_path format ───────────────────────────────────────────────────

proptest! {
    /// to_path joins segments with "::" separator.
    #[test]
    fn key_to_path_format(segments in arb_key()) {
        let key = make_key(&segments);
        let path = key.to_path();
        let expected = segments.join("::");
        prop_assert_eq!(path, expected);
    }
}

// ── 7. QueryKeyFilter semantics ─────────────────────────────────────────

proptest! {
    /// Exact filter matches only the identical key.
    #[test]
    fn filter_exact_matches_only_identical(
        target in arb_key(),
        other in arb_key(),
    ) {
        let target_key = make_key(&target);
        let other_key = make_key(&other);
        let filter = QueryKeyFilter::Exact(&target_key);
        if target == other {
            prop_assert!(filter.matches(&other_key));
        } else {
            prop_assert!(!filter.matches(&other_key));
        }
    }

    /// Prefix filter matches child keys and the prefix itself.
    #[test]
    fn filter_prefix_matches_children(
        prefix in arb_segments(),
        suffix in arb_segments(),
    ) {
        let mut child_segs = prefix.clone();
        child_segs.extend(suffix);
        let child = make_key(&child_segs);
        let prefix_key = make_key(&prefix);
        let filter = QueryKeyFilter::Prefix(&prefix_key);
        prop_assert!(filter.matches(&child));
        prop_assert!(filter.matches(&prefix_key));
    }

    /// Prefix filter rejects keys that differ from the prefix.
    #[test]
    fn filter_prefix_rejects_non_prefix(
        prefix in arb_segments(),
        diverge in any::<String>(),
    ) {
        prop_assume!(!prefix.is_empty());
        let prefix_key = make_key(&prefix);
        let filter = QueryKeyFilter::Prefix(&prefix_key);
        let mut other_segs = prefix.clone();
        other_segs[0] = format!("{}{}", diverge, other_segs[0]);
        let other_key = make_key(&other_segs);
        if other_segs != prefix {
            prop_assert!(!filter.matches(&other_key));
        }
    }

    /// All filter matches every possible key.
    #[test]
    fn filter_all_matches_everything(segments in arb_key()) {
        let key = make_key(&segments);
        prop_assert!(QueryKeyFilter::All.matches(&key));
    }
}

// ── 8. Edge cases: unicode and long keys ────────────────────────────────

proptest! {
    /// Unicode segments survive clone, hash, serde, and to_path.
    #[test]
    fn key_unicode_roundtrip(
        segments in prop::collection::vec("[\\p{L}\\p{N}]{1,10}", 1..5),
    ) {
        let key = make_key(&segments);
        let cloned = key.clone();
        prop_assert!(&key == &cloned);
        prop_assert_eq!(hash_of(&key), hash_of(&cloned));
        let json = serde_json::to_string(&key).unwrap();
        let back: QueryKey = serde_json::from_str(&json).unwrap();
        prop_assert!(&key == &back);
        prop_assert_eq!(key.to_path(), segments.join("::"));
    }

    /// Very long keys (50-100 segments) still satisfy all invariants.
    #[test]
    fn key_long_key_correctness(
        segments in prop::collection::vec(any::<String>(), 50..100),
    ) {
        let key = make_key(&segments);
        let cloned = key.clone();
        prop_assert!(&key == &cloned);
        prop_assert!(key.starts_with(&key));
        let json = serde_json::to_string(&key).unwrap();
        let back: QueryKey = serde_json::from_str(&json).unwrap();
        prop_assert!(&key == &back);
    }
}
