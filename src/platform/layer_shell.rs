//! Wayland layer-shell helpers.
//!
//! Small `cfg(linux)` utility that produces a [`gpui::WindowOptions`] configured
//! for a wlr-layer-shell overlay surface, plus a [`layer_shell_options`]
//! constructor with sensible defaults for a top-anchored, on-demand-keyboard
//! panel. The returned options map directly onto the `zwlr_layer_shell_v1`
//! protocol surface properties; each field's wlr-layer-shell meaning is
//! documented on [`LayerShellOptions`].
//!
//! Mirrors the layer-shell window setup used by the reference launcher
//! (`src/app/window.rs`) but rebinds to generic gpui-starter types.

#[cfg(target_os = "linux")]
use gpui::{Pixels, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, px};

/// `tracing` target for this module. Matches the gpui-starter `LOG` idiom
/// (`"gpui_starter::<module>"`).
#[cfg(target_os = "linux")]
const LOG: &str = "gpui_starter::layer_shell";

/// Sensible default namespace for the layer-shell surface. Compositors use this
/// to apply per-app window rules; callers may override it.
#[cfg(target_os = "linux")]
pub const DEFAULT_NAMESPACE: &str = "gpui-starter";

/// Build a [`LayerShellOptions`] with sensible defaults for a top-anchored
/// overlay panel:
///
/// - `layer`: [`Layer::Top`] (rendered above normal windows, below popups/OS).
/// - `anchor`: [`Anchor::TOP`] only (a bar/panel docked to the top edge).
/// - `exclusive_zone`: `Some(px(0.))` — requests the compositor reserve no
///   space, so the panel can float over other windows. (In wlr-layer-shell an
///   exclusive zone of `0` means "do not occlude"; `-1` means "ignore other
///   surfaces". gpui represents this as `Option<Pixels>`, where `None` leaves
///   it unset and `Some(0px)` reserves a zero-width/height strip.)
/// - `keyboard_interactivity`: [`KeyboardInteractivity::OnDemand`] (focusable
///   like a normal window, no exclusive grab).
///
/// Pass the result into [`overlay_window_options`], or set it as `kind` on a
/// hand-built [`WindowOptions`] via [`WindowKind::LayerShell`].
#[cfg(target_os = "linux")]
pub fn layer_shell_options(namespace: &str) -> LayerShellOptions {
    LayerShellOptions {
        namespace: namespace.to_string(),
        layer: Layer::Top,
        anchor: Anchor::TOP,
        // `0` exclusive zone = "don't occlude me, but I take no reserved space".
        exclusive_zone: Some(px(0.)),
        exclusive_edge: None,
        margin: None,
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
    }
}

/// Build a [`gpui::WindowOptions`] suitable for a Wayland layer-shell overlay.
///
/// `display_size` is the pixel size of the surface (width, height). The window
/// is placed at the origin with a transparent background and no titlebar, so
/// the compositor positions/anchors it purely via the wlr-layer-shell rules in
/// [`layer_shell_options`].
///
/// Returns `Err` (logged + degraded) only if callers want a Result variant; this
/// constructor itself is infallible since every field has a default.
#[cfg(target_os = "linux")]
pub fn overlay_window_options(namespace: &str, display_size: (f32, f32)) -> WindowOptions {
    let options = layer_shell_options(namespace);
    tracing::debug!(
        target: LOG,
        namespace,
        layer = ?options.layer,
        anchor = ?options.anchor,
        keyboard = ?options.keyboard_interactivity,
        "building layer-shell overlay window options"
    );

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui::Bounds {
            origin: gpui::point(px(0.), px(0.)),
            size: gpui::size(px(display_size.0), px(display_size.1)),
        })),
        titlebar: None,
        focus: true,
        show: true,
        app_id: Some(namespace.to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(options),
        ..Default::default()
    }
}

// Re-exports so callers don't need to reach into `gpui::layer_shell` directly
// and so this module is the single seam if the gpui API name ever moves.
#[cfg(target_os = "linux")]
pub use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};

#[cfg(not(target_os = "linux"))]
compile_error!(
    "src/platform/layer_shell.rs is Linux-only; gate any `mod layer_shell;` \
     declaration with #[cfg(target_os = \"linux\")]"
);

// Silence unused-import / unused-Pixels warnings when only part of the API is
// referenced downstream. `Pixels` is part of the public type surface (via
// `LayerShellOptions::exclusive_zone`) and `px` is used above.
#[cfg(target_os = "linux")]
const _: () = {
    let _ = LOG; // referenced in tracing! calls above
    let _: Option<Pixels> = None;
};
