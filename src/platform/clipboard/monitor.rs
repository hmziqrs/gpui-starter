//! Wayland clipboard change monitor (feature-gated).
//!
//! Uses the `wlr-data-control-unstable-v1` protocol (via `wayland-client`
//! and `wayland-protocols-wlr`) to observe clipboard selections on wlroots
//! compositors. On each selection change the current content is read back
//! through [`arboard::Clipboard`] and pushed into a [`ClipboardHistory`].
//!
//! This is a deliberately conservative skeleton: it connects, binds the
//! `zwlr_data_control_manager_v1` global and a `wl_seat`, creates a data
//! device, and then runs a blocking dispatch loop. When the compositor does
//! not advertise the protocol (non-wlroots / X11 / macOS host) the monitor
//! starts, logs the absence, and stops cleanly instead of panicking — the
//! "log + degrade" boilerplate convention.
//!
//! Gated behind BOTH `target_os = "linux"` and the `clipboard-history`
//! cargo feature, so the default (no-feature) build pulls in neither
//! wayland crate.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1, zwlr_data_control_manager_v1, zwlr_data_control_offer_v1,
    zwlr_data_control_source_v1,
};

use super::data::ClipboardHistory;
use super::item::ClipboardContent;

const LOG: &str = "gpui_starter::clipboard::monitor";

/// Handle returned by [`start_monitor`]. Setting the flag to `false`
/// (via [`stop`]) asks the background thread to exit its dispatch loop.
#[derive(Clone)]
pub struct MonitorHandle {
    running: Arc<AtomicBool>,
}

impl MonitorHandle {
    /// Request the monitor thread to stop.
    ///
    /// This only flips the flag; the actual exit happens on the next
    /// non-blocking dispatch wakeup. The blocking loop in `run_monitor`
    /// observes the flag between iterations.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Whether the monitor believes it should keep running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// Start monitoring the clipboard in a background thread.
///
/// Observed content is pushed into `history`. The returned [`MonitorHandle`]
/// lets the caller request shutdown. Failures to connect or to find the
/// required protocol are logged from inside the thread and degrade
/// gracefully (the thread simply exits) — they never propagate as panics.
pub fn start_monitor(history: Arc<ClipboardHistory>) -> MonitorHandle {
    let running = Arc::new(AtomicBool::new(true));
    let handle = MonitorHandle {
        running: running.clone(),
    };

    thread::Builder::new()
        .name("gpui-clipboard-monitor".into())
        .spawn(move || {
            tracing::info!(target: LOG, "starting clipboard monitor");
            if let Err(err) = run_monitor(history, running.clone()) {
                tracing::warn!(
                    target: LOG,
                    error = %err,
                    "clipboard monitor exited with error"
                );
            }
        })
        .ok();

    handle
}

fn run_monitor(
    history: Arc<ClipboardHistory>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let display = conn.display();

    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let _registry = display.get_registry(&qh, ());

    let mut state = ClipboardMonitorState {
        manager: None,
        seat: None,
        device: None,
        history,
        running,
    };

    // Initial roundtrip to discover globals.
    event_queue.roundtrip(&mut state)?;

    if state.manager.is_none() {
        tracing::info!(
            target: LOG,
            "wlr-data-control protocol not advertised; clipboard monitor is a no-op on this compositor"
        );
        return Ok(());
    }
    if state.seat.is_none() {
        tracing::info!(target: LOG, "no wayland seat available; clipboard monitor is a no-op");
        return Ok(());
    }

    if let (Some(manager), Some(seat)) = (state.manager.as_ref(), state.seat.as_ref()) {
        let device = manager.get_data_device(seat, &qh, ());
        state.device = Some(device);
        tracing::debug!(target: LOG, "created wlr data-control device");
    }

    event_queue.roundtrip(&mut state)?;
    tracing::info!(target: LOG, "clipboard monitor initialized");

    while state.running.load(Ordering::Relaxed) {
        // A blocking dispatch keeps the thread idle until the compositor
        // delivers an event. Errors here are transient (e.g. compositor
        // restart) and are logged + looped rather than fatal.
        if let Err(err) = event_queue.blocking_dispatch(&mut state) {
            tracing::warn!(target: LOG, error = %err, "dispatch error; retrying");
            thread::sleep(Duration::from_millis(250));
        }
    }

    tracing::info!(target: LOG, "clipboard monitor stopped");
    Ok(())
}

/// Per-connection state for the wayland dispatch loop.
struct ClipboardMonitorState {
    manager: Option<zwlr_data_control_manager_v1::ZwlrDataControlManagerV1>,
    seat: Option<wl_seat::WlSeat>,
    device: Option<zwlr_data_control_device_v1::ZwlrDataControlDeviceV1>,
    history: Arc<ClipboardHistory>,
    running: Arc<AtomicBool>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for ClipboardMonitorState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };

        if interface == "zwlr_data_control_manager_v1" && state.manager.is_none() {
            let manager = registry
                .bind::<zwlr_data_control_manager_v1::ZwlrDataControlManagerV1, _, _>(
                    name,
                    version.min(2),
                    qh,
                    (),
                );
            state.manager = Some(manager);
            tracing::debug!(target: LOG, "bound wlr-data-control-manager");
        } else if interface == "wl_seat" && state.seat.is_none() {
            let seat = registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(1), qh, ());
            state.seat = Some(seat);
            tracing::debug!(target: LOG, "bound wl_seat");
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for ClipboardMonitorState {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_data_control_manager_v1::ZwlrDataControlManagerV1, ()>
    for ClipboardMonitorState
{
    fn event(
        _: &mut Self,
        _: &zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
        _: zwlr_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_data_control_device_v1::ZwlrDataControlDeviceV1, ()> for ClipboardMonitorState {
    fn event(
        state: &mut Self,
        _: &zwlr_data_control_device_v1::ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_device_v1::Event::Selection { id } = event
            && id.is_some()
        {
            tracing::debug!(target: LOG, "clipboard selection changed");
            if let Err(err) = read_and_record(&state.history) {
                tracing::warn!(target: LOG, error = %err, "failed to read clipboard after selection change");
            }
        }
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<Self>,
    ) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
        // The data-control device only ever spawns data-offer children; any
        // other opcode is unexpected. We bind the real offer proxy for the
        // known opcode and a no-op ObjectData for the (unreachable) default.
        match opcode {
            zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => {
                qhandle.make_data::<zwlr_data_control_offer_v1::ZwlrDataControlOfferV1, _>(())
            }
            _ => std::sync::Arc::new(NullObjectData),
        }
    }
}

/// No-op [`ObjectData`] used as a defensive fallback for unexpected child
/// object opcodes. Never exercises its methods in practice.
struct NullObjectData;

impl wayland_client::backend::ObjectData for NullObjectData {
    fn event(
        self: std::sync::Arc<Self>,
        _backend: &wayland_client::backend::Backend,
        _msg: wayland_client::backend::protocol::Message<
            wayland_client::backend::ObjectId,
            std::os::fd::OwnedFd,
        >,
    ) -> Option<std::sync::Arc<dyn wayland_client::backend::ObjectData>> {
        None
    }

    fn destroyed(&self, _object_id: wayland_client::backend::ObjectId) {}
}

impl Dispatch<zwlr_data_control_offer_v1::ZwlrDataControlOfferV1, ()> for ClipboardMonitorState {
    fn event(
        _: &mut Self,
        _: &zwlr_data_control_offer_v1::ZwlrDataControlOfferV1,
        _: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_data_control_source_v1::ZwlrDataControlSourceV1, ()> for ClipboardMonitorState {
    fn event(
        _: &mut Self,
        _: &zwlr_data_control_source_v1::ZwlrDataControlSourceV1,
        _: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Read the current clipboard selection and append it to the history.
///
/// Images are preferred over text when both are present (browsers commonly
/// publish both an image and an HTML representation). A short settle delay
/// avoids racing the compositor's own clipboard write.
fn read_and_record(history: &ClipboardHistory) -> Result<(), Box<dyn std::error::Error>> {
    thread::sleep(Duration::from_millis(50));

    let mut clipboard = Clipboard::new()?;

    if let Ok(image) = clipboard.get_image()
        && !image.bytes.is_empty()
    {
        tracing::debug!(
            target: LOG,
            width = image.width,
            height = image.height,
            bytes = image.bytes.len(),
            "recording clipboard image"
        );
        history.push(ClipboardContent::Image {
            width: image.width,
            height: image.height,
            rgba_bytes: image.bytes.to_vec(),
        });
        return Ok(());
    }

    if let Ok(text) = clipboard.get_text()
        && !text.is_empty()
    {
        tracing::debug!(target: LOG, len = text.len(), "recording clipboard text");
        history.push(ClipboardContent::Text(text));
        return Ok(());
    }

    Ok(())
}
