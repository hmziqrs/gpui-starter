use crate::core::*;

pub fn make_resource() -> InfiniteQueryResource<Vec<String>> {
    InfiniteQueryResource::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    )
}

pub fn make_bidirectional_resource() -> InfiniteQueryResource<Vec<String>> {
    InfiniteQueryResource::new_bidirectional(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    )
}

/// Convenience: load N pages via `begin_fetch_next` + `complete_page_success`.
/// Each page contains a single element `format!("page{i}")`.
/// Returns the resource with pages loaded.
pub fn load_n_pages(n: usize) -> InfiniteQueryResource<Vec<String>> {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();
    for i in 0..n {
        let has_more = i < n - 1;
        let id = r.begin_fetch_next(&mut seq, (i * 100) as u128).unwrap();
        r.complete_page_success(
            id,
            vec![format!("page{i}")],
            has_more,
            true,
            ((i + 1) * 100) as u128,
        );
    }
    r
}
