use std::ops::Deref;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeSeq};

/// A structured, hierarchical cache key for query resources.
///
/// Inspired by TanStack Query's array keys (`["todos", id]`), a `QueryKey`
/// is an ordered sequence of string segments.
///
/// # Cheap cloning
///
/// Internally uses `Arc<[Arc<str>]>`, so cloning is a single atomic ref-count
/// increment regardless of key length.
///
/// # Invariants
///
/// A `QueryKey` must contain at least one segment. Zero-length keys are
/// unsupported: constructing one via [`QueryKey::new`] with an empty iterator
/// will panic unconditionally in all build modes.
///
/// # Examples
///
/// ```
/// use gpui_query_v2::QueryKey;
///
/// let key = QueryKey::from(["users", "42", "posts"]);
/// assert!(key.starts_with(&QueryKey::from(["users"])));
/// assert!(!key.starts_with(&QueryKey::from(["posts"])));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryKey(Arc<[Arc<str>]>);

impl QueryKey {
    /// Create a key from an iterator of string-like parts.
    ///
    /// # Panics
    ///
    /// Panics if the iterator yields zero segments.
    pub fn new(parts: impl IntoIterator<Item: AsRef<str>>) -> Self {
        let segments: Vec<Arc<str>> = parts.into_iter().map(|s| Arc::from(s.as_ref())).collect();
        assert!(
            !segments.is_empty(),
            "QueryKey must contain at least one segment"
        );
        Self(segments.into())
    }

    /// Create a single-segment key from a string.
    pub fn from_single(value: impl AsRef<str>) -> Self {
        Self(Arc::from([Arc::from(value.as_ref())]))
    }

    /// The key segments.
    pub fn parts(&self) -> &[Arc<str>] {
        &self.0
    }

    /// Returns the single segment if this key has exactly one part, else `None`.
    pub fn as_single(&self) -> Option<&str> {
        if self.0.len() == 1 {
            Some(&self.0[0])
        } else {
            None
        }
    }

    /// Returns the first segment as a string slice.
    pub fn as_str(&self) -> &str {
        match self.0.first() {
            Some(s) => s.as_ref(),
            None => "",
        }
    }

    /// Returns the full key as a double-colon-separated path string.
    ///
    /// Useful for diagnostics and DevTools display. Uses `"::"` as the
    /// separator to avoid ambiguity when segments contain forward slashes.
    pub fn to_path(&self) -> String {
        let mut iter = self.0.iter().map(|s| s.as_ref());
        match iter.next() {
            Some(first) => iter.fold(first.to_owned(), |acc, s| acc + "::" + s),
            None => String::new(),
        }
    }

    /// Returns `true` if this key starts with the given `prefix`.
    ///
    /// If `prefix` is empty (zero segments), returns `true` — an empty
    /// prefix matches every valid key. If `self` is also empty, returns
    /// `false`, since zero-length keys are unsupported.
    pub fn starts_with(&self, prefix: &QueryKey) -> bool {
        if prefix.0.is_empty() {
            // An empty prefix matches every valid (non-empty) key.
            return !self.0.is_empty();
        }
        if prefix.0.len() > self.0.len() {
            return false;
        }
        self.0[..prefix.0.len()]
            .iter()
            .zip(prefix.0.iter())
            .all(|(a, b)| a == b)
    }

    /// Create a new key by appending an extra segment.
    ///
    /// This is **O(n)** in key length because it copies all existing segments
    /// into a new `Arc<[Arc<str>]>`. For hot paths that build keys incrementally,
    /// prefer constructing the full key in one [`QueryKey::new`] call rather than
    /// chaining `.join()` calls (e.g., prefer `QueryKey::new(["a", "b", "c"])`
    /// over `QueryKey::from("a").join("b").join("c")`).
    pub fn join(&self, extra: &str) -> QueryKey {
        let mut parts: Vec<Arc<str>> = self.0.to_vec();
        parts.push(Arc::from(extra));
        Self(parts.into())
    }
}

impl Deref for QueryKey {
    type Target = [Arc<str>];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ── From impls ──────────────────────────────────────────────────────────

impl From<&str> for QueryKey {
    fn from(value: &str) -> Self {
        Self::from_single(value)
    }
}

impl From<String> for QueryKey {
    fn from(value: String) -> Self {
        Self::from_single(value)
    }
}

impl<const N: usize> From<[&str; N]> for QueryKey {
    fn from(parts: [&str; N]) -> Self {
        Self::new(parts)
    }
}

impl From<Vec<&str>> for QueryKey {
    fn from(parts: Vec<&str>) -> Self {
        Self::new(parts)
    }
}

impl From<Vec<String>> for QueryKey {
    fn from(parts: Vec<String>) -> Self {
        Self::new(parts)
    }
}

// ── Serde ───────────────────────────────────────────────────────────────

impl Serialize for QueryKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for s in self.0.iter() {
            seq.serialize_element(s.as_ref())?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for QueryKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum KeyRepr {
            Array(Vec<String>),
            String(String),
        }

        match KeyRepr::deserialize(deserializer)? {
            KeyRepr::Array(parts) => Ok(Self::new(parts)),
            KeyRepr::String(s) => Ok(Self::from_single(s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_part_key_from_str() {
        let key = QueryKey::from("users");
        assert_eq!(key.parts().len(), 1);
        assert_eq!(key.as_str(), "users");
        assert_eq!(key.as_single(), Some("users"));
    }

    #[test]
    fn multi_part_key_from_array() {
        let key = QueryKey::from(["users", "42", "posts"]);
        assert_eq!(key.parts().len(), 3);
        assert_eq!(key.as_single(), None);
    }

    #[test]
    fn starts_with_prefix() {
        let key = QueryKey::from(["users", "42", "posts"]);
        assert!(key.starts_with(&QueryKey::from(["users"])));
        assert!(key.starts_with(&QueryKey::from(["users", "42"])));
        assert!(!key.starts_with(&QueryKey::from(["posts"])));
    }

    #[test]
    fn to_path_joins_segments() {
        let key = QueryKey::from(["users", "42", "posts"]);
        assert_eq!(key.to_path(), "users::42::posts");
    }

    #[test]
    fn clone_is_cheap() {
        let key = QueryKey::from(["a", "b", "c"]);
        let cloned = key.clone();
        assert_eq!(key, cloned);
        assert!(Arc::ptr_eq(&key.0, &cloned.0));
    }

    #[test]
    fn serde_roundtrip() {
        let key = QueryKey::from(["users", "42"]);
        let json = serde_json::to_string(&key).unwrap();
        let back: QueryKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    #[should_panic(expected = "QueryKey must contain at least one segment")]
    fn new_rejects_empty_parts() {
        let _ = QueryKey::new(std::iter::empty::<&str>());
    }

    #[test]
    fn starts_with_empty_prefix_matches_valid_key() {
        let key = QueryKey::from(["users", "42"]);
        // Bypass QueryKey::new to create an empty key for testing starts_with.
        let empty = QueryKey(Arc::from([] as [Arc<str>; 0]));
        assert!(key.starts_with(&empty));
    }

    #[test]
    fn starts_with_empty_prefix_does_not_match_empty_key() {
        // Bypass QueryKey::new to create an empty key for testing starts_with.
        let empty = QueryKey(Arc::from([] as [Arc<str>; 0]));
        assert!(!empty.starts_with(&empty));
    }

    #[test]
    fn to_path_disambiguates_forward_slash_in_segment() {
        let key_with_slash = QueryKey::from(["a/b"]);
        let key_two_parts = QueryKey::from(["a", "b"]);
        assert_ne!(key_with_slash.to_path(), key_two_parts.to_path());
        assert_eq!(key_with_slash.to_path(), "a/b");
        assert_eq!(key_two_parts.to_path(), "a::b");
    }

    #[test]
    fn to_path_disambiguates_empty_string_segments() {
        let key = QueryKey::from(["", ""]);
        assert_eq!(key.to_path(), "::");
    }
}
