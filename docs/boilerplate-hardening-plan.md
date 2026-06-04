# Boilerplate Hardening Plan

## Goal

Close four gaps surfaced while fixing the Query DevTools V2 scroll-lag
regression. Each is independently shippable; together they make the boilerplate
more reusable (shared list widget), more robust (render error boundary, slow
frame visibility), and closer to production-ready (auto-update).

Ordered by recommended sequencing:

1. Reusable virtualized-list widget (`ui/widgets`)
2. Page-level render error boundary
3. Slow-frame instrumentation
4. Auto-updater

## Out Of Scope

- Rewriting existing pages beyond what each item explicitly lists.
- A general-purpose table/data-grid abstraction (the list widget is rows, not columns).
- Remote crash-report upload (telemetry already exists; this plan only adds
  in-process recovery, not a reporting backend).
- Update server / release-artifact hosting infrastructure (item 4 covers the
  client only).

## Background

The DevTools V2 registry was a non-virtualized `v_flex().children(all_rows)`
inside the page's `overflow_y_scroll`, so every scroll frame ran full Taffy
layout + paint over every query row. It was migrated to `v_virtual_list`
(gpui-component) with a bounded scroll region. See
`src/features/pages/query_devtools_v2.rs` (`render_query_registry`) for the
reference implementation, and `docs/gpui-performance.md` for the render-loop
rules this plan stays inside.

---

## 1. Reusable virtualized-list widget

### Current state

- `src/ui/widgets/mod.rs` and `src/ui/components/mod.rs` are empty stubs.
- The `v_virtual_list` setup (scroll handle on the view, `item_sizes`
  precompute, height pinning, bounded scroll container, `.scrollbar(...)`) lives
  inline in `query_devtools_v2.rs`.
- The playground page (`src/features/pages/query_playground.rs`) has its own
  growing lists that still need the same treatment.

### Why

The `v_virtual_list` contract has a sharp edge: items are positioned purely from
the declared `item_sizes`, so every rendered row/detail must be pinned to the
exact height it declares or rows overlap / gap. That gotcha should be solved
once, in one place, not re-derived per page.

### Proposed shape

Add `src/ui/widgets/virtual_list.rs` exposing a thin wrapper that owns the
common wiring:

- holds a `VirtualListScrollHandle` (caller stores it on their view, or the
  widget takes `&VirtualListScrollHandle` like the DevTools page does — handle
  must be threaded from `render()`, never read back via `entity.read(cx)` during
  render; that double-leases and aborts the process).
- accepts a slice/`Rc<Vec<T>>`, a `row_height: fn(&T) -> Pixels` (or fixed
  height for the uniform case), and a render closure `Fn(&T, ...) -> impl IntoElement`.
- computes `item_sizes`, the snug-vs-capped list height, and renders the bounded
  scroll region + scrollbar internally.
- supports the expandable-row case (row + optional detail) since that is why
  DevTools needs variable heights.

### Files

- `src/ui/widgets/virtual_list.rs` (new)
- `src/ui/widgets/mod.rs` (export)
- `src/features/pages/query_devtools_v2.rs` (migrate `render_query_registry` to
  the widget; delete the inline `REGISTRY_*` height plumbing or move it behind
  the widget)
- `src/features/pages/query_playground.rs` (adopt for its growing lists)

### Acceptance criteria

- DevTools V2 registry renders identically (rows + inline expand) through the
  widget, with no regression to the scroll-lag fix.
- Playground's unbounded lists are virtualized via the same widget.
- Adding a third virtualized list is < ~10 lines at the call site.

---

## 2. Page-level render error boundary

### Current state

- `src/app/lifecycle.rs` installs a panic hook (`install_panic_hook`) that
  captures a summary into `LAST_PANIC_SUMMARY`, but the process still aborts.
- A render panic in a single page (e.g. the `entity.read(cx)` double-lease hit
  during this work) takes down the whole app.
- Active page is dispatched in `src/shell/root.rs` (`active_page_view` /
  `page_for_render`).

### Why

One buggy page render should degrade to a fallback panel, not kill every other
page, unsaved state, and open windows.

### Proposed shape

Wrap the active page view at the dispatch boundary in `shell/root.rs` so a
render failure renders a fallback ("This page failed to render — <summary> /
Reload") instead of propagating.

Spike outcome — the route-swap (next-frame) approach was chosen:

- Inline `catch_unwind` was not attempted because GPUI render uses an
  `AtomicBool` flag (`RENDER_PANIC_OCCURRED`) to detect panics rather than
  catching them inline. The render path is driven through `stacksafe`/`stacker`,
  and a `catch_unwind` around `render()` would interact unpredictably with the
  global panic hook.
- **Chosen approach**: the panic hook sets an `AtomicBool`; on the next
  `active_page_view` call, the flag is read and the view is swapped to
  `RenderErrorPage`. A thread-local `IN_RENDER_PATH` guard ensures only panics
  that originate inside the render path (not background tasks, init, etc.) set
  the flag, preventing false error-boundary activation.
- This is safe because the default `panic=unwind` mode lets the current frame
  abort cleanly and the next frame renders the fallback.
- `last_panic_summary()` is reused for the fallback's detail text.

### Files

- `src/shell/root.rs` (wrap `active_page_view`)
- `src/app/lifecycle.rs` (expose hook state if the spike needs it)
- possibly a new `src/features/pages/render_error.rs` fallback view

### Acceptance criteria

- A deliberately panicking page (there is already a `TriggerTestPanic` action in
  `src/app/mod.rs` to lean on) shows a fallback panel; the rest of the app stays
  interactive.
- "Reload" re-attempts the page render.
- If the spike shows inline recovery is not viable under GPUI, document the
  finding here and fall back to the route-swap approach.

---

## 3. Slow-frame instrumentation

### Current state

- `src/shell/root.rs:440` already logs `elapsed_us` for every AppRoot render via
  `render_started.elapsed()`.
- There is no threshold, so a perf regression (like the original lag) is only
  noticed by feel, never by signal.

### Why

Cheapest item on the list. Turns the existing measurement into an automatic
regression alarm.

### Proposed shape

- Add a frame-time threshold (start at ~3–4ms; tune). When a render exceeds it,
  emit a `warn!` instead of (or in addition to) the existing `debug!`, including
  the route so the offending page is obvious.
- Optional: a dev-only on-screen frame-time readout, gated behind a debug build
  or a config/settings flag, so it is visible without tailing logs.

### Files

- `src/shell/root.rs` (threshold + warn around line 440)
- optionally `src/shell/status_bar.rs` for an opt-in readout

### Acceptance criteria

- A render over the threshold logs a `warn!` with route and `elapsed_us`.
- Normal frames stay at `debug!` (no log spam at 120fps).

---

## 4. Auto-updater

### Current state

- No self-update path in `src/services/` or `src/platform/`.
- Everything else (single-instance, tray, secure storage, telemetry,
  persistence) is production-grade; this is the conspicuous gap for shipping to
  real users.

### Why

Without it there is no way to get fixes to installed users short of a manual
reinstall.

### Proposed shape

- macOS-first (matches the rest of the platform code): either integrate Sparkle
  via a Rust binding, or a custom "check signed manifest → download → verify →
  swap on relaunch" flow.
- Capability-aware and degraded-mode friendly, mirroring the
  `native-notifications-plan.md` backend shape (per-platform backend, explicit
  "updates unavailable" state).
- Surface update state through the existing notification/status-bar plumbing
  rather than a bespoke UI.

### Files

- `src/services/updater/` (new service module)
- `src/services/mod.rs` (register)
- `src/shell/status_bar.rs` and/or notifications for surfacing state
- update endpoint/manifest config in the existing config store

### Acceptance criteria

- App can detect a newer published version, download, verify signature, and
  apply on relaunch.
- Clean degraded behavior when the update channel is unreachable or the platform
  backend is unavailable.

> Note: update-server / artifact-hosting infra is out of scope here and needs a
> separate ops plan.

---

## Sequencing

1. **Item 1 (list widget)** — do first; it pays for itself the moment the
   playground is virtualized, and it is low-risk.
2. **Item 3 (slow-frame warn)** — trivial, can land alongside item 1.
3. **Item 2 (error boundary)** — needs the feasibility spike before committing
   to an approach.
4. **Item 4 (updater)** — largest surface; schedule independently.

When an item ships, move the relevant section (or this whole doc, if all are
done) into `docs/completed/` per the existing convention.
