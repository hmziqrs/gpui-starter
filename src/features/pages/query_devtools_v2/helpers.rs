use gpui::*;

use gpui_component::{
    Selectable,
    button::Button,
};

use super::dashboard::QueryDevToolsV2Page;

// ---------------------------------------------------------------------------
// Sort mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuerySort {
    Key,
    Status,
    CacheAge,
    CacheHits,
}

// ---------------------------------------------------------------------------
// Sort Button
// ---------------------------------------------------------------------------

pub(super) fn sort_button(
    label: &str,
    target: QuerySort,
    current: QuerySort,
    cx: &mut Context<QueryDevToolsV2Page>,
) -> Button {
    let active = current == target;
    let mut btn = Button::new(format!("v2-sort-{:?}", target))
        .outline()
        .label(label);
    if active {
        btn = btn.selected(true);
    }
    btn.on_click(cx.listener(move |this, _, _, _cx| {
            this.sort_by = target;
            this.expanded_key = None;
            _cx.notify();
        }))
}

// ---------------------------------------------------------------------------
// Filter Button
// ---------------------------------------------------------------------------

pub(super) fn filter_button(
    target: Option<&str>,
    current: &Option<String>,
    cx: &mut Context<QueryDevToolsV2Page>,
) -> Button {
    let label = target.unwrap_or("All");
    let active = match (current, target) {
        (None, None) => true,
        (Some(cur), Some(tgt)) => cur == tgt,
        _ => false,
    };
    let id = format!("v2-filter-{}", target.unwrap_or("all"));
    let target_owned = target.map(|s| s.to_string());
    let mut btn = Button::new(id)
        .outline()
        .label(label);
    if active {
        btn = btn.selected(true);
    }
    btn.on_click(cx.listener(move |this, _, _, _cx| {
            this.status_filter = target_owned.clone();
            this.expanded_key = None;
            _cx.notify();
        }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(super) fn format_cache_age(age_ms: Option<u128>) -> String {
    match age_ms {
        None => "n/a".to_string(),
        Some(ms) => {
            if ms < 1000 {
                format!("{}ms", ms)
            } else if ms < 60_000 {
                format!("{:.1}s", ms as f64 / 1000.0)
            } else if ms < 3_600_000 {
                format!("{:.1}m", ms as f64 / 60_000.0)
            } else {
                format!("{:.1}h", ms as f64 / 3_600_000.0)
            }
        }
    }
}

/// Convert pixels to rems assuming a 16px base font size (Audit Finding 18:
/// this divisor matches GPUI's default but may differ with system config).
pub(super) fn rems_from_px(px: f32) -> Rems {
    Rems(px / 16.0)
}
