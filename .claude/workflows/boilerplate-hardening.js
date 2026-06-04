export const meta = {
  name: 'boilerplate-hardening',
  description: 'Implement all 4 items from the boilerplate hardening plan',
  phases: [
    { title: 'Implement Core' },
    { title: 'Error Boundary' },
    { title: 'Auto-Updater' },
    { title: 'Compile & Fix' },
  ],
};

// Phase 1: Parallel — Virtual list widget + Slow-frame warn + Error boundary spike
phase('Implement Core');

const SPIKE_SCHEMA = {
  type: 'object',
  properties: {
    approach: { type: 'string', enum: ['inline', 'route-swap'] },
    feasible: { type: 'boolean' },
    reasoning: { type: 'string' },
    catch_unwind_works: { type: 'boolean' },
    unwind_safe: { type: 'boolean' },
    alternative_needed: { type: 'boolean' },
  },
  required: ['approach', 'feasible', 'reasoning', 'catch_unwind_works', 'unwind_safe', 'alternative_needed'],
};

const [virtualListResult, slowFrameResult, spikeResult] = await parallel([

  // Agent A: Reusable virtualized-list widget
  () => agent(
    [
      'You are implementing a reusable virtualized-list widget for a GPUI Rust application.',
      '',
      'TASK: Create src/ui/widgets/virtual_list.rs encapsulating the common v_virtual_list wiring.',
      '',
      'STEP 1: Read these files to understand the exact patterns:',
      '- vendor/gpui-component/crates/ui/src/virtual_list.rs (the v_virtual_list vendor API)',
      '- src/features/pages/query_devtools_v2.rs (reference implementation, especially render_query_registry and REGISTRY_* constants)',
      '- src/features/pages/query_playground.rs (second usage in render_activity_log)',
      '- src/ui/widgets/mod.rs (currently empty stub)',
      '- src/ui/mod.rs (module declarations)',
      '- Cargo.toml (dependencies)',
      '',
      'STEP 2: Create src/ui/widgets/virtual_list.rs with these public items:',
      '',
      '  uniform_item_sizes(count: usize, height: Pixels) -> Rc<Vec<Size<Pixels>>>',
      '    Build uniform item sizes (all items same height). Width is px(0.) to let flex layout handle it.',
      '',
      '  variable_item_sizes(heights: &[Pixels]) -> Rc<Vec<Size<Pixels>>>',
      '    Build variable item sizes from a slice of heights. Width is px(0.).',
      '',
      '  bounded_list_height(item_sizes: &[Size<Pixels>], gap: Pixels, max_height: Pixels) -> Pixels',
      '    Compute the bounded list height: min(total_content_height + gaps, max_height)',
      '',
      '  render_virtual_list<R, V>(cx, id, item_sizes, list_height, gap, scroll_handle, show_scrollbar, render_items) -> Div',
      '    The main entry point wrapping v_virtual_list with all common plumbing.',
      '    Gets cx.entity().clone(), creates VirtualList with render_items, calls .track_scroll(scroll_handle),',
      '    sets gap on the VirtualList if gap > 0, wraps in v_flex().h(list_height).overflow_y_scroll().child(list),',
      '    optionally adds .scrollbar(scroll_handle, ScrollbarAxis::Vertical).',
      '    Where R: Render + static_lifetime, V: IntoElement + static_lifetime.',
      '',
      'Imports: use gpui::*; use ui::virtual_list::{v_virtual_list, VirtualListScrollHandle};',
      'use ui::scrollbar::ScrollbarAxis; use std::rc::Rc;',
      '',
      'STEP 3: Update src/ui/widgets/mod.rs to export:',
      '  pub mod virtual_list;',
      '  pub use virtual_list::{render_virtual_list, uniform_item_sizes, variable_item_sizes, bounded_list_height};',
      '',
      'STEP 4: Migrate src/features/pages/query_devtools_v2.rs:',
      '- Add import: use crate::ui::widgets::{render_virtual_list, variable_item_sizes, bounded_list_height};',
      '- Keep the REGISTRY_* constants (they are still needed for row height pinning)',
      '- Replace the v_virtual_list block in render_query_registry with render_virtual_list call',
      '- Use variable_item_sizes for the per-item heights',
      '- Use bounded_list_height for the capped height',
      '- show_scrollbar: true, gap: px(2.) (matching gap_0p5 = 2px)',
      '- Keep scroll_handle usage the same (cloned from self, passed to widget)',
      '- Keep the .h(px(REGISTRY_ROW_H)) pinning on individual rows',
      '',
      'STEP 5: Migrate src/features/pages/query_playground.rs render_activity_log:',
      '- Add import for the widget helpers',
      '- Replace inline v_virtual_list with render_virtual_list call',
      '- Use uniform_item_sizes for the fixed 20px items',
      '- show_scrollbar: false, gap: px(0.)',
      '- list_height: px(200.) (fixed height, not capped)',
      '',
      'CONSTRAINTS:',
      '- VirtualListScrollHandle must be cloned from view field BEFORE calling this function during render',
      '- Never read entity state via entity.read(cx) during render',
      '- Items MUST be pinned to exact declared heights or rows overlap/gap',
      '',
      'Write all the code. Make sure imports and types match existing patterns.',
    ].join('\n'),
    { label: 'virtual-list-widget', phase: 'Implement Core' }
  ),

  // Agent B: Slow-frame instrumentation
  () => agent(
    [
      'You are adding slow-frame instrumentation to a GPUI Rust application.',
      '',
      'TASK: Add a frame-time threshold warning to the render loop in src/shell/root.rs.',
      '',
      'STEP 1: Read src/shell/root.rs fully. Find the Render impl for AppRoot, the timing log near the end.',
      'It currently logs elapsed_us at debug level.',
      '',
      'STEP 2: Edit the file with these minimal changes:',
      '',
      '1. Extract elapsed_us into a variable before the debug log:',
      '   let elapsed_us = render_started.elapsed().as_micros() as u64;',
      '',
      '2. Keep the existing debug! log using the extracted variable.',
      '',
      '3. After the debug log, add:',
      '   const SLOW_FRAME_THRESHOLD_US: u64 = 4_000; // 4ms',
      '   if elapsed_us > SLOW_FRAME_THRESHOLD_US {',
      '       tracing::warn!(',
      '           target: "gpui_starter::root::render",',
      '           route = %self.active_route.title(),',
      '           elapsed_us,',
      '           threshold_us = SLOW_FRAME_THRESHOLD_US,',
      '           "slow frame detected"',
      '       );',
      '   }',
      '',
      'Make ONLY this change. Do not modify anything else in the file.',
    ].join('\n'),
    { label: 'slow-frame-warn', phase: 'Implement Core' }
  ),

  // Agent C: Error boundary feasibility spike
  () => agent(
    [
      'You are researching whether catch_unwind can work in GPUI render path for a page-level error boundary.',
      '',
      'TASK: Investigate feasibility of wrapping page view rendering in std::panic::catch_unwind.',
      '',
      'STEP 1: Search the vendor/ directory for how GPUI calls render:',
      '- grep -r "catch_unwind" vendor/',
      '- grep -r "UnwindSafe" vendor/',
      '- grep -r "stacker" vendor/',
      '- grep -r "stacksafe" vendor/',
      '- grep -rn "fn render" in the GPUI core crate to find where Render::render is invoked',
      '- Check if Window, Context, or App types implement UnwindSafe',
      '',
      'STEP 2: Read src/app/lifecycle.rs and src/shell/root.rs to understand the panic hook and render dispatch.',
      '',
      'STEP 3: Analyze and return findings as JSON:',
      '- Does GPUI already catch panics during rendering?',
      '- Can we wrap active_page_view() in catch_unwind?',
      '- Are Window/Context UnwindSafe?',
      '- What alternative would work if inline catch_unwind is not viable?',
    ].join('\n'),
    {
      label: 'error-boundary-spike',
      phase: 'Implement Core',
      schema: SPIKE_SCHEMA,
    }
  ),
]);

log(
  'Phase 1 complete. Virtual list: ' + (virtualListResult ? 'done' : 'failed') +
  ', Slow frame: ' + (slowFrameResult ? 'done' : 'failed') +
  ', Spike: ' + (spikeResult ? 'done' : 'failed')
);

// Phase 2: Error boundary implementation
phase('Error Boundary');

const spikeInfo = spikeResult ? JSON.stringify(spikeResult) : 'Spike did not return results. Use route-swap approach.';

const errorBoundaryResult = await agent(
  [
    'You are implementing a page-level render error boundary for a GPUI Rust application.',
    '',
    'SPIKE RESULTS: ' + spikeInfo,
    '',
    'TASK: Implement an error boundary so a panic during page rendering shows a fallback panel instead of crashing the app.',
    '',
    'APPROACH: Use route-swap approach (safest, guaranteed to work):',
    '- Set a flag when render panic is detected',
    '- On next render, show error page instead of the failing page',
    '- Allow reload to retry the original page',
    '',
    'STEP 1: Read these files:',
    '- src/shell/root.rs (AppRoot struct, Render impl, active_page_view, full render method)',
    '- src/app/lifecycle.rs (panic hook, LAST_PANIC_SUMMARY, last_panic_summary())',
    '- src/shell/route.rs (AppRoute, Page enum)',
    '- src/features/pages/mod.rs (page module organization)',
    '- Any existing page file for import/style patterns',
    '',
    'STEP 2: Add a static AtomicBool in src/app/lifecycle.rs for render-panic detection:',
    '- static RENDER_PANIC_OCCURRED: AtomicBool = AtomicBool::new(false);',
    '- In the panic hook, set this to true',
    '- pub fn take_render_panic() -> bool (resets the flag)',
    '- Also set LifecycleStage::Crashed when a render panic occurs',
    '',
    'STEP 3: Create src/features/pages/render_error.rs — a simple fallback view:',
    '- RenderErrorPage struct with summary: String field',
    '- Render impl that shows: error title in destructive color, summary text in muted_foreground, and a Reload button',
    '- Match existing page styling (use cx.theme(), same flex patterns as other pages)',
    '- The Reload button click should emit a ReloadCurrentPage action or call cx.notify() on the parent',
    '',
    'STEP 4: Modify src/shell/root.rs AppRoot:',
    '- Add fields: render_error: bool, error_page: Option<Entity<RenderErrorPage>>',
    '- In render(), check render_error flag AND take_render_panic()',
    '- If render panicked, create or reuse error_page entity with last_panic_summary()',
    '- Show error_page view instead of active_page_view()',
    '- Add a ReloadCurrentPage action handler that clears render_error and error_page',
    '- Observe the RENDER_PANIC_OCCURRED via cx.observe_global or check in render()',
    '',
    'STEP 5: Register render_error page in src/features/pages/mod.rs.',
    '',
    'The TriggerTestPanic action (debug builds, src/app/mod.rs) should trigger this boundary.',
  ].join('\n'),
  { label: 'error-boundary', phase: 'Error Boundary' }
);

// Phase 3: Auto-updater service
phase('Auto-Updater');

const updaterResult = await agent(
  [
    'You are implementing an auto-updater service for a GPUI Rust desktop application (macOS-first).',
    '',
    'TASK: Create src/services/updater/ — a new service for checking, downloading, verifying, and applying updates.',
    '',
    'STEP 1: Read these files for patterns:',
    '- src/services/mod.rs (module registration)',
    '- src/services/connectivity/mod.rs (network check pattern)',
    '- src/services/session/mod.rs (simple service pattern)',
    '- src/shell/status_bar.rs (state surfacing)',
    '- src/state/config_store.rs (config persistence, read the full file)',
    '- src/app/mod.rs (init sequence)',
    '- Cargo.toml (available dependencies)',
    '',
    'STEP 2: Create src/services/updater/mod.rs:',
    '',
    'Define UpdateStatus enum (Idle, Checking, UpToDate, Available with version+notes,',
    'Downloading with progress, Downloaded with version+path, ReadyToInstall, Error with message).',
    '',
    'Define UpdateSnapshot struct (status, current_version, last_check, update_channel) implementing Global.',
    '',
    'Functions:',
    '- initialize(cx) — sets default global, reads config for channel',
    '- snapshot(cx) — reads global',
    '- check_for_updates(cx) — async HTTP GET to manifest URL, compare versions',
    '- download_update(cx) — async download to temp dir',
    '- apply_update(cx) — schedule swap on next launch',
    '- set_channel(channel, cx) — persist channel preference',
    '',
    'The manifest URL should be configurable (default: https://releases.example.com/manifest.json).',
    'Manifest JSON format: { version, release_notes, platforms: { "macos-aarch64": { url, signature, size } } }',
    '',
    'Use crate::services::tokio_runtime for async HTTP (reqwest already in Cargo.toml).',
    'Use connectivity::snapshot to check network before attempting checks.',
    'For macOS verification, use std::process::Command to run codesign --verify.',
    '',
    'STEP 3: Register: add pub mod updater to src/services/mod.rs.',
    'Add updater::initialize(cx) in src/app/mod.rs init (after notifications, before shutdown handlers).',
    '',
    'STEP 4: Add status bar integration in src/shell/status_bar.rs:',
    '- Import updater snapshot',
    '- Show indicator when status is not Idle/UpToDate (e.g. "Update: v1.2.0 available")',
    '- Keep it minimal — just a text label in the existing status bar flex row',
    '',
    'STEP 5: Add config fields to AppConfig in src/state/config_store.rs:',
    '- update_channel: String (default: "stable")',
    '- last_update_check: Option<String> (ISO timestamp)',
    '',
    'PATTERNS TO FOLLOW:',
    '- cx.set_global() for snapshot, cx.update_global() for mutations',
    '- tokio_runtime global for spawning async tasks (cx.spawn with background_executor)',
    '- tracing::info!/warn!/error! for logging',
    '- Handle all errors gracefully — this service must never crash the app',
    '- Follow the same snapshot/mutation/initialize pattern as other services',
  ].join('\n'),
  { label: 'auto-updater', phase: 'Auto-Updater' }
);

// Phase 4: Compile and fix
phase('Compile & Fix');

const compileResult = await agent(
  [
    'You are fixing compilation errors in a Rust GPUI application after multiple parallel implementations.',
    '',
    'TASK: Compile the project and fix any errors until it builds clean.',
    '',
    'STEP 1: Run cargo check:',
    '  cd /Users/hmziq/os/gpui-app && cargo check 2>&1 | head -150',
    '',
    'STEP 2: Read files with errors and fix them. Common issues:',
    '- Missing imports (use statements)',
    '- Type mismatches (Pixels vs f32, px() vs px_f())',
    '- Wrong function signatures or missing arguments',
    '- Missing trait implementations',
    '- Module not declared in mod.rs',
    '- Generic bounds issues',
    '',
    'STEP 3: Re-run cargo check and iterate until zero errors.',
    '',
    'IMPORTANT:',
    '- Do NOT change the architecture — only fix compilation issues',
    '- Match existing codebase patterns for imports and types',
    '- If a dependency is missing, check Cargo.toml and add it',
    '- If something is obviously incomplete, implement the minimum to compile',
    '- Run cargo check at least 3 times to catch cascading errors',
  ].join('\n'),
  { label: 'compile-fix', phase: 'Compile & Fix' }
);

return {
  virtualList: virtualListResult ? 'success' : 'failed',
  slowFrame: slowFrameResult ? 'success' : 'failed',
  spike: spikeResult ? 'success' : 'failed',
  errorBoundary: errorBoundaryResult ? 'success' : 'failed',
  updater: updaterResult ? 'success' : 'failed',
  compile: compileResult ? 'success' : 'failed',
};
