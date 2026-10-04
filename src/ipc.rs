use anyhow::Result;
use tokio::io::AsyncBufReadExt;
use tokio::net::{UnixListener, UnixStream};

use crate::openaction::WsSink;

/// The Management and Approval windows run in their own short-lived
/// processes (see `ui::run_management_process`/`run_approval_process`), not
/// as threads inside the main plugin process — a native window belonging to
/// this same process turned out to run into several Wayland/KDE viewport
/// quirks (see the removed `ui.rs` history) that a genuinely separate,
/// disposable process sidesteps entirely: it just exits when its window
/// closes, no hide/show/minimize state to get wrong.
///
/// Those processes can't reach the main process's in-memory device/pairing
/// state directly, so this is a minimal fire-and-forget command channel over
/// a local Unix socket for the handful of actions that must run in the main
/// process (approve/reject a pairing, rename/remove a saved device). Reads
/// (the saved-device list, pairing/QR info) don't need this: they're served
/// straight off disk or computed locally by the UI process itself.
pub async fn start_server(oa: WsSink) -> Result<()> {
    let path = crate::mobile::ui_ipc_socket_path();
    let _ = std::fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(&path)?;
    log::info!("[UI_IPC] listening path={}", path.display());
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let oa = oa.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(stream, oa).await {
                            log::debug!("[UI_IPC] connection error: {error}");
                        }
                    });
                }
                Err(error) => log::warn!("[UI_IPC] accept failed: {error}"),
            }
        }
    });
    Ok(())
}

async fn handle_connection(stream: UnixStream, oa: WsSink) -> Result<()> {
    let mut reader = tokio::io::BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let line = line.trim();
    log::info!("[UI_IPC] command: {line}");
    let mut parts = line.splitn(3, ' ');
    match parts.next() {
        Some("APPROVE") => {
            if let Some(fp) = parts.next() {
                if let Err(error) = crate::mobile::approve(fp, true).await {
                    log::warn!("[UI_IPC] approve failed: {error}");
                }
            }
        }
        Some("REJECT") => {
            if let Some(fp) = parts.next() {
                if let Err(error) = crate::mobile::approve(fp, false).await {
                    log::warn!("[UI_IPC] reject failed: {error}");
                }
            }
        }
        Some("REMOVE") => {
            if let Some(fp) = parts.next() {
                if let Err(error) = crate::mobile::remove_device(fp, &oa).await {
                    log::warn!("[UI_IPC] remove failed: {error}");
                }
            }
        }
        Some("RENAME") => {
            if let Some(fp) = parts.next() {
                let name = parts.next().unwrap_or("").to_owned();
                if let Err(error) = crate::mobile::rename_device(fp, &name, &oa).await {
                    log::warn!("[UI_IPC] rename failed: {error}");
                }
            }
        }
        _ => log::warn!("[UI_IPC] unknown command: {line}"),
    }
    Ok(())
}

/// Send a one-line fire-and-forget command to the main plugin process. Used
/// only from the short-lived UI processes, which have no Tokio runtime of
/// their own (eframe's event loop is fully synchronous), so this uses a
/// plain blocking `std::os::unix::net::UnixStream` rather than pulling in
/// async machinery for one write.
pub fn send_command_blocking(command: &str) {
    use std::io::Write as _;
    let path = crate::mobile::ui_ipc_socket_path();
    match std::os::unix::net::UnixStream::connect(&path) {
        Ok(mut stream) => {
            if let Err(error) = writeln!(stream, "{command}").and_then(|_| stream.flush()) {
                log::warn!("[UI_IPC] failed to send command: {error}");
            }
        }
        Err(error) => log::warn!("[UI_IPC] failed to reach plugin process at {}: {error}", path.display()),
    }
}
