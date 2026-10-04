#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ipc;
mod mobile;
mod openaction;
mod ui;

use anyhow::Result;
use openaction::{WsSink, connect_plugin};
use std::{fs::OpenOptions, io::Write, sync::OnceLock};

static OPENACTION_SINK: OnceLock<WsSink> = OnceLock::new();

pub async fn rename_command(fingerprint: &str, name: &str) -> Result<()> {
    let sink = OPENACTION_SINK.get().ok_or_else(|| anyhow::anyhow!("OpenDeck sink unavailable"))?.clone();
    mobile::rename_device(fingerprint, name, &sink).await
}

pub async fn remove_command(fingerprint: &str) -> Result<()> {
    let sink = OPENACTION_SINK.get().ok_or_else(|| anyhow::anyhow!("OpenDeck sink unavailable"))?.clone();
    mobile::remove_device(fingerprint, &sink).await
}

#[tokio::main]
async fn main() -> Result<()> {
    let log_path = std::env::var_os("OPENDECK_MOBILE_LOG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".local/state/opendeck-streamdeck-mobile/plugin.log")
        });
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log_file = OpenOptions::new().create(true).append(true).open(&log_path)?;

    simplelog::CombinedLogger::init(vec![
        simplelog::TermLogger::new(
            log::LevelFilter::Info,
            simplelog::Config::default(),
            simplelog::TerminalMode::Mixed,
            simplelog::ColorChoice::Auto,
        ),
        simplelog::WriteLogger::new(
            log::LevelFilter::Info,
            simplelog::Config::default(),
            log_file,
        ),
    ])?;

    let panic_log_path = log_path.clone();
    std::panic::set_hook(Box::new(move |panic| {
        log::error!("[PANIC] {panic}");
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&panic_log_path) {
            let _ = writeln!(file, "[PANIC] {panic}");
        }
    }));

    log::info!("[LOGGER] persistent log={}", log_path.display());
    log::info!("[LOGGER] raw mobile frames={}",
        if std::env::var("OPENDECK_MOBILE_RAW").ok().as_deref().unwrap_or("1") != "0" { "ON" } else { "OFF" });

    let args: Vec<String> = std::env::args().collect();
    fn value_after(args: &[String], key: &str) -> Option<String> {
        let pos = args.iter().position(|arg| arg == key)?;
        args.get(pos + 1).cloned()
    }

    // The Management and Approval windows are separate, short-lived
    // processes (re-invocations of this same binary) rather than threads in
    // the long-running plugin process — see ui.rs for why. Each of these
    // just runs its window synchronously and exits when it closes.
    if args.iter().any(|arg| arg == "--open-management") {
        log::info!("[PLUGIN] opening standalone Stream Deck Mobile management window");
        return ui::run_management_process().map_err(|e| anyhow::anyhow!("{e}"));
    }
    if args.iter().any(|arg| arg == "--open-approval") {
        let fingerprint = value_after(&args, "--fingerprint").ok_or_else(|| anyhow::anyhow!("--open-approval requires --fingerprint"))?;
        let name = value_after(&args, "--name").unwrap_or_default();
        let peer = value_after(&args, "--peer").unwrap_or_default();
        log::info!("[PLUGIN] opening standalone Stream Deck Mobile approval window fingerprint={fingerprint}");
        return ui::run_approval_process(fingerprint, name, peer).map_err(|e| anyhow::anyhow!("{e}"));
    }

    let (sink, rx, info) = connect_plugin(&args).await?;
    ui::persist_theme_from_info(info.as_ref());
    OPENACTION_SINK.set(sink.clone()).map_err(|_| anyhow::anyhow!("plugin sink already initialized"))?;

    log::info!("[PLUGIN] OpenDeck Stream Deck Mobile v{} started", env!("CARGO_PKG_VERSION"));

    ipc::start_server(sink.clone()).await?;
    mobile::initialize_saved_devices(&sink).await?;
    mobile::start_vsd2(sink.clone()).await?;
    mobile::spawn_openaction_reader(sink.clone(), rx.resubscribe());


    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
