//! Deterministic edge-case tests for QueryKey.
//!
//! Covers codepoints that proptest regex strategies (\p{L}, \p{N}, etc.)
//! do not reliably generate: zero-width characters, combining marks, RTL
//! overrides, BOM, replacement characters, and mixed normalization forms.

use crate::core::*;

use super::strategies::*;

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

/// Helper: assert clone, hash, serde roundtrip, and to_path consistency.
fn assert_key_invariants(key: &QueryKey, expected_path: &str) {
    // Clone equality
    let cloned = key.clone();
    assert_eq!(key, &cloned, "clone should be equal");

    // Hash consistency
    assert_eq!(
        hash_of(key),
        hash_of(&cloned),
        "hash should match for equal keys"
    );

    // Serde roundtrip
    let json = serde_json::to_string(key).unwrap();
    let back: QueryKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, &back, "serde roundtrip should produce equal key");

    // to_path format
    assert_eq!(
        key.to_path(),
        expected_path,
        "to_path should match expected"
    );
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
        assert!(
            key.starts_with(&prefix),
            "should match prefix of depth {}",
            depth
        );
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
    let hashes: Vec<u64> = [&zwsp, &zwnj, &zwj, &bom]
        .iter()
        .map(|k| hash_of(k))
        .collect();
    let unique_hashes: std::collections::HashSet<u64> = hashes.into_iter().collect();
    assert_eq!(
        unique_hashes.len(),
        4,
        "four distinct zero-width chars should have distinct hashes"
    );
}
