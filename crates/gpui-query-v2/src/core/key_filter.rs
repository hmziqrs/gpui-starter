use super::QueryKey;

/// A filter for matching query keys, used by bulk operations like
/// `invalidate_queries`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryKeyFilter<'a> {
    /// Match only the key that is exactly equal.
    Exact(&'a QueryKey),
    /// Match all keys that start with the given prefix.
    Prefix(&'a QueryKey),
    /// Match every key.
    All,
}

impl<'a> QueryKeyFilter<'a> {
    /// Returns `true` if the given key matches this filter.
    pub fn matches(&self, key: &QueryKey) -> bool {
        match self {
            Self::Exact(k) => key == *k,
            Self::Prefix(k) => key.starts_with(k),
            Self::All => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(parts: &[&str]) -> QueryKey {
        QueryKey::new(parts)
    }

    #[test]
    fn exact_matches_same_key() {
        let k = key(&["users", "42"]);
        assert!(QueryKeyFilter::Exact(&k).matches(&k));
    }

    #[test]
    fn exact_rejects_different_key() {
        let k1 = key(&["users", "42"]);
        let k2 = key(&["users", "43"]);
        assert!(!QueryKeyFilter::Exact(&k1).matches(&k2));
    }

    #[test]
    fn prefix_matches_child_key() {
        let prefix = key(&["users"]);
        let child = key(&["users", "42", "posts"]);
        assert!(QueryKeyFilter::Prefix(&prefix).matches(&child));
    }

    #[test]
    fn all_matches_everything() {
        let k = key(&["anything"]);
        assert!(QueryKeyFilter::All.matches(&k));
    }
}
