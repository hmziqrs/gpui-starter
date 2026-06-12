//! Proptest strategies and shared helpers for QueryKey property tests.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use proptest::prelude::*;

use crate::core::*;

// ── Strategies ───────────────────────────────────────────────────────────

/// Strategy that produces an arbitrary non-empty Vec<String>.
/// QueryKey requires at least one segment, so we guarantee non-emptiness.
pub fn arb_segments() -> impl Strategy<Value = Vec<String>> {
    prop_oneof![
        // Normal depth
        prop::collection::vec(any::<String>(), 1..10),
        // Deep nesting (up to 100 segments)
        prop::collection::vec(any::<String>(), 10..100),
    ]
}

/// Strategy covering special cases: single-segment, unicode, long, and
/// keys containing the separator string "::".
pub fn arb_key_special() -> impl Strategy<Value = Vec<String>> {
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
pub fn arb_unicode_edge_case_string() -> impl Strategy<Value = String> {
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
            "\u{0300}".to_string(),  // standalone combining grave accent
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
            "\u{00E9}".to_string(),  // é (NFC, single codepoint)
            "e\u{0301}".to_string(), // é (NFD, base + combining)
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
        if prefix {
            format!("{}{}", edge, extra)
        } else {
            format!("{}{}", extra, edge)
        }
    })
}

/// Combined strategy that mixes normal and special cases.
pub fn arb_key() -> impl Strategy<Value = Vec<String>> {
    prop_oneof![arb_segments(), arb_key_special()]
}

// ── Helpers ──────────────────────────────────────────────────────────────

pub fn make_key(segments: &[String]) -> QueryKey {
    QueryKey::new(segments.iter().map(|s| s.as_str()))
}

pub fn hash_of(key: &QueryKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}
