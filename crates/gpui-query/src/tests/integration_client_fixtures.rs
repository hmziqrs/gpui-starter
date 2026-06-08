//! Shared test fixtures for integration client tests.

// ── Test fixtures ──────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct User {
    pub(crate) id: u32,
    pub(crate) name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Post {
    pub(crate) id: u32,
    pub(crate) title: String,
}

pub(crate) fn default_user() -> User {
    User {
        id: 1,
        name: "Alice".into(),
    }
}

pub(crate) fn default_post() -> Post {
    Post {
        id: 1,
        title: "Hello World".into(),
    }
}
