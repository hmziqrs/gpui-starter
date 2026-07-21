#![allow(dead_code)]

use std::{io, path::PathBuf};

use directories::ProjectDirs;
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions,
    prelude::*,
    tokio::Stream as TokioStream,
    traits::tokio::{Listener as _, Stream as _},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const LOG: &str = "gpui_starter::ipc";

/// Reusable IPC endpoint backed by a local domain socket.
///
/// Provides a thin abstraction over `interprocess` local sockets with
/// newline-delimited framing. Each message is a single UTF-8 line
/// terminated by `\n`.
///
/// # Example
///
/// ```ignore
/// let endpoint = IpcEndpoint::new("my-channel");
///
/// // Server side
/// endpoint.listen(|msg| {
///     println!("received: {msg}");
/// }).await?;
///
/// // Client side
/// endpoint.send("hello world").await?;
/// ```
#[derive(Debug)]
pub struct IpcEndpoint {
    socket_path: PathBuf,
}

impl IpcEndpoint {
    /// Create a new endpoint identified by `name`.
    ///
    /// On platforms that support namespaced sockets (macOS, Linux abstract
    /// sockets) the name is used directly. Otherwise it is resolved to a
    /// filesystem path inside the application data directory.
    pub fn new(name: &str) -> Self {
        let socket_path = Self::resolve_socket_path(name);
        Self { socket_path }
    }

    /// Connect to the socket and send a single newline-terminated message.
    ///
    /// Returns an error if no listener is active or the write fails.
    pub async fn send(&self, message: &str) -> io::Result<()> {
        let name = self.resolve_name()?;
        let conn: TokioStream = TokioStream::connect(name).await?;
        (&conn).write_all(format!("{message}\n").as_bytes()).await?;
        tracing::debug!(target: LOG, %message, "ipc message sent");
        Ok(())
    }

    /// Bind to the socket and invoke `handler` for every incoming message.
    ///
    /// Blocks until the returned future is dropped or a fatal error occurs.
    /// The `handler` closure receives the trimmed message payload (without the
    /// trailing newline).
    pub async fn listen<F>(&self, handler: F) -> io::Result<()>
    where
        F: Fn(String),
    {
        let name = self.resolve_name()?;
        let listener = ListenerOptions::new().name(name).create_tokio()?;
        tracing::info!(target: LOG, path = %self.socket_path.display(), "ipc listener started");

        loop {
            let conn = listener.accept().await?;
            let mut reader = BufReader::new(&conn);
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => continue,
                Ok(_) => {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        tracing::trace!(target: LOG, msg = %trimmed, "ipc message received");
                        handler(trimmed);
                    }
                }
            }
        }
    }

    /// Check whether a listener is likely active by attempting a connection.
    ///
    /// Returns `true` if the socket accepts a connection, `false` otherwise.
    pub fn is_listening(&self) -> bool {
        let name = match self.resolve_name() {
            Ok(n) => n,
            Err(_) => return false,
        };
        interprocess::local_socket::Stream::connect(name).is_ok()
    }

    // -- helpers --------------------------------------------------------

    /// Build a socket path in the application cache/runtime directory.
    fn resolve_socket_path(name: &str) -> PathBuf {
        if let Some(project_dirs) = ProjectDirs::from("com", "gpui-starter", "GPUI Starter") {
            project_dirs
                .cache_dir()
                .join("runtime")
                .join(format!("{name}.sock"))
        } else {
            std::env::temp_dir().join(format!("gpui-starter-{name}.sock"))
        }
    }

    /// Resolve the platform-appropriate socket name.
    fn resolve_name(&self) -> io::Result<interprocess::local_socket::Name<'_>> {
        if GenericNamespaced::is_supported() {
            self.socket_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("gpui-starter-ipc")
                .to_ns_name::<GenericNamespaced>()
        } else {
            self.socket_path
                .display()
                .to_string()
                .to_fs_name::<GenericFilePath>()
        }
    }
}

pub mod command;
pub mod rpc;

pub use command::{ForwardedCommand, ForwardedRequest, ForwardedResponse};
pub use rpc::{PendingRequests, decode_request, decode_response, encode_line};

#[cfg(test)]
#[path = "mod.test.rs"]
mod mod_test;
