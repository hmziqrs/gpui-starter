use global_hotkey::GlobalHotKeyEvent;
use gpui::App;
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

const LOG: &str = "gpui_starter::tray";

// ---------------------------------------------------------------------------
// Tray icon pixel data — 36×36 RGBA magnifying-glass template image
// ---------------------------------------------------------------------------

fn build_icon() -> tray_icon::Icon {
    const SIZE: usize = 36;
    let mut px = vec![0u8; SIZE * SIZE * 4];

    let cx = SIZE as f32 * 0.42;
    let cy = SIZE as f32 * 0.42;
    let r_outer = SIZE as f32 * 0.30;
    let r_inner = r_outer - 3.2;

    let hx0 = cx + r_outer * 0.65;
    let hy0 = cy + r_outer * 0.65;
    let hx1 = SIZE as f32 * 0.86;
    let hy1 = SIZE as f32 * 0.86;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;

            // Lens ring
            let dx = fx - cx;
            let dy = fy - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let lens_a: f32 = if dist >= r_inner && dist <= r_outer {
                1.0
            } else if dist < r_inner {
                ((dist - (r_inner - 1.0)) / 1.0).clamp(0.0, 1.0)
            } else {
                (1.0 - (dist - r_outer) / 1.0).clamp(0.0, 1.0)
            };

            // Handle segment
            let ex = hx1 - hx0;
            let ey = hy1 - hy0;
            let len2 = ex * ex + ey * ey;
            let t = ((fx - hx0) * ex + (fy - hy0) * ey) / len2;
            let t = t.clamp(0.0, 1.0);
            let px2 = hx0 + t * ex;
            let py2 = hy0 + t * ey;
            let d_h = ((fx - px2) * (fx - px2) + (fy - py2) * (fy - py2)).sqrt();
            let handle_a: f32 = if d_h <= 1.8 {
                1.0
            } else {
                (1.0 - (d_h - 1.8) / 1.0).clamp(0.0, 1.0)
            };

            let a = (lens_a.max(handle_a) * 255.0) as u8;
            let i = (y * SIZE + x) * 4;
            px[i] = 0;
            px[i + 1] = 0;
            px[i + 2] = 0;
            px[i + 3] = a;
        }
    }

    // SAFETY: Pixel data is generated from a compile-time constant RGBA buffer.
    // The dimensions match the buffer length by construction.
    tray_icon::Icon::from_rgba(px, SIZE as u32, SIZE as u32)
        .expect("tray icon pixel data is valid: compile-time constant")
}

// ---------------------------------------------------------------------------
// Public entry point — call once from main() on macOS
// ---------------------------------------------------------------------------

pub fn setup(cx: &mut App) {
    tracing::info!(target: LOG, "Setting up tray icon");

    let icon = build_icon();
    let Ok(tray) = TrayIconBuilder::new()
        .with_icon(icon)
        .with_icon_as_template(true)
        .with_tooltip("Open Launcher  (⌥Space)")
        .with_menu_on_left_click(false)
        .build()
    else {
        tracing::error!("failed to create system tray icon");
        return;
    };
    Box::leak(Box::new(tray));
    tracing::debug!(target: LOG, "Tray icon created");

    // Tray-icon clicks and the global hotkey arrive on two crossbeam channels
    // exposed by `tray_icon` / `global_hotkey`. Each `receiver()` supports a
    // blocking `recv()`, so instead of waking the GPUI executor 20×/s to poll,
    // a dedicated OS thread parks on each receiver and forwards a unit tick
    // over a flume channel. A foreground GPUI task drains that channel
    // reactively (`recv_async`) and opens the launcher on the UI thread.
    //
    // `AsyncApp` is `!Send` (it holds an `Rc`-backed app reference), so the
    // OS threads cannot call `cx.update` directly; the flume channel is what
    // gets the event back onto the foreground executor.
    let (tx, rx) = flume::unbounded::<()>();
    let tx_hotkey = tx.clone();

    if let Err(e) = std::thread::Builder::new()
        .name("gpui-tray-events".into())
        .spawn(move || {
            let tray_rx = TrayIconEvent::receiver();
            loop {
                let Ok(event) = tray_rx.recv() else {
                    return;
                };
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    tracing::info!(target: LOG, source = "tray_click", "Launcher trigger");
                    let _ = tx.send(());
                }
            }
        })
    {
        tracing::error!(
            target: LOG,
            error = %e,
            "failed to spawn gpui-tray-events thread; tray clicks will not be delivered"
        );
    }

    if let Err(e) = std::thread::Builder::new()
        .name("gpui-hotkey-events".into())
        .spawn(move || {
            let hotkey_rx = GlobalHotKeyEvent::receiver();
            loop {
                if hotkey_rx.recv().is_err() {
                    return;
                }
                tracing::info!(target: LOG, source = "hotkey_alt_space", "Launcher trigger");
                let _ = tx_hotkey.send(());
            }
        })
    {
        tracing::error!(
            target: LOG,
            error = %e,
            "failed to spawn gpui-hotkey-events thread; global hotkey will not be delivered"
        );
    }

    cx.spawn(async move |cx| {
        loop {
            // Parked until a tray click or hotkey fires — no periodic wake-ups.
            if rx.recv_async().await.is_err() {
                break;
            }
            let _ = cx.update(crate::launcher::open_launcher);
        }
    })
    .detach();
}
