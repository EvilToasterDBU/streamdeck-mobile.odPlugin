use std::sync::OnceLock;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::mobile;

/* ---------- Theme: pulled from OpenDeck's own -info payload ---------- */
//
// The Management and Approval windows each run in their own short-lived
// process (see the module doc comment further down), so they never see the
// `-info` OpenDeck passed to the long-running plugin process on launch. The
// plugin process parses it once and writes the resulting palette to a small
// JSON file; each UI process reads that file on startup. This is the same
// "shared state lives on disk" pattern as `saved_devices_from_disk`.

#[derive(Clone, Copy)]
struct Theme {
    background: egui::Color32,
    panel: egui::Color32,
    accent: egui::Color32,
    text: egui::Color32,
    muted: egui::Color32,
    border: egui::Color32,
}

impl Theme {
    fn fallback() -> Self {
        Self {
            background: egui::Color32::from_rgb(0x1c, 0x1c, 0x1c),
            panel: egui::Color32::from_rgb(0x2b, 0x2b, 0x2b),
            accent: egui::Color32::from_rgb(0xf7, 0x82, 0x1b),
            text: egui::Color32::from_rgb(0xe8, 0xe8, 0xe8),
            muted: egui::Color32::from_rgb(0x96, 0x96, 0x96),
            border: egui::Color32::from_rgb(0x46, 0x46, 0x46),
        }
    }
}

fn parse_hex_color(s: &str) -> Option<egui::Color32> {
    let s = s.trim_start_matches('#');
    let byte = |a: usize, b: usize| u8::from_str_radix(s.get(a..b)?, 16).ok();
    match s.len() {
        6 => Some(egui::Color32::from_rgb(byte(0, 2)?, byte(2, 4)?, byte(4, 6)?)),
        8 => Some(egui::Color32::from_rgba_unmultiplied(byte(0, 2)?, byte(2, 4)?, byte(4, 6)?, byte(6, 8)?)),
        _ => None,
    }
}

fn to_hex(c: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn darken(c: egui::Color32, factor: f32) -> egui::Color32 {
    let f = 1.0 - factor.clamp(0.0, 1.0);
    egui::Color32::from_rgb((c.r() as f32 * f) as u8, (c.g() as f32 * f) as u8, (c.b() as f32 * f) as u8)
}

fn theme_path() -> std::path::PathBuf {
    mobile::config_dir().join("streamdeck-mobile-theme.json")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ThemeFile {
    background: String,
    panel: String,
    accent: String,
    text: String,
    muted: String,
    border: String,
}

/// Called once by the long-running plugin process on startup with OpenDeck's
/// `-info` payload (`{"colors": {"highlightColor": "#F7821BFF", ...}}`).
/// Best-effort: writes a fallback theme if the shape is unexpected or the
/// plugin was launched standalone (no `-info`), so the file always exists.
pub fn persist_theme_from_info(info: Option<&serde_json::Value>) {
    let theme = info
        .and_then(|v| v.get("colors"))
        .and_then(|colors| {
            let get = |key: &str| colors.get(key).and_then(|v| v.as_str()).and_then(parse_hex_color);
            let accent = get("highlightColor")?;
            let panel = get("buttonPressedBackgroundColor").unwrap_or_else(|| egui::Color32::from_rgb(0x2b, 0x2b, 0x2b));
            let border = get("buttonPressedBorderColor").unwrap_or_else(|| egui::Color32::from_rgb(0x46, 0x46, 0x46));
            let muted = get("buttonPressedTextColor").unwrap_or_else(|| egui::Color32::from_rgb(0x96, 0x96, 0x96));
            Some(Theme {
                background: darken(panel, 0.35),
                panel,
                accent,
                text: egui::Color32::from_rgb(0xe8, 0xe8, 0xe8),
                muted,
                border,
            })
        })
        .unwrap_or_else(Theme::fallback);

    let file = ThemeFile {
        background: to_hex(theme.background),
        panel: to_hex(theme.panel),
        accent: to_hex(theme.accent),
        text: to_hex(theme.text),
        muted: to_hex(theme.muted),
        border: to_hex(theme.border),
    };
    if let Ok(json) = serde_json::to_vec_pretty(&file) {
        let path = theme_path();
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("[UI] theme dir create failed: {e}");
            }
        }
        match std::fs::write(&path, json) {
            Ok(()) => log::info!("[UI] theme written to {}", path.display()),
            Err(e) => log::warn!("[UI] theme write failed to {}: {e}", path.display()),
        }
    } else {
        log::warn!("[UI] theme serialize failed");
    }
}

fn load_theme_from_disk() -> Option<Theme> {
    let bytes = std::fs::read(theme_path()).ok()?;
    let file: ThemeFile = serde_json::from_slice(&bytes).ok()?;
    Some(Theme {
        background: parse_hex_color(&file.background)?,
        panel: parse_hex_color(&file.panel)?,
        accent: parse_hex_color(&file.accent)?,
        text: parse_hex_color(&file.text)?,
        muted: parse_hex_color(&file.muted)?,
        border: parse_hex_color(&file.border)?,
    })
}

fn theme() -> Theme {
    static CACHE: OnceLock<Theme> = OnceLock::new();
    *CACHE.get_or_init(|| load_theme_from_disk().unwrap_or_else(Theme::fallback))
}

fn apply_theme(ctx: &egui::Context) {
    let t = theme();
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(t.text);
    visuals.panel_fill = t.background;
    visuals.window_fill = t.background;
    visuals.extreme_bg_color = darken(t.panel, 0.2);
    visuals.faint_bg_color = t.panel;
    visuals.code_bg_color = t.panel;
    visuals.hyperlink_color = t.accent;
    visuals.selection.bg_fill = t.accent;
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, t.accent);
    visuals.window_corner_radius = egui::CornerRadius::same(14);
    visuals.window_stroke = egui::Stroke::new(1.0_f32, t.border);
    visuals.menu_corner_radius = egui::CornerRadius::same(10);

    for (widgets, bg, corner) in [
        (&mut visuals.widgets.noninteractive, t.panel, 10),
        (&mut visuals.widgets.inactive, t.panel, 10),
        (&mut visuals.widgets.hovered, t.accent.gamma_multiply(0.9), 10),
        (&mut visuals.widgets.active, t.accent, 10),
        (&mut visuals.widgets.open, t.panel, 10),
    ] {
        widgets.bg_fill = bg;
        widgets.weak_bg_fill = bg;
        widgets.corner_radius = egui::CornerRadius::same(corner);
        widgets.bg_stroke = egui::Stroke::new(1.0_f32, t.border);
    }
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);

    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(14.0, 8.0);
    });
}

/* ---------- Process lifecycle ---------- */
//
// winit hardcodes a process-wide, one-time-ever EventLoop creation limit
// (see winit's event_loop.rs EVENT_LOOP_CREATED — no reset outside wasm
// builds), and on this Wayland/KDE combination every trick tried to hide and
// later re-show *that same* window turned out to be unreliable too:
// ViewportCommand::Visible(false) left it mapped but frozen,
// ViewportCommand::Minimized(true) worked one-way only (winit logs
// "Unminimizing is ignored on Wayland" — Wayland deliberately gives clients
// no way to un-minimize themselves), and even an always-invisible 4x4
// placeholder root viewport still showed up as a real, closeable, blank
// window in KDE's window switcher.
//
// None of that is a problem a *separate process* has: Management and
// Approval each run in their own short-lived process (a re-invocation of
// this same binary with `--open-management` / `--open-approval`), with an
// entirely ordinary single-viewport eframe app that simply exits when its
// window closes. "Showing" one means spawning a fresh process; "hiding" it
// means it already exited on its own. No hide/show/minimize state to get
// wrong, because there is nothing left running once the window is gone.
// State that both the plugin process and these UI processes need (the saved
// device list, the theme) lives on disk instead of in memory; the handful of
// actions a UI process needs the plugin process to actually perform
// (approve/reject a pairing, rename/remove a device) go over the tiny local
// IPC channel in `ipc.rs`.

// Each "show settings" request used to unconditionally spawn a brand new
// Management process — harmless the first time, but clicking it again while
// an earlier window is still open (no dedup window long enough to matter)
// opened yet another one on top of it. Keep the previous Child around and
// check with try_wait() whether it's still alive before spawning a new one.
static MANAGEMENT_CHILD: OnceLock<std::sync::Mutex<Option<std::process::Child>>> = OnceLock::new();

pub fn spawn_settings_window() {
    let slot = MANAGEMENT_CHILD.get_or_init(|| std::sync::Mutex::new(None));
    let Ok(mut child) = slot.lock() else { return; };
    if let Some(existing) = child.as_mut() {
        match existing.try_wait() {
            Ok(None) => {
                log::info!("[UI] management window already open (pid={}); not opening another", existing.id());
                return;
            }
            Ok(Some(_)) | Err(_) => {
                // Exited (or its status can no longer be queried) — fall
                // through and spawn a fresh one.
            }
        }
    }
    log::info!("[UI] opening native management window");
    *child = spawn_self(&["--open-management"]);
}

pub fn spawn_approval_process(fingerprint: &str, name: &str, peer: &str) {
    log::info!("[UI] opening native approval window fingerprint={fingerprint}");
    let _ = spawn_self(&["--open-approval", "--fingerprint", fingerprint, "--name", name, "--peer", peer]);
}

fn spawn_self(args: &[&str]) -> Option<std::process::Child> {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => { log::warn!("[UI] could not resolve current executable: {error}"); return None; }
    };
    match std::process::Command::new(exe).args(args).spawn() {
        Ok(child) => Some(child),
        Err(error) => { log::warn!("[UI] failed to spawn UI process ({args:?}): {error}"); None }
    }
}

fn event_loop_builder_hook(builder: &mut winit::event_loop::EventLoopBuilder<eframe::UserEvent>) {
    winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(builder, true);
    winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(builder, true);
}

fn native_options(size: [f32; 2], title: &str) -> eframe::NativeOptions {
    let mut options = eframe::NativeOptions::default();
    options.viewport = egui::ViewportBuilder::default()
        .with_inner_size(size)
        .with_min_inner_size(size)
        .with_resizable(false)
        .with_title(title);
    options.event_loop_builder = Some(Box::new(event_loop_builder_hook));
    options
}

/* ---------- Management window ---------- */

struct QrState {
    hostname: String,
    texture: egui::TextureHandle,
}

enum ManagementScreen {
    List,
    AddDevice,
}

struct ManagementApp {
    screen: ManagementScreen,
    devices: Vec<mobile::SavedDevice>,
    last_poll: Instant,
    qr: Option<QrState>,
    pending_remove: Option<String>,
    editing_name: Option<(String, String)>,
    // Fingerprints already saved at the moment "Add device" was opened. Used
    // to detect that a *new* device just finished pairing while the QR
    // screen was up, so we can jump back to the device list automatically
    // instead of leaving the user stranded on a QR code for a pairing that
    // already completed.
    add_device_baseline: std::collections::HashSet<String>,
}

impl Default for ManagementApp {
    fn default() -> Self {
        Self {
            screen: ManagementScreen::List,
            devices: mobile::saved_devices_from_disk(),
            last_poll: Instant::now(),
            qr: None,
            pending_remove: None,
            editing_name: None,
            add_device_baseline: std::collections::HashSet::new(),
        }
    }
}

pub fn run_management_process() -> eframe::Result {
    eframe::run_native(
        "OpenDeck Stream Deck Mobile",
        native_options([420.0, 560.0], "OpenDeck Stream Deck Mobile"),
        Box::new(|cc| {
            apply_theme(&cc.egui_ctx);
            Ok(Box::new(ManagementApp::default()))
        }),
    )
}

impl eframe::App for ManagementApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_poll.elapsed() > Duration::from_millis(500) {
            self.devices = mobile::saved_devices_from_disk();
            self.last_poll = Instant::now();

            if matches!(self.screen, ManagementScreen::AddDevice)
                && self.devices.iter().any(|d| !self.add_device_baseline.contains(&d.fingerprint))
            {
                // A new device just finished pairing while the QR screen was
                // up — return to the list instead of leaving the user on a
                // QR code for a request that's already done.
                self.screen = ManagementScreen::List;
                self.qr = None;
            }
        }
        ctx.request_repaint_after(Duration::from_millis(500));

        match self.screen {
            ManagementScreen::List => self.draw_list(ctx),
            ManagementScreen::AddDevice => self.draw_add_device(ctx),
        }
    }
}

impl ManagementApp {
    fn draw_list(&mut self, ctx: &egui::Context) {
        let t = theme();
        let mut remove = None;
        let mut rename = None;
        let mut add_device = false;

        // A vertical ScrollArea with auto_shrink([false, false]) greedily
        // claims all remaining space in its parent, so anything placed after
        // it in the same CentralPanel never gets room to lay out. Reserve the
        // "Add device" bar as its own bottom panel first; the scrollable
        // device list then sizes itself to whatever's left above it.
        egui::TopBottomPanel::bottom("add_device_bar").show_separator_line(true).show(ctx, |ui| {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                if ui.add_sized([160.0, 36.0], egui::Button::new(egui::RichText::new("Add device").strong())).clicked() {
                    add_device = true;
                }
            });
            ui.add_space(8.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);
            ui.heading(egui::RichText::new("Saved devices").size(18.0));
            ui.add_space(8.0);

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if self.devices.is_empty() {
                    ui.label(egui::RichText::new("No saved Mobile devices yet.").color(t.muted));
                }
                for device in &self.devices {
                    egui::Frame::new()
                        .fill(t.panel)
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&device.name).strong());
                            ui.label(egui::RichText::new(&device.fingerprint).size(11.0).color(t.muted).monospace());
                            ui.horizontal(|ui| {
                                if ui.button("Rename").clicked() {
                                    rename = Some((device.fingerprint.clone(), device.name.clone()));
                                }
                                if ui.button("Remove").clicked() {
                                    remove = Some(device.fingerprint.clone());
                                }
                            });
                        });
                    ui.add_space(6.0);
                }
            });
        });

        if add_device {
            self.add_device_baseline = self.devices.iter().map(|d| d.fingerprint.clone()).collect();
            self.screen = ManagementScreen::AddDevice;
        }
        if let Some(pair) = rename {
            self.editing_name = Some(pair);
        }
        if let Some(fp) = remove {
            self.pending_remove = Some(fp);
        }

        if let Some((fp, old_name)) = self.editing_name.clone() {
            let mut open = true;
            let mut value = old_name;
            let mut save = false;
            egui::Window::new("Rename Mobile device").open(&mut open).show(ctx, |ui| {
                ui.text_edit_singleline(&mut value);
                save = ui.button("Save").clicked();
            });
            if save {
                self.editing_name = None;
                crate::ipc::send_command_blocking(&format!("RENAME {fp} {value}"));
            } else if open {
                self.editing_name = Some((fp, value));
            } else {
                self.editing_name = None;
            }
        }

        if let Some(fp) = self.pending_remove.clone() {
            let mut open = true;
            let mut confirm = false;
            egui::Window::new("Remove device").open(&mut open).show(ctx, |ui| {
                ui.label(format!("Remove {fp}?"));
                confirm = ui.button("Remove").clicked();
            });
            if confirm {
                self.pending_remove = None;
                crate::ipc::send_command_blocking(&format!("REMOVE {fp}"));
            } else if !open {
                self.pending_remove = None;
            }
        }
    }

    fn ensure_qr(&mut self, ctx: &egui::Context) {
        if self.qr.is_some() {
            return;
        }
        let Ok(info) = mobile::pairing_info() else { return; };
        let Ok(image) = mobile::qr_color_image(&info.qr_url) else { return; };
        let texture = ctx.load_texture("streamdeck-mobile-pairing-qr", image, egui::TextureOptions::NEAREST);
        self.qr = Some(QrState { hostname: info.hostname, texture });
    }

    fn draw_add_device(&mut self, ctx: &egui::Context) {
        self.ensure_qr(ctx);
        let t = theme();
        let mut back = false;

        egui::CentralPanel::default().show(ctx, |ui| {
            // egui only rasterizes the glyphs in its own bundled fonts — it
            // doesn't fall back to system fonts — so Unicode arrows like ←
            // render as a missing-glyph box. Plain "<" is in every font.
            if ui.button("< Back").clicked() {
                back = true;
            }
            ui.add_space(12.0);
            if let Some(qr) = &self.qr {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(&qr.hostname).strong());
                    ui.add_space(14.0);
                    let side = 260.0f32.min(ui.available_width());
                    egui::Frame::new()
                        .fill(egui::Color32::WHITE)
                        .corner_radius(egui::CornerRadius::same(12))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.add(egui::Image::new((qr.texture.id(), egui::vec2(side, side))));
                        });
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new("Scan with Stream Deck Mobile to pair.").color(t.muted));
                });
            }
        });

        if back {
            self.screen = ManagementScreen::List;
        }
    }
}

/* ---------- Approval window ---------- */

struct ApprovalApp {
    fingerprint: String,
    name: String,
    peer: String,
    // Set the instant Approve/Reject is clicked, before we ask the viewport
    // to close itself. Without this, our own ViewportCommand::Close makes
    // close_requested() true on the very next frame — indistinguishable from
    // the user dismissing the window — and the close-requested branch below
    // would fire an extra REJECT right on top of the APPROVE that was just
    // sent, undoing it (this is exactly what was breaking pairing: the log
    // showed APPROVE immediately followed by REJECT for the same
    // fingerprint).
    decided: bool,
}

pub fn run_approval_process(fingerprint: String, name: String, peer: String) -> eframe::Result {
    eframe::run_native(
        "OpenDeck Stream Deck Mobile Pairing",
        native_options([560.0, 480.0], "OpenDeck — Stream Deck Mobile pairing"),
        Box::new(move |cc| {
            apply_theme(&cc.egui_ctx);
            Ok(Box::new(ApprovalApp { fingerprint, name, peer, decided: false }))
        }),
    )
}

impl eframe::App for ApprovalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.decided && ctx.input(|i| i.viewport().close_requested()) {
            // Dismissing the window without an explicit choice counts as a
            // reject, so the pairing request doesn't linger with no UI able
            // to act on it.
            self.decided = true;
            crate::ipc::send_command_blocking(&format!("REJECT {}", self.fingerprint));
            return;
        }

        let t = theme();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.heading(egui::RichText::new("Pairing request").size(22.0).strong());
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&self.name).color(t.muted));
                ui.label(egui::RichText::new(&self.peer).size(12.0).color(t.muted));

                ui.add_space(28.0);
                ui.label(egui::RichText::new("Verification code").color(t.muted));
                ui.add_space(6.0);

                egui::Frame::new()
                    .fill(t.panel)
                    .stroke(egui::Stroke::new(1.5_f32, t.accent))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(28, 16))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(&self.fingerprint)
                                .size(34.0)
                                .strong()
                                .color(t.accent)
                                .monospace(),
                        );
                    });

                ui.add_space(10.0);
                ui.label(egui::RichText::new("Confirm this matches the code on your phone.").size(13.0).color(t.muted));
                ui.add_space(32.0);

                ui.horizontal(|ui| {
                    let available = ui.available_width();
                    ui.add_space((available - 260.0).max(0.0) / 2.0);
                    if ui.add_sized([120.0, 44.0], egui::Button::new(egui::RichText::new("Reject").size(16.0))).clicked() {
                        log::info!("[UI] pairing rejection clicked");
                        self.decided = true;
                        crate::ipc::send_command_blocking(&format!("REJECT {}", self.fingerprint));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ui.add_space(20.0);
                    if ui.add_sized([120.0, 44.0], egui::Button::new(egui::RichText::new("Approve").size(16.0).strong())).clicked() {
                        log::info!("[UI] pairing approval clicked");
                        self.decided = true;
                        crate::ipc::send_command_blocking(&format!("APPROVE {}", self.fingerprint));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });
    }
}
