use super::*;

// ---------------------------------------------------------------------------
// uniform_item_sizes
// ---------------------------------------------------------------------------

#[test]
fn uniform_item_sizes_correct_count() {
    let sizes = uniform_item_sizes(5, px(30.));
    assert_eq!(sizes.len(), 5);
}

#[test]
fn uniform_item_sizes_zero_count() {
    let sizes = uniform_item_sizes(0, px(30.));
    assert!(sizes.is_empty());
}

#[test]
fn uniform_item_sizes_each_element_height() {
    let height = px(42.);
    let sizes = uniform_item_sizes(3, height);
    for s in sizes.iter() {
        assert_eq!(s.height, height);
        // Width is px(0.) so the flex layout controls width.
        assert_eq!(s.width, px(0.));
    }
}

// ---------------------------------------------------------------------------
// variable_item_sizes
// ---------------------------------------------------------------------------

#[test]
fn variable_item_sizes_matches_input_lengths() {
    let heights = vec![px(10.), px(20.), px(30.)];
    let sizes = variable_item_sizes(&heights);
    assert_eq!(sizes.len(), heights.len());
}

#[test]
fn variable_item_sizes_per_element_heights() {
    let heights = vec![px(10.), px(25.), px(40.)];
    let sizes = variable_item_sizes(&heights);
    for (s, &h) in sizes.iter().zip(heights.iter()) {
        assert_eq!(s.height, h);
        assert_eq!(s.width, px(0.));
    }
}

// ---------------------------------------------------------------------------
// bounded_list_height
// ---------------------------------------------------------------------------

#[test]
fn bounded_list_height_no_gap() {
    let items = vec![size(px(0.), px(100.)), size(px(0.), px(200.))];
    let result = bounded_list_height(&items, px(0.), px(500.));
    assert_eq!(result, px(300.));
}

#[test]
fn bounded_list_height_with_gap() {
    let items = vec![size(px(0.), px(100.)), size(px(0.), px(100.)), size(px(0.), px(100.))];
    // 3 items * 100px = 300px + 2 gaps * 10px = 20px => 320px
    let result = bounded_list_height(&items, px(10.), px(500.));
    assert_eq!(result, px(320.));
}

#[test]
fn bounded_list_height_clamps_to_max() {
    let items = vec![size(px(0.), px(200.)), size(px(0.), px(200.))];
    // Total 400px but max is 150px
    let result = bounded_list_height(&items, px(0.), px(150.));
    assert_eq!(result, px(150.));
}

#[test]
fn bounded_list_height_single_item() {
    let items = vec![size(px(0.), px(75.))];
    let result = bounded_list_height(&items, px(10.), px(500.));
    assert_eq!(result, px(75.));
}

#[test]
fn bounded_list_height_empty() {
    let items: Vec<Size<Pixels>> = vec![];
    let result = bounded_list_height(&items, px(10.), px(500.));
    assert_eq!(result, px(0.));
}
