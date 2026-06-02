# Query DevTools & Cache Explorer

> A live diagnostic dashboard for visualizing and manipulating the QueryClient registry.

---

## Table of Contents

1. [Overview](#overview)
2. [UI Layout](#ui-layout)
3. [Data Source](#data-source)
4. [Component Design](#component-design)
5. [Registration](#registration)
6. [Verification](#verification)

---

## Overview

The gpui-query crate exposes a DevTools diagnostics API (`QueryClient::diagnostics()`) that returns structured snapshots of all query state. This document describes a new **Query DevTools** page that consumes this API to provide:

- **Overview dashboard** — total resources, buckets, mutations at a glance
- **Cache explorer** — sortable, filterable table of all query resources with click-to-expand detail panels
- **Action bar** — one-click cache manipulation (invalidate, reset, GC, remove, clear)
- **Live refresh** — auto-updates when QueryClient state changes
- **Empty state** — graceful fallback when no query resources exist

---

## UI Layout

```
v_flex().min_h_full().p_6().gap_5()
├── Hero Banner
│   └── "Query DevTools" title + description (muted bg, bordered card)
│
├── Empty State (shown when no QueryClient registered or 0 resources)
│   └── Icon + "No Query Resources" + instructions to visit HTTP Lab Testing
│
├── Overview Dashboard (3 stat cards in horizontal row)
│   ├── Total Resources    (diag.total_resources)
│   ├── Type Buckets       (diag.bucket_count)
│   └── Mutations          (diag.mutation_count)
│
├── Actions (section card with button row, disabled when no QueryClient)
│   ├── [Invalidate All]
│   ├── [Reset All]
│   ├── [GC]
│   ├── [Remove All]
│   └── [Clear]
│
└── Query Registry (section card — the cache explorer)
    ├── Filter controls
    │   ├── Sort: [By Key] [By Status] [By Cache Hits]
    │   └── Status: [All] [Idle] [Loading] [Success] [Failure] [Cancelled]
    │
    └── Table rows (one per QueryDiagnostic)
        ├── Clickable row: status badge | key | data/error dots | policies | cache hits
        └── Expanded detail (on click): all 11 fields as key-value pairs in muted panel
```

---

## Data Source

### API Access Pattern

`QueryClient` is a GPUI `Global` registered lazily by `http_lab_testing.rs`. It may not exist when the page first loads. Access pattern:

```rust
// In render — read-only, safe on &App
if let Some(client) = cx.try_global::<gpui_query::client::QueryClient>() {
    let diag = client.diagnostics(cx);
    // render dashboard + registry
} else {
    // render empty state
}
```

### ClientDiagnostic

| Field | Type | Description |
|---|---|---|
| `total_resources` | `usize` | Total query resources across all type buckets |
| `bucket_count` | `usize` | Number of type-partitioned buckets |
| `mutation_count` | `usize` | Total mutation resources |
| `queries` | `Vec<QueryDiagnostic>` | Per-query diagnostics |

### QueryDiagnostic

| Field | Type | Display |
|---|---|---|
| `key` | `String` | Monospace text |
| `status` | `String` | Colored badge (Idle=gray, Success=primary, Loading=primary, Failure=destructive, Cancelled=muted) |
| `has_data` | `bool` | Dot indicator `[data]` in primary color |
| `has_error` | `bool` | Dot indicator `[error]` in destructive color |
| `cache_policy` | `String` | Text chip |
| `request_policy` | `String` | Text chip |
| `cache_hits` | `u64` | Number |
| `cancelled_count` | `u64` | Number |
| `ignored_results` | `u64` | Number |
| `last_updated_at_ms` | `Option<u128>` | Formatted timestamp or "N/A" |
| `started_at_ms` | `Option<u128>` | Formatted timestamp or "N/A" |

---

## Component Design

### Imports

```rust
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _,
    Icon, IconName,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_query::client::QueryClient;
use gpui_query::QueryKeyFilter;
```

### Struct

```rust
pub struct QueryDevToolsPage {
    _subscriptions: Vec<Subscription>,
    expanded_key: Option<String>,       // which row is expanded
    sort_by: QuerySort,                 // ByKey | ByStatus | ByCacheHits
    status_filter: Option<String>,      // None = show all
}

enum QuerySort {
    ByKey,
    ByStatus,
    ByCacheHits,
}
```

### Constructor

**Critical:** `QueryClient` is not registered at app startup. It's lazily created by `http_lab_testing.rs`. The constructor must handle its absence. Two approaches:

**Option A (recommended) — Register QueryClient eagerly in `AppRoot::new()`:**
```rust
// In AppRoot::new(), alongside other global registrations:
if !cx.has_global::<gpui_query::client::QueryClient>() {
    cx.set_global(gpui_query::client::QueryClient::new(
        gpui_query::CachePolicy::default(),
        gpui_query::RequestPolicy::default(),
    ));
}
```
Then the constructor can safely observe it:
```rust
pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
    let subscriptions = vec![
        cx.observe_global_in::<gpui_query::client::QueryClient>(window, |_, _, cx| {
            cx.notify();
        }),
    ];
    Self {
        _subscriptions: subscriptions,
        expanded_key: None,
        sort_by: QuerySort::ByKey,
        status_filter: None,
    }
}
```

**Option B — Guard the subscription, add manual refresh:**
```rust
let mut subscriptions = Vec::new();
if cx.has_global::<gpui_query::client::QueryClient>() {
    subscriptions.push(
        cx.observe_global_in::<gpui_query::client::QueryClient>(window, |_, _, cx| {
            cx.notify();
        }),
    );
}
// Plus a "Refresh" button for manual re-poll when QueryClient appears later
```

### Data Flow

1. Render reads `cx.try_global::<QueryClient>()`
2. If `None` → render empty state
3. If `Some(client)` → call `client.diagnostics(cx)` → `ClientDiagnostic`
4. Clone `queries`, apply `status_filter` + `sort_by`
5. Render table rows with click-to-expand
6. Action buttons mutate QueryClient → `cx.notify()` → re-render

### Action Button Pattern

All mutation methods require `&mut self` on `QueryClient`. Must use `cx.update_global`:

```rust
// Every action button follows this pattern:
Button::new("devtools-invalidate-all")
    .outline()
    .label("Invalidate All")
    .on_click(cx.listener(|_, _, _, cx| {
        if cx.has_global::<QueryClient>() {
            cx.update_global::<QueryClient, _>(|client, cx| {
                client.invalidate_queries(&QueryKeyFilter::All, cx);
            });
            cx.notify();
        }
    }))
```

Method signatures (all require `&mut self` via `cx.update_global`):

| Method | Actual Signature |
|---|---|
| `invalidate_queries` | `(&mut self, filter: &QueryKeyFilter, cx: &mut App)` |
| `reset_queries` | `(&mut self, filter: &QueryKeyFilter, cx: &mut App)` |
| `remove_queries` | `(&mut self, filter: &QueryKeyFilter, cx: &mut App)` |
| `gc` | `(&mut self, cx: &mut App, now_ms: u128)` |
| `clear` | `(&mut self)` — no `cx` parameter |

**Note:** `QueryKeyFilter` has a lifetime parameter but `All` is a unit variant — pass as `&QueryKeyFilter::All`.

**Note:** `gc` needs a `now_ms: u128` timestamp. Use:
```rust
let now_ms = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis();
```

**Note:** All action button handlers must guard with `cx.has_global::<QueryClient>()` before calling `cx.update_global`, since `update_global` panics if the global doesn't exist.

### Status Badge Colors

Uses theme colors only (no hardcoded values):

| Status | Theme Property | Visual |
|---|---|---|
| Idle | `muted_foreground` | Gray text |
| LoadingEmpty / LoadingWithData | `primary` | Accent text |
| Success | `primary` | Accent text |
| Failure | `destructive` | Red text |
| Cancelled | `muted_foreground` | Gray text |

### Sort & Filter Logic

```rust
let mut queries = diag.queries.clone();
if let Some(ref filter) = self.status_filter {
    queries.retain(|q| &q.status == filter);
}
match self.sort_by {
    QuerySort::ByKey => queries.sort_by(|a, b| a.key.cmp(&b.key)),
    QuerySort::ByStatus => queries.sort_by(|a, b| a.status.cmp(&b.status)),
    QuerySort::ByCacheHits => queries.sort_by(|a, b| b.cache_hits.cmp(&a.cache_hits)),
}
```

---

## Registration

6 files to touch (1 new, 5 modified):

| File | Change |
|---|---|
| `src/features/pages/query_devtools.rs` | **CREATE** — full page (~400 lines) |
| `src/features/pages/mod.rs` | Add `mod query_devtools;` + `pub use query_devtools::QueryDevToolsPage;` |
| `src/shell/sidebar.rs` | Add `Page::QueryDevTools` variant, title, icon, add to `all()` |
| `src/shell/route.rs` | Add `"query-devtools"` deep link host, URL mapping, parse arm |
| `src/shell/root.rs` | Add entity field + init + routing; **eagerly register QueryClient global** |
| `src/shell/route.test.rs` | Add deep link roundtrip test |

### Sidebar

In `src/shell/sidebar.rs` — add `QueryDevTools` variant to `Page` enum (before `About`):

```rust
Page::QueryDevTools => "Query DevTools",
Page::QueryDevTools => IconName::Search,
```

Add to `Page::all()` array.

### Deep Link

In `src/shell/route.rs`:

- Add `"query-devtools"` to `VALID_HOSTS`
- `to_url()`: `Self::Page(Page::QueryDevTools) => "gpui-starter://query-devtools".to_string()`
- `parse_deep_link()`: `("query-devtools", []) => Ok(Self::Page(Page::QueryDevTools))`

### Root Wiring

In `src/shell/root.rs`:

```rust
// Import
use crate::views::QueryDevToolsPage;

// Field on AppRoot
query_devtools_page: Entity<QueryDevToolsPage>,

// In AppRoot::new() — eagerly register QueryClient
if !cx.has_global::<gpui_query::client::QueryClient>() {
    cx.set_global(gpui_query::client::QueryClient::new(
        gpui_query::CachePolicy::default(),
        gpui_query::RequestPolicy::default(),
    ));
}

// Create entity
let query_devtools_page = cx.new(|window, cx| QueryDevToolsPage::new(window, cx));

// Route in active_page_view()
Page::QueryDevTools => self.query_devtools_page.clone().into(),
```

### Module Declaration

In `src/features/pages/mod.rs` — add alphabetically:

```rust
mod query_devtools;
pub use query_devtools::QueryDevToolsPage;
```

---

## Verification

1. `cargo check -p gpui-starter` — compiles clean
2. `cargo test -p gpui-starter` — all tests pass (including new route test)
3. Run the app, navigate to Query DevTools via sidebar
4. Verify empty state shows (no resources yet)
5. Navigate to HTTP Lab Testing, send a request, return to Query DevTools
6. Verify registry shows resources with correct status and details
7. Test action buttons: Invalidate All, Reset All, GC, Remove All, Clear
8. Test click-to-expand detail panel
9. Test sort buttons reorder the registry
10. Test status filter buttons filter correctly
