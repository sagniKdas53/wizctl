//! The full "WiZ Controller" studio window.
//!
//! Port of the Tkinter panel in `src/wizctl/gui.py`: connection card, power
//! banner, brightness card and four control tabs (color wheel, white
//! temperature, scenes, image palette). Every UDP round trip is owned by
//! [`crate::worker::BulbWorker`]; image decoding and the native file chooser
//! run on short-lived helper threads so the UI thread never blocks.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, TextureHandle,
    Ui, Vec2, ViewportBuilder, ViewportCommand,
};

use crate::colors::{get_scene_name, kelvin_to_rgb, parse_color, rgb_to_hex, SCENES};
use crate::palette::{self, ImageInfo, PaletteColor};
use crate::state::{load_state, save_state, validate_ip, State};
use crate::theme;
use crate::worker::{BulbWorker, Cmd, Event};

const WINDOW_TITLE: &str = "WiZ Controller - wizctl";

/// Featured scene ids, mirroring Python `FEATURED_SCENES` (whose labels for
/// ids 4/7/15/17 disagreed with the WiZ table — names come from
/// [`get_scene_name`] instead, so ids 20/31 replace the stale 15/17).
const FEATURED_SCENES: [u32; 12] = [6, 3, 1, 29, 4, 14, 2, 7, 5, 23, 20, 31];

/// Brightness quick presets: label -> raw 0..255 value.
const BRIGHTNESS_PRESETS: [(&str, u8); 5] =
    [("10%", 26), ("25%", 64), ("50%", 128), ("75%", 191), ("100%", 255)];

const KELVIN_PRESETS: [(&str, u16); 4] =
    [("2200K", 2200), ("2700K", 2700), ("4000K", 4000), ("6500K", 6500)];

const PALETTE_COUNTS: [(&str, usize); 4] = [("4", 4), ("6", 6), ("8", 8), ("12", 12)];

/// Longest edge of the palette thumbnail, in pixels.
const THUMB_MAX: u32 = 72;

const RECENT_LIMIT: usize = 16;

// ---------------------------------------------------------------------------
// Tabs & connection status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Wheel,
    Kelvin,
    Scenes,
    Palette,
}

enum Status {
    Connecting,
    Pinging,
    Online {
        latency_ms: u32,
        mac: Option<String>,
        rssi: Option<i32>,
    },
    Offline(String),
}

// ---------------------------------------------------------------------------
// Off-thread palette work
// ---------------------------------------------------------------------------

enum PaletteMsg {
    /// The native file chooser returned a path.
    Chosen(PathBuf),
    /// The chooser closed without a selection.
    Cancelled,
    /// No chooser binary was available, or it failed to launch.
    ChooserFailed(String),
    Extracted {
        path: PathBuf,
        palette: Vec<PaletteColor>,
        info: ImageInfo,
    },
    Failed(String),
}

/// Ask the desktop for an image path. Runs on a helper thread and always
/// reports a terminal message so the caller's "chooser open" latch clears.
fn run_file_chooser() -> PaletteMsg {
    let zenity = Command::new("zenity")
        .arg("--file-selection")
        .arg("--title=Select Image to Extract Color Palette")
        .arg("--file-filter=Image Files | *.jpg *.jpeg *.png *.webp *.bmp *.gif *.tif *.tiff *.JPG *.JPEG *.PNG *.WEBP")
        .arg("--file-filter=All Files | *")
        .output();

    match zenity {
        Ok(out) => {
            let picked = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return if picked.is_empty() {
                PaletteMsg::Cancelled
            } else {
                PaletteMsg::Chosen(PathBuf::from(picked))
            };
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return PaletteMsg::ChooserFailed(format!("zenity failed to launch: {e}"));
        }
        Err(_) => {}
    }

    let kdialog = Command::new("kdialog")
        .arg("--getopenfilename")
        .arg(".")
        .arg("Image Files (*.jpg *.jpeg *.png *.webp *.bmp *.gif *.tif *.tiff)")
        .output();

    match kdialog {
        Ok(out) => {
            let picked = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if picked.is_empty() {
                PaletteMsg::Cancelled
            } else {
                PaletteMsg::Chosen(PathBuf::from(picked))
            }
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            PaletteMsg::ChooserFailed(format!("kdialog failed to launch: {e}"))
        }
        Err(_) => {
            PaletteMsg::ChooserFailed(
                "No file chooser found (install zenity or kdialog)".to_string(),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct StudioApp {
    state: State,
    worker: BulbWorker,
    status: Status,
    tab: Tab,

    // Live widget values (float so the sliders can track sub-step drags).
    ip_input: String,
    hex_input: String,
    brightness: f32,
    kelvin: f32,
    rgb: [u8; 3],

    // Drag latches: incoming bulb polls must not fight the user's pointer.
    bright_dragging: bool,
    kelvin_dragging: bool,
    wheel_dragging: bool,

    // Palette tab.
    palette_count: usize,
    palette: Vec<PaletteColor>,
    image_path: Option<PathBuf>,
    image_info: Option<ImageInfo>,
    thumb: Option<TextureHandle>,
    extracting: bool,
    chooser_open: bool,
    files_hovering: bool,
    msg_tx: Sender<PaletteMsg>,
    msg_rx: Receiver<PaletteMsg>,

    log: String,
    log_is_error: bool,
    dirty: bool,
}

impl StudioApp {
    fn new(cc: &eframe::CreationContext<'_>, target_ip: Option<String>) -> Self {
        let mut state = load_state();
        if let Some(raw) = target_ip {
            if let Ok(ip) = validate_ip(&raw) {
                state.ip = ip;
            }
        }

        let worker = BulbWorker::new(state.ip.clone(), cc.egui_ctx.clone());
        let (msg_tx, msg_rx) = channel();

        Self {
            ip_input: state.ip.clone(),
            hex_input: state.hex.clone(),
            brightness: state.brightness.max(1) as f32,
            kelvin: state.kelvin as f32,
            rgb: state.rgb,
            status: Status::Connecting,
            tab: Tab::Wheel,
            state,
            worker,
            bright_dragging: false,
            kelvin_dragging: false,
            wheel_dragging: false,
            palette_count: 8,
            palette: Vec::new(),
            image_path: None,
            image_info: None,
            thumb: None,
            extracting: false,
            chooser_open: false,
            files_hovering: false,
            msg_tx,
            msg_rx,
            log: format!("wizctl v{} - Ready", env!("CARGO_PKG_VERSION")),
            log_is_error: false,
            dirty: false,
        }
    }

    // -- small helpers ----------------------------------------------------

    fn log(&mut self, msg: impl Into<String>) {
        self.log = msg.into();
        self.log_is_error = false;
    }

    fn log_err(&mut self, msg: impl Into<String>) {
        self.log = msg.into();
        self.log_is_error = true;
    }

    fn is_online(&self) -> bool {
        matches!(self.status, Status::Online { .. })
    }

    fn persist(&mut self) {
        if let Err(e) = save_state(&self.state) {
            self.log_err(format!("Could not save state: {e}"));
        }
    }

    fn record_recent(&mut self, hex: &str) {
        self.state
            .recent_colors
            .retain(|c| !c.eq_ignore_ascii_case(hex));
        self.state.recent_colors.insert(0, hex.to_string());
        self.state.recent_colors.truncate(RECENT_LIMIT);
    }

    // -- commands ---------------------------------------------------------

    fn do_ping(&mut self) {
        match validate_ip(&self.ip_input) {
            Ok(ip) => {
                self.ip_input = ip.clone();
                self.state.ip = ip.clone();
                self.worker.send(Cmd::SetIp(ip.clone()));
                self.status = Status::Pinging;
                self.log(format!("Pinging {ip}..."));
                self.dirty = true;
            }
            Err(e) => self.log_err(e),
        }
    }

    fn toggle_power(&mut self) {
        let next = !self.state.power;
        self.state.power = next;
        self.worker.send(Cmd::Power(next));
        self.log(format!(
            "Turning bulb {}",
            if next { "ON" } else { "OFF" }
        ));
        self.dirty = true;
    }

    fn set_brightness(&mut self, value: u8) {
        let value = value.max(1);
        self.brightness = value as f32;
        self.state.brightness = value;
        self.worker.send(Cmd::Brightness(value));
        let pct = (value as u32 * 100 + 127) / 255;
        self.log(format!("Brightness set to {pct}% ({value}/255)"));
        self.dirty = true;
    }

    fn set_kelvin(&mut self, kelvin: u16) {
        self.kelvin = kelvin as f32;
        self.state.kelvin = kelvin;
        self.state.mode = "kelvin".to_string();
        self.worker.send(Cmd::Kelvin(kelvin));
        self.log(format!("White temperature set to {kelvin} K"));
        self.dirty = true;
    }

    fn set_scene(&mut self, scene_id: u32) {
        self.state.scene_id = scene_id;
        self.state.mode = "scene".to_string();
        self.worker.send(Cmd::Scene(scene_id));
        let name = get_scene_name(scene_id).unwrap_or("Scene");
        self.log(format!("Scene set to {name} (id {scene_id})"));
        self.dirty = true;
    }

    /// Commit an RGB pick: state, recent-color list, bulb command, activity line.
    fn commit_color(&mut self, rgb: [u8; 3], note: Option<String>) {
        let hex = rgb_to_hex(rgb[0], rgb[1], rgb[2]);
        self.rgb = rgb;
        self.state.rgb = rgb;
        self.state.hex = hex.clone();
        self.state.mode = "color".to_string();
        self.hex_input = hex.clone();
        self.record_recent(&hex);
        self.worker.send(Cmd::Rgb(rgb[0], rgb[1], rgb[2]));
        match note {
            Some(text) => self.log(text),
            None => self.log(format!("Color set to {hex}")),
        }
        self.dirty = true;
    }

    fn apply_hex(&mut self, raw: &str) {
        match parse_color(raw) {
            Ok((r, g, b)) => self.commit_color([r, g, b], None),
            Err(e) => self.log_err(e),
        }
    }

    /// Re-push the saved preset after the bulb reappears on the network.
    fn restore_saved_preset(&mut self) {
        self.worker.send(Cmd::Power(self.state.power));
        match self.state.mode.as_str() {
            "scene" => self.worker.send(Cmd::Scene(self.state.scene_id)),
            "kelvin" => self.worker.send(Cmd::Kelvin(self.state.kelvin)),
            _ => {
                let [r, g, b] = self.state.rgb;
                self.worker.send(Cmd::Rgb(r, g, b));
            }
        }
        self.worker
            .send(Cmd::Brightness(self.state.brightness.max(1)));
    }

    // -- worker events ----------------------------------------------------

    fn drain_events(&mut self) {
        while let Some(event) = self.worker.try_recv() {
            match event {
                Event::Online { pilot, latency_ms } => self.apply_online(&pilot, latency_ms),
                Event::Offline(err) => {
                    let was_online = self.is_online();
                    self.status = Status::Offline(err.clone());
                    if was_online || !self.log_is_error {
                        self.log_err(format!("Bulb unreachable: {err}"));
                    }
                }
            }
        }
    }

    fn apply_online(&mut self, pilot: &crate::bulb::PilotResult, latency_ms: u32) {
        let was_offline = !self.is_online();
        self.status = Status::Online {
            latency_ms,
            mac: pilot.mac.clone(),
            rssi: pilot.rssi,
        };

        if was_offline && self.state.restore_on_reconnect {
            self.log("Bulb online - restoring saved preset to bulb...");
            self.restore_saved_preset();
            return;
        }

        self.state.power = pilot.is_on();

        if let Some(b) = pilot.brightness_255() {
            let b = b.max(1);
            self.state.brightness = b;
            if !self.bright_dragging {
                self.brightness = b as f32;
            }
        }

        let scene_id = pilot.scene_id.unwrap_or(0);
        if scene_id != 0 {
            self.state.scene_id = scene_id;
            self.state.mode = "scene".to_string();
        }

        let rgb = pilot.rgb();
        if let Some((r, g, b)) = rgb {
            // The RGB/white split loses brightness, so re-deriving a hex from the
            // confirm poll would nudge the user's pick. Only adopt the readback
            // when the bulb is showing something we did not set.
            if !pilot.matches_rgb(self.state.rgb) {
                self.state.rgb = [r, g, b];
                let hex = rgb_to_hex(r, g, b);
                self.state.hex = hex.clone();
                if !self.wheel_dragging {
                    self.rgb = [r, g, b];
                    self.hex_input = hex;
                }
            }
            if scene_id == 0 {
                self.state.mode = "color".to_string();
            }
        }

        if let Some(k) = pilot.temp.filter(|k| *k > 0) {
            self.state.kelvin = k;
            if !self.kelvin_dragging {
                self.kelvin = k as f32;
            }
            if rgb.is_none() && scene_id == 0 {
                self.state.mode = "kelvin".to_string();
            }
        }

        if was_offline {
            let ip = self.state.ip.clone();
            self.log(format!("Connected to {ip} ({latency_ms}ms)"));
        }
    }

    // -- palette plumbing -------------------------------------------------

    fn open_chooser(&mut self, ctx: &egui::Context) {
        if self.chooser_open {
            return;
        }
        self.chooser_open = true;
        self.log("Opening image chooser...");
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(run_file_chooser());
            ctx.request_repaint();
        });
    }

    fn load_image(&mut self, path: PathBuf, ctx: &egui::Context) {
        if !palette::is_supported_image(&path) {
            self.log_err(format!(
                "Unsupported image type: {}",
                file_label(&path)
            ));
            return;
        }
        self.image_path = Some(path.clone());
        self.extracting = true;
        self.log(format!("Extracting colors from {}...", file_label(&path)));

        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        let count = self.palette_count;
        thread::spawn(move || {
            let msg = match palette::load_image_info(&path, THUMB_MAX) {
                Ok(info) => match palette::extract_palette(&path, count) {
                    Ok(colors) => PaletteMsg::Extracted {
                        path,
                        palette: colors,
                        info,
                    },
                    Err(e) => PaletteMsg::Failed(e),
                },
                Err(e) => PaletteMsg::Failed(e),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    fn drain_palette_msgs(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                PaletteMsg::Chosen(path) => {
                    self.chooser_open = false;
                    self.tab = Tab::Palette;
                    self.load_image(path, ctx);
                }
                PaletteMsg::Cancelled => {
                    self.chooser_open = false;
                }
                PaletteMsg::ChooserFailed(err) => {
                    self.chooser_open = false;
                    self.log_err(err);
                }
                PaletteMsg::Extracted {
                    path,
                    palette,
                    info,
                } => {
                    self.extracting = false;
                    self.thumb = upload_thumb(ctx, &info);
                    self.log(format!(
                        "Extracted {} colors from {}",
                        palette.len(),
                        file_label(&path)
                    ));
                    self.palette = palette;
                    self.image_info = Some(info);
                    self.image_path = Some(path);
                }
                PaletteMsg::Failed(err) => {
                    self.extracting = false;
                    self.log_err(format!("Error reading image palette: {err}"));
                }
            }
        }
    }

    // -- sections ---------------------------------------------------------

    fn connection_card(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("WiZ Bulb IP:").size(11.5).strong());
                let button_w = 54.0;
                let edit_w = (ui.available_width() - button_w - 10.0).max(70.0);
                let edit = ui.add_sized(
                    Vec2::new(edit_w, 22.0),
                    egui::TextEdit::singleline(&mut self.ip_input)
                        .font(egui::TextStyle::Monospace),
                );
                let submitted =
                    edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if blue_button(ui, "Ping", button_w).clicked() || submitted {
                    self.do_ping();
                }
            });

            ui.add_space(4.0);
            let (dot, text, tint, rssi) = self.status_line();
            ui.horizontal(|ui| {
                theme::status_dot(ui, dot);
                ui.label(RichText::new(text).color(tint).size(10.5));
                if let Some(rssi) = rssi {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{rssi} dBm"))
                                .color(theme::TEXT_MUTED)
                                .monospace()
                                .size(9.5),
                        );
                    });
                }
            });

            ui.add_space(2.0);
            let toggled = ui
                .checkbox(
                    &mut self.state.restore_on_reconnect,
                    RichText::new("Auto-restore saved preset when bulb comes online")
                        .color(theme::TEXT_SECONDARY)
                        .size(9.5),
                )
                .changed();
            if toggled {
                let on = self.state.restore_on_reconnect;
                self.log(format!(
                    "Auto-restore on reconnect {}",
                    if on { "enabled" } else { "disabled" }
                ));
                self.dirty = true;
            }
        });
    }

    fn status_line(&self) -> (Color32, String, Color32, Option<i32>) {
        match &self.status {
            Status::Connecting => (
                theme::ACCENT_AMBER,
                "Initializing connection...".to_string(),
                theme::TEXT_SECONDARY,
                None,
            ),
            Status::Pinging => (
                theme::ACCENT_AMBER,
                format!("Pinging {}...", self.state.ip),
                theme::ACCENT_AMBER,
                None,
            ),
            Status::Online {
                latency_ms,
                mac,
                rssi,
            } => {
                let mac = match mac {
                    Some(m) => format!(" - MAC: {m}"),
                    None => String::new(),
                };
                (
                    theme::ACCENT_GREEN,
                    format!("Online ({latency_ms}ms){mac}"),
                    theme::ACCENT_GREEN,
                    *rssi,
                )
            }
            Status::Offline(err) => (
                theme::ACCENT_RED,
                format!("Offline - {}", clip(err, 46)),
                theme::ACCENT_RED,
                None,
            ),
        }
    }

    fn brightness_card(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            let raw = self.brightness.round().clamp(1.0, 255.0) as u8;
            let pct = (raw as u32 * 100 + 127) / 255;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Brightness").size(11.5).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{pct}% ({raw}/255)"))
                            .color(theme::TEXT_SECONDARY)
                            .monospace()
                            .size(10.5),
                    );
                });
            });

            let mut value = self.brightness;
            let out = theme::track_slider(ui, &mut value, 1.0, 255.0, theme::ACCENT_BLUE);
            self.brightness = value;
            if out.changed {
                self.bright_dragging = true;
                let v = value.round().clamp(1.0, 255.0) as u8;
                self.state.brightness = v;
                self.worker.send(Cmd::Brightness(v));
            }
            if out.released {
                self.bright_dragging = false;
                let v = self.brightness.round().clamp(1.0, 255.0) as u8;
                self.set_brightness(v);
            }

            ui.add_space(2.0);
            let current = self.state.brightness;
            let picked = theme::chip_row(
                ui,
                &BRIGHTNESS_PRESETS,
                |v: u8| v == current,
                20.0,
            );
            if let Some(v) = picked {
                self.set_brightness(v);
            }
        });
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        let active = self.tab;
        let picked = theme::chip_row(
            ui,
            &[
                ("Color Wheel", Tab::Wheel),
                ("White (Kelvin)", Tab::Kelvin),
                ("Scenes", Tab::Scenes),
                ("Palette", Tab::Palette),
            ],
            |t: Tab| t == active,
            26.0,
        );
        if let Some(tab) = picked {
            self.tab = tab;
        }
    }

    fn wheel_body(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            let mut rgb = self.rgb;
            let out = ui
                .vertical_centered(|ui| theme::color_wheel(ui, 210.0, &mut rgb))
                .inner;
            self.rgb = rgb;
            if out.changed {
                self.wheel_dragging = true;
                self.worker.send(Cmd::Rgb(rgb[0], rgb[1], rgb[2]));
            }
            if out.released {
                self.wheel_dragging = false;
                self.commit_color(rgb, None);
            }

            ui.add_space(6.0);
            let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            theme::swatch(ui, color, Vec2::new(ui.available_width(), 34.0), false);

            ui.add_space(4.0);
            theme::section(ui, "HEX CODE");
            ui.horizontal(|ui| {
                let button_w = 52.0;
                let edit_w = (ui.available_width() - button_w - 10.0).max(70.0);
                let edit = ui.add_sized(
                    Vec2::new(edit_w, 22.0),
                    egui::TextEdit::singleline(&mut self.hex_input)
                        .font(egui::TextStyle::Monospace),
                );
                let submitted =
                    edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if blue_button(ui, "Apply", button_w).clicked() || submitted {
                    let raw = self.hex_input.clone();
                    self.apply_hex(&raw);
                }
            });

            ui.label(
                RichText::new(format!("RGB: {}, {}, {}", rgb[0], rgb[1], rgb[2]))
                    .color(theme::TEXT_SECONDARY)
                    .monospace()
                    .size(9.5),
            );

            ui.add_space(4.0);
            theme::section(ui, "RECENT COLORS");
            let recents = self.state.recent_colors.clone();
            let active_hex = self.state.hex.clone();
            let mut picked: Option<[u8; 3]> = None;
            ui.horizontal_wrapped(|ui| {
                for hex in &recents {
                    if let Ok((r, g, b)) = parse_color(hex) {
                        let active = hex.eq_ignore_ascii_case(&active_hex);
                        if theme::color_dot(ui, Color32::from_rgb(r, g, b), 9.0, active).clicked()
                        {
                            picked = Some([r, g, b]);
                        }
                    }
                }
            });
            if let Some(rgb) = picked {
                self.commit_color(rgb, None);
            }
        });
    }

    fn kelvin_body(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            let shown = self.kelvin.round().clamp(2200.0, 6500.0) as u16;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Color Temperature").size(11.5).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{shown} K"))
                            .color(theme::ACCENT_AMBER)
                            .monospace()
                            .size(20.0)
                            .strong(),
                    );
                });
            });

            ui.add_space(4.0);
            let mut value = self.kelvin;
            let out = theme::gradient_slider(ui, &mut value, 2200.0, 6500.0, |t| {
                let kelvin = (2200.0 + t * 4300.0).round().clamp(1000.0, 10000.0) as u16;
                let (r, g, b) = kelvin_to_rgb(kelvin);
                Color32::from_rgb(r, g, b)
            });
            self.kelvin = value;
            if out.changed {
                self.kelvin_dragging = true;
                let k = value.round().clamp(2200.0, 6500.0) as u16;
                self.state.kelvin = k;
                self.state.mode = "kelvin".to_string();
                self.worker.send(Cmd::Kelvin(k));
            }
            if out.released {
                self.kelvin_dragging = false;
                let k = self.kelvin.round().clamp(2200.0, 6500.0) as u16;
                self.set_kelvin(k);
            }

            ui.add_space(6.0);
            theme::section(ui, "TEMPERATURE PRESETS");
            // Only badge a preset as active when the bulb is actually in white mode.
            let current = if self.state.mode == "kelvin" {
                self.state.kelvin
            } else {
                0
            };
            let picked = theme::chip_row(ui, &KELVIN_PRESETS, |v: u16| v == current, 24.0);
            if let Some(k) = picked {
                self.set_kelvin(k);
            }
        });
    }

    fn scenes_body(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            theme::section(ui, "FEATURED SCENES");
            ui.add_space(2.0);

            let active = if self.state.mode == "scene" {
                Some(self.state.scene_id)
            } else {
                None
            };
            let mut picked: Option<u32> = None;

            let spacing = 4.0;
            let width = ((ui.available_width() - spacing * 2.0) / 3.0).max(40.0);
            for row in FEATURED_SCENES.chunks(3) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = spacing;
                    for &id in row {
                        let name = get_scene_name(id).unwrap_or("Scene");
                        if theme::chip(ui, name, active == Some(id), Vec2::new(width, 30.0))
                            .clicked()
                        {
                            picked = Some(id);
                        }
                    }
                });
            }

            ui.add_space(6.0);
            egui::CollapsingHeader::new(
                RichText::new("All Scenes")
                    .color(theme::TEXT_SECONDARY)
                    .size(10.5)
                    .strong(),
            )
            .id_salt("studio_all_scenes")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                    for &(id, name) in SCENES {
                        if theme::chip(ui, name, active == Some(id), Vec2::new(88.0, 20.0))
                            .clicked()
                        {
                            picked = Some(id);
                        }
                    }
                });
            });

            if let Some(id) = picked {
                self.set_scene(id);
            }
        });
    }

    fn palette_body(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        // --- drop zone ---
        let border = if self.files_hovering {
            theme::ACCENT_BLUE
        } else {
            theme::CARD_BORDER
        };
        let mut open_chooser = false;
        let zone = egui::Frame::new()
            .fill(theme::INPUT_BG)
            .stroke(Stroke::new(if self.files_hovering { 2.0_f32 } else { 1.0_f32 }, border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(10, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Drag & drop image here or click 'Select Image...'")
                            .color(theme::TEXT_PRIMARY)
                            .size(10.0)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if blue_button(ui, "Select Image...", 96.0).clicked() {
                            open_chooser = true;
                        }
                    });
                });
            })
            .response;
        if zone.interact(Sense::click()).clicked() {
            open_chooser = true;
        }
        if open_chooser {
            self.open_chooser(ctx);
        }

        ui.add_space(6.0);

        // --- image info ---
        theme::card(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(tex) = &self.thumb {
                    let size = tex.size_vec2();
                    ui.image(egui::load::SizedTexture::new(tex.id(), size));
                    ui.add_space(6.0);
                }
                ui.vertical(|ui| {
                    if self.extracting {
                        ui.label(
                            RichText::new("Extracting colors...")
                                .color(theme::ACCENT_AMBER)
                                .size(10.0)
                                .strong(),
                        );
                    }
                    match (&self.image_path, &self.image_info) {
                        (Some(path), Some(info)) => {
                            ui.label(
                                RichText::new(file_label(path))
                                    .color(theme::TEXT_PRIMARY)
                                    .size(10.5)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Resolution: {} x {} px",
                                    info.width, info.height
                                ))
                                .color(theme::TEXT_SECONDARY)
                                .monospace()
                                .size(9.5),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Colors extracted: {}",
                                    self.palette.len()
                                ))
                                .color(theme::TEXT_SECONDARY)
                                .monospace()
                                .size(9.5),
                            );
                        }
                        _ => {
                            if !self.extracting {
                                ui.label(
                                    RichText::new(
                                        "No image loaded. Drag & drop or select an image to extract colors.",
                                    )
                                    .color(theme::TEXT_MUTED)
                                    .size(9.5),
                                );
                            }
                        }
                    }
                });
            });
        });

        ui.add_space(6.0);
        theme::section(ui, "COLOR COUNT");
        let current_count = self.palette_count;
        let picked = theme::chip_row(ui, &PALETTE_COUNTS, |v: usize| v == current_count, 20.0);
        if let Some(count) = picked {
            if count != self.palette_count {
                self.palette_count = count;
                if let Some(path) = self.image_path.clone() {
                    self.load_image(path, ctx);
                }
            }
        }

        ui.add_space(6.0);
        ui.label(
            RichText::new("Extracted Color Palette (Click to Apply):")
                .color(theme::TEXT_MUTED)
                .size(9.5)
                .strong(),
        );
        ui.add_space(3.0);

        if self.palette.is_empty() {
            ui.label(
                RichText::new("No colors yet.")
                    .color(theme::TEXT_MUTED)
                    .size(9.5),
            );
            return;
        }

        let entries = self.palette.clone();
        let col_w = ((ui.available_width() - 6.0) / 2.0).max(90.0);
        let mut clicked: Option<PaletteColor> = None;
        for row in entries.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for color in row {
                    let card = ui
                        .allocate_ui(Vec2::new(col_w, 44.0), |ui| {
                            ui.set_min_width(col_w);
                            egui::Frame::new()
                                .fill(theme::INPUT_BG)
                                .stroke(Stroke::new(1.0_f32, theme::CARD_BORDER))
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(Margin::symmetric(7, 5))
                                .show(ui, |ui| {
                                    ui.set_min_width(col_w - 16.0);
                                    ui.horizontal(|ui| {
                                        theme::swatch(
                                            ui,
                                            Color32::from_rgb(color.r, color.g, color.b),
                                            Vec2::new(28.0, 28.0),
                                            false,
                                        );
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(color.hex().to_uppercase())
                                                    .color(theme::TEXT_PRIMARY)
                                                    .monospace()
                                                    .size(10.5)
                                                    .strong(),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "{:.1}% dominance",
                                                    color.percentage
                                                ))
                                                .color(theme::TEXT_MUTED)
                                                .monospace()
                                                .size(9.0),
                                            );
                                        });
                                    });
                                })
                                .response
                        })
                        .inner;
                    if card.interact(Sense::click()).clicked() {
                        clicked = Some(*color);
                    }
                }
            });
        }

        if let Some(color) = clicked {
            let hex = color.hex();
            let note = format!(
                "Applied image palette color {hex} ({:.1}%)",
                color.percentage
            );
            self.commit_color([color.r, color.g, color.b], Some(note));
        }
    }

    /// Color the titlebar bulb glows with, following the bulb's active mode.
    fn accent(&self) -> Color32 {
        match self.state.mode.as_str() {
            "kelvin" => {
                let (r, g, b) = kelvin_to_rgb(self.state.kelvin);
                Color32::from_rgb(r, g, b)
            }
            "scene" => Color32::from_rgb(255, 180, 50),
            _ => Color32::from_rgb(self.state.rgb[0], self.state.rgb[1], self.state.rgb[2]),
        }
    }

    /// Painted titlebar: drag to move, double-click to maximize, own buttons.
    fn title_bar(&mut self, ctx: &egui::Context) {
        let frame = egui::Frame::new()
            .fill(theme::CARD_BG)
            .corner_radius(CornerRadius {
                nw: 10,
                ne: 10,
                sw: 0,
                se: 0,
            })
            .inner_margin(Margin::symmetric(8, 0));

        egui::TopBottomPanel::top("studio_titlebar")
            .exact_height(36.0)
            .frame(frame)
            .show(ctx, |ui| {
                let bar = ui.max_rect();
                let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);

                // Claimed first so the buttons drawn below take interaction priority.
                let drag = ui.interact(bar, ui.id().with("drag"), Sense::click_and_drag());
                if drag.drag_started() {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                if drag.double_clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                }

                ui.painter().hline(
                    bar.left() - 8.0..=bar.right() + 8.0,
                    bar.bottom(),
                    Stroke::new(1.0_f32, theme::CARD_BORDER),
                );

                let mut close_requested = false;
                ui.horizontal_centered(|ui| {
                    let (icon, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
                    theme::bulb_icon(
                        ui.painter(),
                        icon.center(),
                        7.0,
                        self.state.power,
                        self.accent(),
                    );
                    ui.label(
                        RichText::new("WiZ Controller")
                            .color(theme::TEXT_PRIMARY)
                            .strong()
                            .size(12.5),
                    );
                    ui.label(RichText::new("wizctl").color(theme::TEXT_MUTED).size(9.5));

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if theme::window_button(ui, theme::WinButton::Close).clicked() {
                            close_requested = true;
                        }
                        let restore = if maximized {
                            theme::WinButton::Restore
                        } else {
                            theme::WinButton::Maximize
                        };
                        if theme::window_button(ui, restore).clicked() {
                            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                        }
                        if theme::window_button(ui, theme::WinButton::Minimize).clicked() {
                            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                        }
                    });
                });

                if close_requested {
                    let _ = save_state(&self.state);
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            });
    }
}

impl eframe::App for StudioApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.drain_palette_msgs(ctx);
        // Rounded window body, painted before the panels claim the layer.
        let screen = ctx.screen_rect();
        let backdrop = ctx.layer_painter(egui::LayerId::background());
        backdrop.rect_filled(screen, CornerRadius::same(10), theme::BG_DARK);
        backdrop.rect_stroke(
            screen,
            CornerRadius::same(10),
            Stroke::new(1.0_f32, theme::CARD_BORDER),
            egui::StrokeKind::Inside,
        );


        self.files_hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            let candidate = dropped
                .iter()
                .filter_map(|f| f.path.clone())
                .find(|p| palette::is_supported_image(p));
            match candidate {
                Some(path) => {
                    self.tab = Tab::Palette;
                    self.load_image(path, ctx);
                }
                None => self.log_err("Dropped file is not a supported image"),
            }
        }

        let footer = egui::Frame::new()
            .fill(Color32::TRANSPARENT)
            .inner_margin(Margin::symmetric(12, 5));
        egui::TopBottomPanel::bottom("studio_activity")
            .frame(footer)
            .show(ctx, |ui| {
                let color = if self.log_is_error {
                    theme::ACCENT_RED
                } else {
                    theme::TEXT_MUTED
                };
                ui.label(
                    RichText::new(clip(&self.log, 150))
                        .color(color)
                        .monospace()
                        .size(9.5),
                );
            });

        self.title_bar(ctx);
        theme::resize_grips(ctx);

        let body = egui::Frame::new()
            .fill(Color32::TRANSPARENT)
            .inner_margin(Margin::symmetric(12, 10));
        egui::CentralPanel::default().frame(body).show(ctx, |ui| {
            self.connection_card(ui);
            ui.add_space(8.0);

            if theme::power_banner(ui, self.state.power, self.is_online()).clicked() {
                self.toggle_power();
            }
            ui.add_space(8.0);

            self.brightness_card(ui);
            ui.add_space(8.0);

            self.tab_bar(ui);
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.tab {
                    Tab::Wheel => self.wheel_body(ui),
                    Tab::Kelvin => self.kelvin_body(ui),
                    Tab::Scenes => self.scenes_body(ui),
                    Tab::Palette => self.palette_body(ui, ctx),
                });
        });

        if self.dirty {
            self.dirty = false;
            self.persist();
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            let _ = save_state(&self.state);
        }
    }
}

// ---------------------------------------------------------------------------
// Small free helpers
// ---------------------------------------------------------------------------

fn blue_button(ui: &mut Ui, label: &str, width: f32) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .color(Color32::WHITE)
                .size(10.0)
                .strong(),
        )
        .fill(theme::ACCENT_BLUE)
        .corner_radius(CornerRadius::same(5))
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(width, 22.0)),
    )
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Upload (or replace) the palette thumbnail texture.
fn upload_thumb(ctx: &egui::Context, info: &ImageInfo) -> Option<TextureHandle> {
    let expected = info
        .thumb_width
        .checked_mul(info.thumb_height)
        .and_then(|px| px.checked_mul(4))?;
    if expected == 0 || info.thumb_rgba.len() != expected {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [info.thumb_width, info.thumb_height],
        &info.thumb_rgba,
    );
    Some(ctx.load_texture(
        "studio_palette_thumb",
        image,
        egui::TextureOptions::LINEAR,
    ))
}

fn load_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon_64.png"));
    let decoded = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = (decoded.width(), decoded.height());
    Some(egui::IconData {
        rgba: decoded.into_raw(),
        width,
        height,
    })
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Launch the frameless, resizable studio window.
///
/// Decorations are off: the titlebar, window buttons and resize grips are all
/// painted by egui so the studio matches the popover instead of inheriting the
/// desktop's window frame.
pub fn run_studio(target_ip: Option<String>) -> Result<(), eframe::Error> {
    let mut viewport = ViewportBuilder::default()
        .with_title(WINDOW_TITLE)
        .with_app_id("wizctl")
        .with_inner_size(Vec2::new(520.0, 820.0))
        .with_min_inner_size(Vec2::new(460.0, 640.0))
        .with_resizable(true)
        .with_decorations(false)
        .with_transparent(true)
        .with_taskbar(true);

    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        WINDOW_TITLE,
        native_options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(StudioApp::new(cc, target_ip)))
        }),
    )
}
