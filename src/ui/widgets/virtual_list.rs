//! Reusable virtualized-list widget wrapping `gpui_component::v_virtual_list`.
//!
//! This module provides helpers for building item-size vectors and computing
//! bounded list heights, plus a single `render_virtual_list` entry point that
//! encapsulates the common plumbing: entity clone, VirtualList construction,
//! scroll-handle tracking, gap, overflow-y-scroll container, and optional
//! scrollbar.

use std::ops::Range;
use std::rc::Rc;

use gpui::{prelude::*, *};
use gpui_component::scroll::{ScrollableElement, ScrollbarAxis};
use gpui_component::{v_flex, v_virtual_list, VirtualListScrollHandle};

// ---------------------------------------------------------------------------
// Item-size helpers
// ---------------------------------------------------------------------------

/// Build uniform item sizes (all items the same height).
///
/// Width is `px(0.)` so that the flex layout inside each row controls width.
pub fn uniform_item_sizes(count: usize, height: Pixels) -> Rc<Vec<Size<Pixels>>> {
    Rc::new(vec![size(px(0.), height); count])
}

/// Build variable item sizes from a slice of heights.
///
/// Width is `px(0.)` so that the flex layout inside each row controls width.
pub fn variable_item_sizes(heights: &[Pixels]) -> Rc<Vec<Size<Pixels>>> {
    Rc::new(heights.iter().map(|&h| size(px(0.), h)).collect())
}

// ---------------------------------------------------------------------------
// Bounded list height
// ---------------------------------------------------------------------------

/// Compute the bounded list height: `min(total_content_height + gaps, max_height)`.
///
/// `gap` is the inter-item gap applied between every pair of consecutive items.
pub fn bounded_list_height(item_sizes: &[Size<Pixels>], gap: Pixels, max_height: Pixels) -> Pixels {
    let content_h: f32 = item_sizes.iter().map(|s| s.height.as_f32()).sum();
    let gap_total = gap.as_f32() * item_sizes.len().saturating_sub(1) as f32;
    px((content_h + gap_total).min(max_height.as_f32()))
}

// ---------------------------------------------------------------------------
// render_virtual_list
// ---------------------------------------------------------------------------

/// Render a virtualized list with all common wiring.
///
/// - `cx`: the view context (used for `cx.entity().clone()`).
/// - `id`: element id for the virtual list.
/// - `item_sizes`: pre-computed sizes for every item.
/// - `list_height`: the container height (e.g. from `bounded_list_height` or a fixed value).
/// - `gap`: inter-item gap; pass `px(0.)` for no gap.
/// - `scroll_handle`: a cloned `VirtualListScrollHandle` (clone from the view field *before*
///   calling this during render).
/// - `show_scrollbar`: whether to attach a vertical scrollbar.
/// - `render_items`: closure `Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<R>`
///   that renders the items in the given visible range.
///
/// ## Constraints
///
/// - `VirtualListScrollHandle` must be cloned from the view field **before** calling this
///   function during render.
/// - Never read entity state via `entity.read(cx)` during render.
/// - Items **must** be pinned to exact declared heights or rows will overlap/gap.
pub fn render_virtual_list<R, V>(
    cx: &mut Context<V>,
    id: impl Into<ElementId>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    list_height: Pixels,
    gap: Pixels,
    scroll_handle: &VirtualListScrollHandle,
    show_scrollbar: bool,
    render_items: impl 'static + Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<R>,
) -> Div
where
    R: IntoElement + 'static,
    V: Render + 'static,
{
    let entity = cx.entity();

    let mut list = v_virtual_list(entity, id, item_sizes, render_items)
        .track_scroll(scroll_handle);

    if gap > px(0.) {
        list = list.gap(gap);
    }

    let mut container = v_flex()
        .relative()
        .w_full()
        .h(list_height)
        .child(list);

    if show_scrollbar {
        container = container.scrollbar(scroll_handle, ScrollbarAxis::Vertical);
    }

    container
}

#[cfg(test)]
#[path = "virtual_list.test.rs"]
mod virtual_list_test;
