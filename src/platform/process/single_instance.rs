use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use gpui::{App, Global};
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Stream, prelude::*,
};
use single_instance::SingleInstance;
use std::time::Duration;

use crate::events::{self, AppEventKind};
use crate::ipc::{
    ForwardedRequest, ForwardedResponse,
    rpc::{decode_request, encode_line},
};

const INSTANCE_NAME: &str = "com.gpui-starter.app.instance";
const LOG: &str = "gpui_starter::single_instance";
const SCHEME: &str = "gpui-starter://";

pub struct SingleInstanceRuntime {
    _instance: SingleInstance,
    ipc_name: String,
    queue_file: PathBuf,
    /// Filesystem path of the forwarder socket, if a namespaced socket is
    /// unavailable. Stored so a [`Drop`] impl can remove a stale socket.
    socket_path: Option<PathBuf>,
    ipc_running: Arc<AtomicBool>,
}

impl Drop for SingleInstanceRuntime {
    fn drop(&mut self) {
        // Signal the blocking listener thread to exit, then remove the
        // stale socket file (RAII). A leftover socket causes the next
        // `Stream::connect` to succeed against a dead listener or, for
        // filesystem sockets, refuses the connection entirely — so this
        // probe+cleanup is essential for a clean restart (especially the
        // exec-reload path, which re-launches immediately).
        self.ipc_running.store(false, Ordering::SeqCst);
        let _ = send_forwarded_link_via_ipc(&self.ipc_name, "__shutdown__");
        if let Some(path) = &self.socket_path
            && path.exists()
        {
            let _ = fs::remove_file(path);
            tracing::debug!(target: LOG, path = %path.display(), "removed stale ipc socket on drop");
        }
    }
}

impl Global for SingleInstanceRuntime {}

pub struct Preflight {
    pub should_start: bool,
    pub runtime: Option<SingleInstanceRuntime>,
    pub initial_deep_link: Option<String>,
}

pub fn preflight() -> Preflight {
    let args: Vec<String> = std::env::args().collect();
    let deep_link = args.iter().find(|arg| arg.starts_with(SCHEME)).cloned();
    let ipc_name = ipc_name();
    let queue_file = queue_file_path();

    let instance = match SingleInstance::new(INSTANCE_NAME) {
        Ok(instance) => instance,
        Err(err) => {
            eprintln!("single-instance init failed: {err}");
            return Preflight {
                should_start: true,
                runtime: None,
                initial_deep_link: deep_link,
            };
        }
    };

    if instance.is_single() {
        if let Some(parent) = queue_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::remove_file(&queue_file);
        // Best-effort stale-socket probe: if a filesystem socket from a
        // crashed previous run is still present, probe its liveness and
        // remove it when no listener answers. A namespaced socket has no
        // filesystem path, so this is a no-op there.
        let socket_path = filesystem_socket_path(&ipc_name);
        if let Some(path) = &socket_path
            && path.exists()
            && !is_socket_live(&ipc_name)
        {
            let _ = fs::remove_file(path);
            tracing::warn!(
                target: LOG,
                path = %path.display(),
                "removed stale ipc socket before startup"
            );
        }
        Preflight {
            should_start: true,
            runtime: Some(SingleInstanceRuntime {
                _instance: instance,
                ipc_name,
                queue_file,
                socket_path,
                ipc_running: Arc::new(AtomicBool::new(true)),
            }),
            initial_deep_link: deep_link,
        }
    } else {
        if let Some(link) = deep_link
            && let Err(err) = send_forwarded_link_via_ipc(&ipc_name, &link)
        {
            tracing::warn!(
                target: LOG,
                error = %err,
                "ipc forward failed; falling back to queue file"
            );
            append_forwarded_link(&queue_file, &link);
        }
        Preflight {
            should_start: false,
            runtime: None,
            initial_deep_link: None,
        }
    }
}

pub fn install(runtime: SingleInstanceRuntime, cx: &mut App) {
    crate::capabilities::set(
        "single_instance",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    crate::capabilities::set(
        "second_instance_forwarding",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
    let queue_file = runtime.queue_file.clone();
    let ipc_name = runtime.ipc_name.clone();
    let ipc_running = runtime.ipc_running.clone();
    cx.set_global(runtime);
    let ipc_ok = start_ipc_forwarder(ipc_name, ipc_running, cx);
    if !ipc_ok {
        crate::capabilities::set(
            "second_instance_forwarding",
            crate::capabilities::CapabilityStatus {
                supported: true,
                enabled: true,
                degraded: true,
                reason: Some("ipc forwarding unavailable; using queue-file fallback".into()),
                last_error: Some("failed to initialize local-socket listener".into()),
            },
            cx,
        );
    }
    start_forwarded_link_poller(queue_file, cx);
}

pub fn shutdown(cx: &mut App) {
    if let Some(runtime) = cx.try_global::<SingleInstanceRuntime>() {
        runtime.ipc_running.store(false, Ordering::SeqCst);
        // Nudge the blocking listener accept loop so it can observe `ipc_running = false`.
        let _ = send_forwarded_link_via_ipc(&runtime.ipc_name, "__shutdown__");
    }
}

fn start_forwarded_link_poller(queue_file: PathBuf, cx: &mut App) {
    tracing::info!(target: LOG, queue = %queue_file.display(), "starting deep-link forwarder poller");
    let bg = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        loop {
            bg.timer(Duration::from_millis(450)).await;
            let links = drain_forwarded_links(&queue_file);
            if links.is_empty() {
                continue;
            }
            cx.update(move |cx| {
                for link in links {
                    tracing::info!(target: LOG, link, "received forwarded deep-link payload");
                    events::emit(AppEventKind::DeepLinkReceived(link), cx);
                }
            });
        }
    })
    .detach();
}

fn start_ipc_forwarder(ipc_name: String, ipc_running: Arc<AtomicBool>, cx: &mut App) -> bool {
    // flume unbounded channel: the blocking listener thread pushes raw
    // received lines; the gpui task drains them reactively via recv_async,
    // replacing the legacy 180ms polling loop.
    let (tx, rx) = flume::unbounded::<ForwardedPayload>();
    let ipc_name_for_thread = ipc_name.clone();
    let thread = std::thread::Builder::new()
        .name("gpui-ipc-forwarder".to_string())
        .spawn(move || {
            let name = match resolve_ipc_name(&ipc_name_for_thread) {
                Ok(name) => name,
                Err(err) => {
                    tracing::error!(
                        target: LOG,
                        error = %err,
                        "failed to resolve ipc listener name"
                    );
                    return;
                }
            };

            let listener = match ListenerOptions::new().name(name).create_sync() {
                Ok(listener) => listener,
                Err(err) => {
                    tracing::error!(target: LOG, error = %err, "failed to create ipc listener");
                    return;
                }
            };

            tracing::info!(target: LOG, ipc = %ipc_name_for_thread, "starting ipc deep-link listener");

            for conn in listener.incoming() {
                if !ipc_running.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut conn) = conn else {
                    continue;
                };
                let line = {
                    let mut reader = BufReader::new(&conn);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        String::new()
                    } else {
                        line
                    }
                };
                let trimmed = line.trim().to_string();
                if trimmed.is_empty() || trimmed == "__shutdown__" {
                    continue;
                }
                // Typed protocol: decode a ForwardedRequest, emit the
                // command to the gpui task, and write a typed
                // ForwardedResponse back on the same connection so the
                // caller gets synchronous ok/err.
                if let Some(req) = decode_request(&trimmed) {
                    let id = req.id;
                    let _ = tx.send(ForwardedPayload::Request(req));
                    // Best-effort response: dispatch never fails to
                    // receive (the gpui task owns the rx), so report ok.
                    let resp = ForwardedResponse::ok(id);
                    if let Ok(encoded) = encode_line(&resp) {
                        let _ = conn.write_all(encoded.as_bytes());
                    }
                } else if trimmed.starts_with(SCHEME) {
                    // Legacy raw deep-link line — backward compatibility
                    // with older second instances that did not use the
                    // typed layer.
                    let _ = tx.send(ForwardedPayload::DeepLink(trimmed));
                } else {
                    tracing::debug!(
                        target: LOG,
                        line = %trimmed,
                        "ignoring unrecognized ipc line"
                    );
                }
            }
        });

    if thread.is_err() {
        return false;
    }

    // Reactive drain: recv_async replaces the 180ms timer poll.
    cx.spawn(async move |cx| {
        loop {
            let payload = match rx.recv_async().await {
                Ok(payload) => payload,
                // Sender half dropped (runtime shutting down): exit cleanly.
                Err(_) => break,
            };
            cx.update(move |cx| match payload {
                ForwardedPayload::Request(req) => {
                    tracing::info!(
                        target: LOG,
                        id = req.id,
                        command = req.command.label(),
                        "received forwarded ipc command"
                    );
                    events::emit(AppEventKind::RemoteCommand(req.command), cx);
                }
                ForwardedPayload::DeepLink(link) => {
                    tracing::info!(target: LOG, link, "received forwarded deep-link payload via ipc");
                    events::emit(AppEventKind::DeepLinkReceived(link), cx);
                }
            });
        }
    })
    .detach();

    true
}

/// Payload pushed from the blocking forwarder thread to the gpui task.
enum ForwardedPayload {
    /// Typed request decoded from the typed IPC protocol.
    Request(ForwardedRequest),
    /// Legacy raw deep-link line (starts with the app scheme).
    DeepLink(String),
}

fn append_forwarded_link(path: &PathBuf, link: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    match fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => {
            let _ = writeln!(file, "{link}");
        }
        Err(err) => {
            eprintln!("failed forwarding deep-link to primary instance: {err}");
        }
    }
}

/// Send a single deep-link to the primary instance over a local socket.
/// Synchronous counterpart of `crate::ipc::IpcEndpoint::send`.
fn send_forwarded_link_via_ipc(ipc_name: &str, link: &str) -> Result<(), std::io::Error> {
    let name = resolve_ipc_name(ipc_name)?;
    let mut stream = Stream::connect(name)?;
    writeln!(stream, "{link}")
}

fn drain_forwarded_links(path: &PathBuf) -> Vec<String> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    if content.trim().is_empty() {
        return Vec::new();
    }
    let _ = fs::write(path, "");
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn queue_file_path() -> PathBuf {
    if let Some(project_dirs) = crate::platform::filesystem::paths::project_dirs() {
        let dir = project_dirs.cache_dir().join("runtime");
        return dir.join("forwarded-deep-links.queue");
    }
    std::env::temp_dir().join("gpui-starter-forwarded-deep-links.queue")
}

/// Resolve a platform-appropriate local-socket name.
/// Mirrors `crate::ipc::IpcEndpoint::resolve_name`; kept here for the
/// synchronous code path. Prefer `IpcEndpoint` for async callers.
fn resolve_ipc_name<'a>(
    ipc_name: &'a str,
) -> std::io::Result<interprocess::local_socket::Name<'a>> {
    if GenericNamespaced::is_supported() {
        ipc_name.to_ns_name::<GenericNamespaced>()
    } else {
        ipc_name.to_fs_name::<GenericFilePath>()
    }
}

fn ipc_name() -> String {
    if GenericNamespaced::is_supported() {
        "com.gpui-starter.app.forwarder".to_string()
    } else {
        queue_file_path()
            .with_extension("sock")
            .display()
            .to_string()
    }
}

/// Return the filesystem path the forwarder socket uses, but only when
/// namespaced sockets are unavailable (so the socket is a real file that
/// can go stale). On Linux abstract sockets there is no file to clean up.
fn filesystem_socket_path(ipc_name: &str) -> Option<PathBuf> {
    if GenericNamespaced::is_supported() {
        None
    } else {
        Some(PathBuf::from(ipc_name))
    }
}

/// Probe whether a listener is currently answering on the forwarder socket.
/// Returns `true` if a connection is accepted (a live primary is running),
/// `false` otherwise (the socket is stale/abandoned).
fn is_socket_live(ipc_name: &str) -> bool {
    match resolve_ipc_name(ipc_name) {
        Ok(name) => Stream::connect(name).is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
#[path = "single_instance.test.rs"]
mod single_instance_test;
