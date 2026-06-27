//! Command palette (a.k.a. "launcher") — thin adapter over
//! [`crate::features::palette`].
//!
//! Historically a ~466-line monolith, this file is now a thin re-export /
//! adapter that preserves the existing public surface (`init`,
//! [`LauncherOpen`], [`LauncherActionKind`], [`LauncherItem`],
//! [`LauncherEvent`], [`Launcher`], [`LauncherRoot`], [`open_launcher`]) while
//! delegating its item model and fuzzy scoring to the reusable
//! [`crate::features::palette`] module.
//!
//! Behaviour is preserved: the `Launcher` view still renders its own search bar
//! and result rows (so the visual layout is byte-for-byte identical), and
//! [`LauncherRoot`] still installs `LiquidGlass` on macOS. The only functional
//! upgrade is that filtering now uses `SkimMatcherV2` fuzzy scoring (a strict
//! superset of the old `contains` filter) when the `fuzzy-matcher` crate is
//! available; it gracefully falls back to substring matching otherwise.
//!
//! The new generic pieces live in [`crate::features::palette`]:
//! [`palette::BaseDelegate`], [`palette::PaletteDelegate`],
//! [`palette::ItemFilter`], [`palette::SectionManager`], [`palette::PaletteEntry`].

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, FocusTrapElement as _, Icon, IconName, Root, Sizable as _, h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::commands::{self, CommandId};
use crate::features::palette::{BaseDelegate, FuzzyMatchConfig, ItemFilter, KindStr, PaletteEntry};

const LOG: &str = "gpui_starter::launcher";
const CONTEXT: &str = "Launcher";

actions!(launcher, [SelectNext, SelectPrev, Dismiss]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("up", SelectPrev, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
    ]);
}

// Prevents double-opening the launcher
pub struct LauncherOpen(pub bool);
impl Global for LauncherOpen {}

// ---------------------------------------------------------------------------
// Item model  — now an adapter implementing the generic PaletteEntry contract.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub enum LauncherActionKind {
    Execute(CommandId),
}

#[derive(Clone)]
pub struct LauncherItem {
    pub title: SharedString,
    pub subtitle: SharedString,
    pub icon: IconName,
    pub action: LauncherActionKind,
}

impl PaletteEntry for LauncherItem {
    type Kind = KindStr;

    fn kind(&self) -> Self::Kind {
        // Every launcher command belongs to the same group.
        KindStr("Commands")
    }
    fn name(&self) -> &str {
        &self.title
    }
    fn description(&self) -> Option<&str> {
        Some(&self.subtitle)
    }
    fn icon(&self) -> Option<IconName> {
        Some(self.icon.clone())
    }
    fn action_hint(&self) -> Option<&str> {
        Some("Enter")
    }
    fn debug_id(&self) -> Option<SharedString> {
        match self.action {
            LauncherActionKind::Execute(id) => Some(format!("{id:?}").into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Launcher view  (pure search UI – emits LauncherEvent)
//
// Rendering is intentionally unchanged from the original monolith. Selection
// state is now held by a `BaseDelegate<LauncherItem>`, and filtering uses the
// generic `ItemFilter` (SkimMatcherV2) — a superset of the old substring match.
// ---------------------------------------------------------------------------

pub enum LauncherEvent {
    Act(LauncherActionKind),
    Dismiss,
}

pub struct Launcher {
    focus_handle: FocusHandle,
    pub input: Entity<InputState>,
    /// Generic selection / query state helper (from the palette module).
    state: BaseDelegate<LauncherItem>,
    /// Fuzzy scorer (from the palette module). Built once.
    filter: ItemFilter,
}

impl EventEmitter<LauncherEvent> for Launcher {}

impl Focusable for Launcher {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Launcher {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search pages and commands…"));

        cx.subscribe(&input, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::Change => this.refilter(cx),
            InputEvent::PressEnter { .. } => this.act(cx),
            _ => {}
        })
        .detach();

        let state = BaseDelegate::new(Self::make_items());
        let filter = ItemFilter::new(FuzzyMatchConfig::default());

        Self {
            focus_handle: cx.focus_handle(),
            input,
            state,
            filter,
        }
    }

    fn make_items() -> Vec<LauncherItem> {
        commands::registry()
            .into_iter()
            .map(|command| LauncherItem {
                title: command.title,
                subtitle: command.subtitle,
                icon: command.icon,
                action: LauncherActionKind::Execute(command.id),
            })
            .collect()
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let q = self.input.read(cx).value().to_lowercase();
        // The generic ItemFilter scores name (preferred) then description
        // (penalised). This is a strict superset of the old `contains` match.
        let indices = self.filter.filter_indices(self.state.items(), &q);
        self.state.apply_filtered_indices(indices);
        tracing::debug!(
            target: LOG,
            query = %q,
            results = self.state.filtered_count(),
            "Launcher filtered"
        );
        cx.notify();
    }

    fn filtered_index(&self, display_ix: usize) -> Option<usize> {
        self.state.filtered_indices().get(display_ix).copied()
    }

    fn selected_display(&self) -> usize {
        self.state.selected_index().unwrap_or(0)
    }

    fn set_selected_display(&mut self, display_ix: usize) {
        self.state.set_selected_unchecked(display_ix);
    }

    fn item(&self, display_ix: usize) -> Option<&LauncherItem> {
        self.filtered_index(display_ix)
            .and_then(|i| self.state.items().get(i))
    }

    fn act(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = self.item(self.selected_display()) {
            let action = item.action; // LauncherActionKind: Copy
            tracing::info!(
                target: LOG,
                action = ?action,
                item = %item.title,
                "Launcher action triggered"
            );
            cx.emit(LauncherEvent::Act(action));
        } else {
            tracing::debug!(target: LOG, "Launcher dismissed with no selection");
        }
        cx.emit(LauncherEvent::Dismiss);
    }
}

impl Render for Launcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let selected = self.selected_display();
        let filtered_count = self.state.filtered_count();
        let has_results = filtered_count > 0;

        // Snapshot the visible rows up front so the render closure holds only
        // owned data and can never fail to resolve an item. Degrade gracefully
        // by skipping any index that (defensively) does not map to an item.
        let rows: Vec<(usize, IconName, SharedString, SharedString)> = (0..filtered_count)
            .filter_map(|di| {
                let item = self.item(di)?;
                Some((
                    di,
                    item.icon.clone(),
                    item.title.clone(),
                    item.subtitle.clone(),
                ))
            })
            .collect();

        v_flex()
            .size_full()
            .bg(theme.background.opacity(0.0))
            .border_1()
            .border_color(theme.border.opacity(0.5))
            .rounded(theme.radius_lg)
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .focus_trap("launcher", &self.focus_handle)
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                this.state.select_down();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| {
                this.state.select_up();
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| {
                cx.emit(LauncherEvent::Dismiss);
            }))
            // ── Search bar ──────────────────────────────────────────────────
            .child(
                h_flex()
                    .px_4()
                    .py(px(12.))
                    .gap_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .items_center()
                    .child(
                        Icon::new(IconName::Search)
                            .size_5()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        Input::new(&self.input)
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .flex_1(),
                    ),
            )
            // ── Results ─────────────────────────────────────────────────────
            .child(
                v_flex()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .py_1()
                    .children(rows.iter().enumerate().map(|(display_ix, row)| {
                        let is_selected = display_ix == selected;
                        let icon = row.1.clone();
                        let title = row.2.clone();
                        let subtitle = row.3.clone();

                        h_flex()
                            .id(display_ix)
                            .px_3()
                            .py_2()
                            .mx_1()
                            .gap_3()
                            .items_center()
                            .rounded(theme.radius)
                            .cursor_pointer()
                            .when(is_selected, |el| el.bg(theme.list_active))
                            .when(!is_selected, |el| el.hover(|el| el.bg(theme.list_hover)))
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.selected_display() != display_ix {
                                    this.set_selected_display(display_ix);
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_selected_display(display_ix);
                                this.act(cx);
                            }))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .size_8()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(theme.radius)
                                    .bg(theme.secondary)
                                    .child(Icon::new(icon).small()),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .truncate()
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .truncate()
                                            .child(subtitle),
                                    ),
                            )
                    }))
                    .when(!has_results, |el| {
                        el.child(
                            div()
                                .px_4()
                                .py_8()
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child("No results"),
                        )
                    }),
            )
            // ── Footer hint ─────────────────────────────────────────────────
            .child(
                h_flex()
                    .px_4()
                    .py(px(8.))
                    .gap_4()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("↑↓  navigate")
                    .child("↵  open")
                    .child("esc  close"),
            )
    }
}

// ---------------------------------------------------------------------------
// LauncherRoot  (standalone window root – handles events, closes window)
// ---------------------------------------------------------------------------

pub struct LauncherRoot {
    launcher: Entity<Launcher>,
    should_close: bool,
}

impl Focusable for LauncherRoot {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.launcher.focus_handle(cx)
    }
}

impl LauncherRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Install Liquid Glass — creates NSGlassEffectView directly in the
        // native view hierarchy, no GPUI source patches needed.
        #[cfg(target_os = "macos")]
        crate::platform::liquid_glass::LiquidGlass::install(window, &Default::default());

        let launcher = cx.new(|cx| Launcher::new(window, cx));

        // Focus the search input after the first layout pass
        let input_fh = launcher.read(cx).input.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| {
            input_fh.focus(window, cx);
        });

        cx.subscribe(&launcher, |this, _, ev: &LauncherEvent, cx| {
            match ev {
                LauncherEvent::Act(action) => {
                    tracing::info!(target: LOG, action = ?action, "LauncherRoot handling action");
                    match action {
                        LauncherActionKind::Execute(command_id) => {
                            tracing::info!(
                                target: LOG,
                                command = ?command_id,
                                "Executing command"
                            );
                            commands::execute(*command_id, cx);
                        }
                    }
                }
                LauncherEvent::Dismiss => {
                    tracing::debug!(target: LOG, "LauncherRoot received Dismiss event");
                }
            }
            tracing::debug!(target: LOG, "Scheduling launcher window close");
            this.should_close = true;
            cx.notify();
        })
        .detach();

        // Close the launcher when the window loses OS-level activation
        // (e.g. user clicks outside the popup window).
        // Call remove_window directly to avoid a 1-frame delay — macOS
        // changes the blur treatment on deactivation, which would flash.
        cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                tracing::debug!(target: LOG, "Launcher window deactivated — closing");
                cx.set_global(LauncherOpen(false));
                window.remove_window();
            }
        })
        .detach();

        Self {
            launcher,
            should_close: false,
        }
    }
}

impl Render for LauncherRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.should_close {
            self.should_close = false;
            cx.set_global(LauncherOpen(false));
            tracing::info!(target: LOG, "Removing launcher window (deferred)");
            window.defer(cx, |window, _cx| {
                window.remove_window();
            });
        }

        let dialog_layer = Root::render_dialog_layer(window, cx);

        div()
            .size_full()
            .child(self.launcher.clone())
            .children(dialog_layer)
    }
}

// ---------------------------------------------------------------------------
// Open the launcher as a floating PopUp window
// ---------------------------------------------------------------------------

pub fn open_launcher(cx: &mut App) {
    if cx.try_global::<LauncherOpen>().is_some_and(|g| g.0) {
        tracing::debug!(target: LOG, "Launcher already open — ignoring open request");
        return;
    }
    tracing::info!(target: LOG, "Opening launcher window");
    cx.set_global(LauncherOpen(true));

    let window_w = px(620.);
    let window_h = px(460.);

    let bounds = if let Some(display) = cx.primary_display() {
        let display_bounds = display.bounds();
        let x = display_bounds.origin.x + (display_bounds.size.width - window_w) / 2.;
        let y = display_bounds.origin.y + display_bounds.size.height * 0.12;
        Bounds {
            origin: point(x, y),
            size: size(window_w, window_h),
        }
    } else {
        Bounds {
            origin: point(px(200.), px(120.)),
            size: size(window_w, window_h),
        }
    };

    cx.spawn(async move |cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: true,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: true,
            is_resizable: false,
            window_background: WindowBackgroundAppearance::Blurred,
            window_min_size: Some(gpui::Size {
                width: window_w,
                height: window_h,
            }),
            ..Default::default()
        };

        let Some(window) = cx
            .open_window(options, |window, cx| {
                let launcher_root = cx.new(|cx| LauncherRoot::new(window, cx));
                cx.new(|cx| Root::new(launcher_root, window, cx).bg(transparent_black()))
            })
            .ok()
        else {
            tracing::error!("failed to open launcher window");
            return Ok::<_, anyhow::Error>(());
        };

        window
            .update(cx, |_, window, _| {
                window.activate_window();
            })
            .ok();

        Ok::<_, anyhow::Error>(())
    })
    .detach();
}
