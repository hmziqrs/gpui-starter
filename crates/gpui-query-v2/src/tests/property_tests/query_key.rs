//! Property-based tests for QueryKey and QueryKeyFilter.
//!
//! Uses proptest to verify structural properties hold for all possible inputs,
//! including edge cases like unicode, zero-width characters, and long keys.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use proptest::prelude::*;

use crate::core::*;

// ── Strategies ───────────────────────────────────────────────────────────

/// Strategy that produces an arbitrary non-empty Vec<String>.
/// QueryKey requires at least one segment, so we guarantee non-emptiness.
fn arb_segments() -> impl Strategy<Value = Vec<String>> {
    prop_oneof![
        // Normal depth
        prop::collection::vec(any::<String>(), 1..10),
        // Deep nesting (up to 100 segments)
        prop::collection::vec(any::<String>(), 10..100),
    ]
}

/// Strategy covering special cases: single-segment, unicode, long, and
/// keys containing the separator string "::".
fn arb_key_special() -> impl Strategy<Value = Vec<String>> {
    prop_oneof![
        // Single segment with arbitrary content
        any::<String>().prop_map(|s| vec![s]),
        // Unicode-heavy segments (letters, numbers, punctuation, symbols)
        prop::collection::vec("[\\p{L}\\p{N}\\p{P}\\p{S}]{1,20}", 1..5),
        // Keys containing the "::" separator
        any::<String>().prop_map(|s| vec![format!("{}::{}", s, s)]),
        // Longer keys (up to 50 segments for deeper nesting)
        prop::collection::vec(any::<String>(), 5..50),
        // Unicode edge-case segments: zero-width joiners, combining marks,
        // RTL overrides, surrogates-replacement, and other tricky codepoints
        // that regex classes like \p{L} do not cover.
        prop::collection::vec(arb_unicode_edge_case_string(), 1..5),
        // Very long single segment (100-2000 chars) to stress allocation paths
        ".{100,2000}".prop_map(|s| vec![s]),
    ]
}

/// Generates strings containing unicode edge cases that regex classes miss:
/// zero-width characters, combining marks, RTL/LTR overrides, BOM,
/// replacement characters, and mixed-direction text.
fn arb_unicode_edge_case_string() -> impl Strategy<Value = String> {
    use std::sync::LazyLock;
    // Codepoints that stress string handling: zero-width, combining, bidi, BOM,
    // replacement char, soft hyphen, non-breaking space, and normal mixed text.
    static EDGE_CASES: LazyLock<Vec<String>> = LazyLock::new(|| {
        vec![
            // Zero-width characters
            "\u{200B}".to_string(), // ZWSP
            "\u{200C}".to_string(), // ZWNJ
            "\u{200D}".to_string(), // ZWJ
            "\u{FEFF}".to_string(), // BOM
            // Combining characters (base + combining marks)
            "e\u{0301}".to_string(), // e + combining acute accent → é (NFD)
            "a\u{0308}\u{0301}".to_string(), // a + combining diaeresis + acute
            "\u{0300}".to_string(), // standalone combining grave accent
            // BiDi overrides
            "\u{202A}".to_string(), // LRE
            "\u{202B}".to_string(), // RLE
            "\u{202C}".to_string(), // PDF
            "\u{202D}".to_string(), // LRO
            "\u{202E}".to_string(), // RLO
            "\u{2066}".to_string(), // LRI
            "\u{2067}".to_string(), // RLI
            "\u{2068}".to_string(), // FSI
            "\u{2069}".to_string(), // PDI
            // Tricky whitespace / control-like
            "\u{00A0}".to_string(), // non-breaking space
            "\u{FEFF}".to_string(), // BOM (zero-width no-break space)
            "\u{2000}".to_string(), // en quad
            "\u{3000}".to_string(), // ideographic space
            // Replacement and special
            "\u{FFFD}".to_string(), // replacement character
            "\u{00AD}".to_string(), // soft hyphen
            // Mixed-direction strings
            "hello\u{202E}world\u{202C}".to_string(),
            "\u{0627}\u{0628}\u{062A}".to_string(), // Arabic ا ب ت
            "\u{05D0}\u{05D1}\u{05D2}".to_string(), // Hebrew א ב ג
            // Strings with mixed normalization forms
            "\u{00E9}".to_string(),       // é (NFC, single codepoint)
            "e\u{0301}".to_string(),       // é (NFD, base + combining)
            // Very long combining chain
            format!("x{}", "\u{0301}".repeat(50)),
            // String of only zero-width characters
            "\u{200B}\u{200C}\u{200D}\u{FEFF}".to_string(),
        ]
    });

    let cases = EDGE_CASES.clone();
    // Pick a random edge-case string, possibly concatenated with arbitrary text
    (any::<bool>(), any::<String>()).prop_map(move |(prefix, extra)| {
        let idx = (extra.len()) % cases.len();
        let edge = cases[idx].clone();
        if prefix { format!("{}{}", edge, extra) } else { format!("{}{}", extra, edge) }
    })
}

/// Combined strategy that mixes normal and special cases.
fn arb_key() -> impl Strategy<Value = Vec<String>> {
    prop_oneof![arb_segments(), arb_key_special()]
}

fn make_key(segments: &[String]) -> QueryKey {
    QueryKey::new(segments.iter().map(|s| s.as_str()))
}

fn hash_of(key: &QueryKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

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

// Standalone deterministic edge-case tests (no proptest parameters needed).

#[test]
fn key_empty_string_segment_distinguishes_from_multi() {
    let single_empty = QueryKey::from([""]);
    let two_empty = QueryKey::from(["", ""]);
    assert_ne!(single_empty, two_empty);
    assert_eq!(single_empty.to_path(), "");
    assert_eq!(two_empty.to_path(), "::");
}

#[test]
fn key_single_empty_segment_properties() {
    let key = QueryKey::from([""]);
    assert_eq!(key.parts().len(), 1);
    assert_eq!(key.as_str(), "");
    assert_eq!(key.to_path(), "");
    assert_eq!(key.as_single(), Some(""));
}

// ── 9. Deterministic unicode edge-case tests ────────────────────────────
//
// These cover codepoints that proptest regex strategies (\p{L}, \p{N}, etc.)
// do not reliably generate: zero-width characters, combining marks, RTL
// overrides, BOM, replacement characters, and mixed normalization forms.

/// Helper: assert clone, hash, serde roundtrip, and to_path consistency.
fn assert_key_invariants(key: &QueryKey, expected_path: &str) {
    // Clone equality
    let cloned = key.clone();
    assert_eq!(key, &cloned, "clone should be equal");

    // Hash consistency
    assert_eq!(hash_of(key), hash_of(&cloned), "hash should match for equal keys");

    // Serde roundtrip
    let json = serde_json::to_string(key).unwrap();
    let back: QueryKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, &back, "serde roundtrip should produce equal key");

    // to_path format
    assert_eq!(key.to_path(), expected_path, "to_path should match expected");
}

#[test]
fn key_zero_width_space_segment() {
    // U+200B Zero Width Space
    let key = QueryKey::from(["\u{200B}"]);
    assert_key_invariants(&key, "\u{200B}");
}

#[test]
fn key_zero_width_joiner_segment() {
    // U+200D ZWJ (used in emoji sequences like family emoji)
    let key = QueryKey::from(["\u{200D}"]);
    assert_key_invariants(&key, "\u{200D}");
}

#[test]
fn key_zwnj_segment() {
    // U+200C Zero Width Non-Joiner
    let key = QueryKey::from(["\u{200C}"]);
    assert_key_invariants(&key, "\u{200C}");
}

#[test]
fn key_bom_segment() {
    // U+FEFF Byte Order Mark / Zero Width No-Break Space
    let key = QueryKey::from(["\u{FEFF}"]);
    assert_key_invariants(&key, "\u{FEFF}");
}

#[test]
fn key_combining_acute_accent_nfd() {
    // "e" + U+0301 combining acute accent = é in NFD form
    let nfd = "e\u{0301}";
    let key = QueryKey::from([nfd]);
    assert_key_invariants(&key, nfd);
}

#[test]
fn key_combining_precomposed_nfc() {
    // U+00E9 é (precomposed, NFC form) — must differ from NFD form
    let nfc = "\u{00E9}";
    let nfd = "e\u{0301}";
    let key_nfc = QueryKey::from([nfc]);
    let key_nfd = QueryKey::from([nfd]);
    // NFC and NFD are different byte sequences so keys must differ
    assert_ne!(key_nfc, key_nfd, "NFC and NFD keys should be distinct");
    assert_key_invariants(&key_nfc, nfc);
    assert_key_invariants(&key_nfd, nfd);
}

#[test]
fn key_standalone_combining_mark() {
    // Combining grave accent with no base character
    let key = QueryKey::from(["\u{0300}"]);
    assert_key_invariants(&key, "\u{0300}");
}

#[test]
fn key_long_combining_chain() {
    // Base char followed by many combining marks
    let segment = format!("a{}", "\u{0301}".repeat(50));
    let key = QueryKey::from([&*segment]);
    assert_key_invariants(&key, &segment);
}

#[test]
fn key_rtl_override() {
    // U+202E Right-to-Left Override
    let key = QueryKey::from(["\u{202E}hello\u{202C}"]);
    assert_key_invariants(&key, "\u{202E}hello\u{202C}");
}

#[test]
fn key_bidi_isolates() {
    // U+2066 LRI, U+2067 RLI, U+2068 FSI, U+2069 PDI
    let key = QueryKey::from(["\u{2066}\u{2067}\u{2068}\u{2069}"]);
    assert_key_invariants(&key, "\u{2066}\u{2067}\u{2068}\u{2069}");
}

#[test]
fn key_replacement_character() {
    // U+FFFD replacement character
    let key = QueryKey::from(["\u{FFFD}"]);
    assert_key_invariants(&key, "\u{FFFD}");
}

#[test]
fn key_soft_hyphen() {
    // U+00AD soft hyphen
    let key = QueryKey::from(["\u{00AD}"]);
    assert_key_invariants(&key, "\u{00AD}");
}

#[test]
fn key_non_breaking_space() {
    // U+00A0 non-breaking space
    let key = QueryKey::from(["\u{00A0}"]);
    assert_key_invariants(&key, "\u{00A0}");
}

#[test]
fn key_ideographic_space() {
    // U+3000 ideographic space
    let key = QueryKey::from(["\u{3000}"]);
    assert_key_invariants(&key, "\u{3000}");
}

#[test]
fn key_mixed_rtl_and_ltr() {
    // Mixed Arabic and Latin text
    let segment = "hello\u{0627}\u{0628}\u{062A}world";
    let key = QueryKey::from([segment]);
    assert_key_invariants(&key, segment);
}

#[test]
fn key_only_zero_width_chars_segment() {
    // String composed entirely of zero-width characters
    let segment = "\u{200B}\u{200C}\u{200D}\u{FEFF}";
    let key = QueryKey::from([segment]);
    assert_key_invariants(&key, segment);
}

#[test]
fn key_very_long_single_segment() {
    // 2000-character single segment to stress allocation and hashing
    let segment = "x".repeat(2000);
    let key = QueryKey::from([&*segment]);
    assert_key_invariants(&key, &segment);
    assert_eq!(key.parts()[0].len(), 2000);
}

#[test]
fn key_unicode_edge_case_in_multi_segment_key() {
    // Ensure edge-case segments interact correctly with the "::" separator
    let key = QueryKey::from(["\u{200B}", "\u{FEFF}", "\u{FFFD}"]);
    let expected_path = "\u{200B}::\u{FEFF}::\u{FFFD}";
    assert_key_invariants(&key, expected_path);
    // Verify prefix matching works with these segments
    let prefix = QueryKey::from(["\u{200B}"]);
    assert!(key.starts_with(&prefix));
    let prefix2 = QueryKey::from(["\u{200B}", "\u{FEFF}"]);
    assert!(key.starts_with(&prefix2));
    assert!(!prefix.starts_with(&key));
}

#[test]
fn key_deeply_nested_200_segments() {
    // Stress-test: 200-segment key should still satisfy all invariants
    let segments: Vec<String> = (0..200).map(|i| format!("seg{}", i)).collect();
    let key = make_key(&segments);
    assert_key_invariants(&key, &segments.join("::"));
    // Prefix matching at various depths
    for depth in [1, 50, 100, 199] {
        let prefix_segs: Vec<String> = segments[..depth].to_vec();
        let prefix = make_key(&prefix_segs);
        assert!(key.starts_with(&prefix), "should match prefix of depth {}", depth);
    }
}

#[test]
fn key_unicode_edge_cases_distinct_keys() {
    // Different zero-width characters must produce distinct keys
    let zwsp = QueryKey::from(["\u{200B}"]);
    let zwnj = QueryKey::from(["\u{200C}"]);
    let zwj = QueryKey::from(["\u{200D}"]);
    let bom = QueryKey::from(["\u{FEFF}"]);

    assert_ne!(zwsp, zwnj);
    assert_ne!(zwsp, zwj);
    assert_ne!(zwsp, bom);
    assert_ne!(zwnj, zwj);
    assert_ne!(zwnj, bom);
    assert_ne!(zwj, bom);

    // They should also have distinct hashes (not guaranteed but very likely)
    let hashes: Vec<u64> = [&zwsp, &zwnj, &zwj, &bom].iter().map(|k| hash_of(k)).collect();
    let unique_hashes: std::collections::HashSet<u64> = hashes.into_iter().collect();
    assert_eq!(unique_hashes.len(), 4, "four distinct zero-width chars should have distinct hashes");
}
