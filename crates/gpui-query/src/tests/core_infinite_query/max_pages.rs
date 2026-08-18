//! Tests for max_pages enforcement, edge cases, and evicted pages returns.

use crate::core::*;
use super::helpers::*;

// ── 4. max_pages enforcement ────────────────────────────────────────────

#[test]
fn max_pages_evicts_oldest_page_on_append() {
    let mut r = make_resource();
    r.set_max_pages(Some(2));
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
    let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);

    // Third page exceeds max_pages=2, evicts oldest ("a")
    let id3 = r.begin_fetch_next(&mut seq, 5_000).unwrap();
    r.complete_page_success(id3, vec!["c".to_string()], false, true, 6_000);

    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["b".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["c".to_string()]));
}

#[test]
fn max_pages_evicts_newest_page_on_prepend() {
    let mut r = make_resource();
    r.set_max_pages(Some(2));
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
    let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);

    // Prepend a page: ["c", "a", "b"] enforced to 2 removes from back => ["c", "a"]
    r.set_has_previous_page(true);
    let id3 = r.begin_fetch_previous(&mut seq, 5_000).unwrap();
    r.complete_page_success(id3, vec!["c".to_string()], false, false, 6_000);

    assert_eq!(r.page_count(), 2);
    assert_eq!(r.pages()[0], vec!["c".to_string()]);
    assert_eq!(r.pages()[1], vec!["a".to_string()]);
}

// ── 5. max_pages edge cases ─────────────────────────────────────────────

#[test]
fn max_pages_zero_treated_as_unbounded() {
    let mut r = load_n_pages(3);

    // v2 audit 2: Some(0) is treated as None (unbounded) — no eviction
    r.set_max_pages(Some(0));
    assert_eq!(r.max_pages(), None);
    assert_eq!(r.page_count(), 3);
}

#[test]
fn max_pages_one_retains_only_latest_page() {
    let mut r = make_resource();
    r.set_max_pages(Some(1));
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["a".to_string()], true, true, 2_000);
    let id2 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    r.complete_page_success(id2, vec!["b".to_string()], true, true, 4_000);

    // Only the last page is retained
    assert_eq!(r.page_count(), 1);
    assert_eq!(r.first_page(), Some(&vec!["b".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["b".to_string()]));
}

#[test]
fn max_pages_default_is_50() {
    let r = make_resource();
    assert_eq!(r.max_pages(), Some(50));
}

#[test]
fn max_pages_50_allows_50_pages_and_evicts_on_51st() {
    let mut r = make_resource();
    assert_eq!(r.max_pages(), Some(50));
    let mut seq = RequestSequencer::new();

    // Load 50 pages — all with has_more=true so has_next_page stays true
    for i in 0..50 {
        let id = r.begin_fetch_next(&mut seq, (i * 100) as u128).unwrap();
        r.complete_page_success(
            id,
            vec![format!("p{i}")],
            true, // always report more pages available
            true,
            ((i + 1) * 100) as u128,
        );
    }
    assert_eq!(r.page_count(), 50);
    assert_eq!(r.first_page(), Some(&vec!["p0".to_string()]));

    // 51st page evicts p0
    let id51 = r.begin_fetch_next(&mut seq, 5_000_000).unwrap();
    r.complete_page_success(id51, vec!["p50".to_string()], false, true, 5_000_100);
    assert_eq!(r.page_count(), 50);
    assert_eq!(r.first_page(), Some(&vec!["p1".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["p50".to_string()]));
}

#[test]
fn set_max_pages_returns_evicted_pages() {
    let mut r = load_n_pages(3);

    let evicted = r.set_max_pages(Some(2));
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0], vec!["page0".to_string()]);
    assert_eq!(r.page_count(), 2);
}

#[test]
fn append_page_returns_evicted_pages() {
    let mut r = make_resource();
    r.set_max_pages(Some(2));

    assert!(r.append_page(vec!["a".to_string()]).is_empty());
    assert!(r.append_page(vec!["b".to_string()]).is_empty());

    let evicted = r.append_page(vec!["c".to_string()]);
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0], vec!["a".to_string()]);
    assert_eq!(r.page_count(), 2);
}

#[test]
fn prepend_page_returns_evicted_pages() {
    let mut r = make_resource();
    r.set_max_pages(Some(2));

    r.prepend_page(vec!["a".to_string()]);
    r.prepend_page(vec!["b".to_string()]);

    let evicted = r.prepend_page(vec!["c".to_string()]);
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0], vec!["a".to_string()]);
    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["c".to_string()]));
}
