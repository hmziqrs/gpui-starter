use gpui_query::QueryKey;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

// ── Edge Case 1: Empty key ──

#[test]
fn empty_key_compiles_and_has_zero_parts() {
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    assert_eq!(empty.parts().len(), 0);
}

#[test]
fn empty_key_as_str_returns_empty_string() {
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    assert_eq!(empty.as_str(), "");
}

#[test]
fn empty_key_matches_everything_in_prefix() {
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    let key = QueryKey::from(["users", "42"]);
    assert!(key.starts_with(&empty));
}

#[test]
fn empty_key_starts_with_itself() {
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    assert!(empty.starts_with(&empty));
}

#[test]
fn empty_key_does_not_start_with_nonempty() {
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    let nonempty = QueryKey::from(["users"]);
    assert!(!empty.starts_with(&nonempty));
}

// ── Edge Case 2: Empty string segment ──

#[test]
fn key_with_empty_string_segment() {
    let key = QueryKey::from(["users", "", "42"]);
    assert_eq!(key.parts().len(), 3);
    assert_eq!(key.parts()[0].as_ref(), "users");
    assert_eq!(key.parts()[1].as_ref(), "");
    assert_eq!(key.parts()[2].as_ref(), "42");
}

#[test]
fn prefix_matching_with_empty_segment() {
    let key = QueryKey::from(["users", "", "42"]);
    assert!(key.starts_with(&QueryKey::from(["users"])));
    assert!(key.starts_with(&QueryKey::from(["users", ""])));
    assert!(key.starts_with(&QueryKey::from(["users", "", "42"])));
    // Segment 1 is "" not "42", so this should NOT match
    assert!(!key.starts_with(&QueryKey::from(["users", "42"])));
}

#[test]
fn single_empty_string_key_differs_from_zero_segment_key() {
    let single_empty = QueryKey::from("");
    let zero_segments: QueryKey = QueryKey::from([] as [&str; 0]);
    assert_ne!(single_empty, zero_segments);
    assert_eq!(single_empty.parts().len(), 1);
    assert_eq!(zero_segments.parts().len(), 0);
}

// ── Edge Case 3: Special characters ──

#[test]
fn unicode_segments_hash_and_eq() {
    let k1 = QueryKey::from(["users", "日本語", "🦀"]);
    let k2 = QueryKey::from(["users", "日本語", "🦀"]);
    assert_eq!(k1, k2);
    assert_eq!(hash_of(&k1), hash_of(&k2));
}

#[test]
fn special_chars_in_segments() {
    let slash = QueryKey::from(["users/42", "posts/1"]);
    let dot = QueryKey::from(["a.b.c", "d.e.f"]);
    let colon = QueryKey::from(["http://example.com", ":path"]);
    let tab = QueryKey::from(["col\tumn", "row\n"]);

    assert!(slash.starts_with(&QueryKey::from(["users/42"])));
    assert!(dot.starts_with(&QueryKey::from(["a.b.c"])));
    assert!(colon.starts_with(&QueryKey::from(["http://example.com"])));

    assert_eq!(slash.clone(), slash);
    assert_eq!(dot.clone(), dot);
    assert_eq!(colon.clone(), colon);
    assert_eq!(tab.clone(), tab);
}

#[test]
fn keys_with_different_unicode_are_not_equal() {
    let k1 = QueryKey::from(["users", "日本語"]);
    let k2 = QueryKey::from(["users", "日本冶"]);
    assert_ne!(k1, k2);
}

// ── Edge Case 4: as_str() collision ──

#[test]
fn as_str_only_returns_first_segment() {
    let multi = QueryKey::from(["users", "42"]);
    let single = QueryKey::from("users");
    assert_eq!(multi.as_str(), "users");
    assert_eq!(single.as_str(), "users");
    assert_ne!(multi, single);
}

#[test]
fn as_str_on_empty_key_returns_empty() {
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    assert_eq!(empty.as_str(), "");
}

#[test]
fn as_str_on_key_starting_with_empty_segment() {
    let key = QueryKey::from(["", "42"]);
    assert_eq!(key.as_str(), "");
}

// ── Edge Case 5: from_single("a.b") vs from(["a.b"]) ──

#[test]
fn from_single_vs_from_array_single_element() {
    let single = QueryKey::from_single("a.b");
    let arr = QueryKey::from(["a.b"]);
    assert_eq!(single, arr);
    assert_eq!(single.parts().len(), 1);
    assert_eq!(arr.parts().len(), 1);
    assert_eq!(single.parts()[0].as_ref(), "a.b");
}

#[test]
fn from_single_does_not_split_on_dots() {
    let key = QueryKey::from_single("a.b.c");
    assert_eq!(key.parts().len(), 1);
    assert_eq!(key.parts()[0].as_ref(), "a.b.c");
}

// ── Edge Case 6: Prefix longer than key ──

#[test]
fn prefix_longer_than_key_returns_false() {
    let short = QueryKey::from("a");
    let long = QueryKey::from(["a", "b", "c"]);
    assert!(!short.starts_with(&long));
}

#[test]
fn empty_key_vs_long_prefix() {
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    let long = QueryKey::from(["a", "b", "c"]);
    assert!(!empty.starts_with(&long));
}

// ── Edge Case 7: Empty prefix matches everything ──

#[test]
fn empty_prefix_matches_any_key() {
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    assert!(QueryKey::from("x").starts_with(&empty));
    assert!(QueryKey::from(["x", "y", "z"]).starts_with(&empty));
    assert!(empty.starts_with(&empty));
}

// ── Edge Case 8: Clone sharing ──
//
// NOTE: The inner Arc<[Arc<str>]> field is private, so we cannot test
// Arc::ptr_eq from integration tests. Instead we verify that clone produces
// an equal key and that starts_with works bidirectionally. The internal
// unit test `clone_is_cheap` in key.rs verifies Arc pointer sharing.

#[test]
fn clone_is_equal_and_starts_with_self() {
    let original = QueryKey::from(["a", "b", "c"]);
    let cloned = original.clone();
    assert_eq!(original, cloned);
    assert!(cloned.starts_with(&original));
    assert!(original.starts_with(&cloned));
}

// ── Edge Case 9: Serde roundtrip ──
//
// NOTE: Arc sharing is NOT preserved across serde roundtrip because
// Deserialize builds fresh Arc allocations. The internal unit test
// `clone_is_cheap` verifies Arc sharing for clone. Here we test that
// value equality is preserved, which is what matters for correctness.

#[test]
fn serde_roundtrip_preserves_value() {
    let key = QueryKey::from(["users", "42"]);
    let json = serde_json::to_string(&key).unwrap();
    let back: QueryKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, back);
}

#[test]
fn serde_roundtrip_empty_key() {
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    let json = serde_json::to_string(&empty).unwrap();
    assert_eq!(json, "[]");
    let back: QueryKey = serde_json::from_str(&json).unwrap();
    assert_eq!(empty, back);
    assert_eq!(back.parts().len(), 0);
}

#[test]
fn serde_roundtrip_unicode() {
    let key = QueryKey::from(["users", "日本語", "🦀"]);
    let json = serde_json::to_string(&key).unwrap();
    let back: QueryKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, back);
}

#[test]
fn serde_roundtrip_empty_segment() {
    let key = QueryKey::from(["users", "", "42"]);
    let json = serde_json::to_string(&key).unwrap();
    assert_eq!(json, r#"["users","","42"]"#);
    let back: QueryKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, back);
}

#[test]
fn serde_roundtrip_special_chars() {
    let key = QueryKey::from(["users/42", "a.b.c", "http://x:y"]);
    let json = serde_json::to_string(&key).unwrap();
    let back: QueryKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, back);
}

// ── key_filter with empty key ──

#[test]
fn filter_all_matches_empty_key() {
    use gpui_query::QueryKeyFilter;
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    assert!(QueryKeyFilter::All.matches(&empty));
}

#[test]
fn filter_exact_empty_key() {
    use gpui_query::QueryKeyFilter;
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    assert!(QueryKeyFilter::Exact(&empty).matches(&empty));
    let nonempty = QueryKey::from(["x"]);
    assert!(!QueryKeyFilter::Exact(&empty).matches(&nonempty));
}

#[test]
fn filter_prefix_empty_matches_everything() {
    use gpui_query::QueryKeyFilter;
    let empty: QueryKey = QueryKey::from([] as [&str; 0]);
    let key = QueryKey::from(["users", "42"]);
    assert!(QueryKeyFilter::Prefix(&empty).matches(&key));
    assert!(QueryKeyFilter::Prefix(&empty).matches(&empty));
}
