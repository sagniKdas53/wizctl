//! Compact panel popover widget.
//!
//! Ephemeral by design: the process exits when the popover closes, so idle RAM
//! and file descriptors drop to zero. See `WIZCTL_RUST_REWRITE_PLAN.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Pos2, Sense, Stroke, Vec2,
    ViewportBuilder, ViewportCommand, X11WindowType,
};

use crate::colors::{kelvin_to_rgb, parse_color, rgb_to_hex};
use crate::palette::{extract_palette, is_supported_image, PaletteColor};
use crate::state::{load_state, save_state, State};
use crate::theme;
use crate::worker::{BulbWorker, Cmd, Event};

/// Quick scene chips, mirroring the Python widget's `QUICK_SCENES`.
const QUICK_SCENES: &[(&str, u32)] = &[("Cozy", 6), ("Sunset", 3), ("Ocean", 1), ("Night", 14)];

/// White temperature chips, mirroring the Python widget's `QUICK_KELVIN`.
const QUICK_KELVIN: &[(&str, u16)] = &[
    ("2200K", 2200),
    ("2700K", 2700),
    ("4000K", 4000),
    ("6500K", 6500),
];

/// Brightness chips, mirroring the Python widget's quick brightness row.
const QUICK_BRIGHTNESS: &[(&str, u8)] = &[("25%", 64), ("50%", 128), ("75%", 191), ("100%", 255)];

const WIN_W: f32 = 340.0;
const BASE_H: f32 = 444.0;
const PICKER_H: f32 = 238.0;
const PALETTE_ROW_H: f32 = 20.0;

// ---------------------------------------------------------------------------
// Single-instance PID guard & debounce latch
// ---------------------------------------------------------------------------
pub struct PidGuard;

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file("/tmp/wizctl_gui.pid");
        let _ = fs::write("/tmp/wizctl_closed_stamp", format!("{}", now_millis()));
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn check_single_instance() -> Option<PidGuard> {
    let pid_file = Path::new("/tmp/wizctl_gui.pid");
    if pid_file.exists() {
        if let Ok(content) = fs::read_to_string(pid_file) {
            if let Ok(pid) = content.trim().parse::<i32>() {
                if Path::new(&format!("/proc/{pid}")).exists() {
                    let _ = Command::new("kill").arg(format!("{pid}")).status();
                    let _ = fs::remove_file(pid_file);
                    let _ = fs::write("/tmp/wizctl_closed_stamp", format!("{}", now_millis()));
                    return None; // Toggled off.
                }
            }
        }
    }

    let stamp_file = Path::new("/tmp/wizctl_closed_stamp");
    if stamp_file.exists() {
        if let Ok(content) = fs::read_to_string(stamp_file) {
            if let Ok(last_closed) = content.trim().parse::<u128>() {
                if now_millis().saturating_sub(last_closed) < 250 {
                    let _ = fs::remove_file(stamp_file);
                    return None; // Debounce latch: stay closed.
                }
            }
        }
    }

    let _ = fs::write(pid_file, format!("{}", std::process::id()));
    Some(PidGuard)
}

// ---------------------------------------------------------------------------
// Cursor-anchored placement
// ---------------------------------------------------------------------------
pub fn get_mouse_position() -> (f32, f32) {
    if let Ok(out) = Command::new("xdotool").arg("getmouselocation").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        let mut x = 600.0_f32;
        let mut y = 26.0_f32;
        for part in s.split_whitespace() {
            if let Some(val) = part.strip_prefix("x:") {
                if let Ok(v) = val.parse::<f32>() {
                    x = v;
                }
            } else if let Some(val) = part.strip_prefix("y:") {
                if let Ok(v) = val.parse::<f32>() {
                    y = v;
                }
            }
        }
        return (x, y);
    }
    (600.0_f32, 26.0_f32)
}

// ---------------------------------------------------------------------------
// Dropped-image palette
// ---------------------------------------------------------------------------
struct DroppedPalette {
    name: String,
    colors: Vec<PaletteColor>,
}

#[derive(PartialEq)]
enum Link {
    Pinging,
    Online,
    Offline,
}

// ---------------------------------------------------------------------------
// Popover app
// ---------------------------------------------------------------------------
pub struct PopoverApp {
    opened_at: Instant,
    has_gained_focus: bool,
    is_pinned: bool,
    state: State,
    /// Live slider value in 1..=255; committed into `state.brightness`.
    brightness: f32,
    /// Live slider value in 2200..=6500; committed into `state.kelvin`.
    kelvin: f32,
    link: Link,
    latency_ms: u32,
    rssi: Option<i32>,
    custom_color: Color32,
    show_picker: bool,
    palette: Option<DroppedPalette>,
    palette_error: Option<String>,
    palette_rx: Option<Receiver<Result<DroppedPalette, String>>>,
    extracting: bool,
    requested_height: f32,
    /// Screen position the popover was mapped at, and whether it hangs above
    /// the cursor. When it does, growth must extend upward so the popover never
    /// slides under the panel it was launched from.
    origin: Pos2,
    grows_upward: bool,
    worker: BulbWorker,
    _pid_guard: Option<PidGuard>,
}

impl PopoverApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        target_ip: Option<String>,
        pid_guard: Option<PidGuard>,
        origin: Pos2,
        grows_upward: bool,
    ) -> Self {
        theme::apply(&cc.egui_ctx);

        let mut state = load_state();
        if let Some(ip) = target_ip {
            state.ip = ip;
        }

        let worker = BulbWorker::new(state.ip.clone(), cc.egui_ctx.clone());
        let custom_color = Color32::from_rgb(state.rgb[0], state.rgb[1], state.rgb[2]);

        Self {
            opened_at: Instant::now(),
            has_gained_focus: false,
            is_pinned: false,
            brightness: state.brightness as f32,
            kelvin: state.kelvin.clamp(2200, 6500) as f32,
            state,
            link: Link::Pinging,
            latency_ms: 0,
            rssi: None,
            custom_color,
            show_picker: false,
            palette: None,
            palette_error: None,
            palette_rx: None,
            extracting: false,
            requested_height: BASE_H,
            origin,
            grows_upward,
            worker,
            _pid_guard: pid_guard,
        }
    }

    fn close(&mut self, ctx: &egui::Context) {
        let _ = save_state(&self.state);
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn glow(&self) -> Color32 {
        match self.state.mode.as_str() {
            "kelvin" => {
                let (r, g, b) = kelvin_to_rgb(self.state.kelvin);
                Color32::from_rgb(r, g, b)
            }
            "scene" => Color32::from_rgb(255, 180, 50),
            _ => Color32::from_rgb(self.state.rgb[0], self.state.rgb[1], self.state.rgb[2]),
        }
    }

    fn apply_rgb(&mut self, r: u8, g: u8, b: u8) {
        self.state.rgb = [r, g, b];
        self.state.hex = rgb_to_hex(r, g, b);
        self.state.mode = "color".to_string();
        self.state.power = true;
        self.custom_color = Color32::from_rgb(r, g, b);
        self.record_recent(rgb_to_hex(r, g, b));
        self.worker.send(Cmd::Rgb(r, g, b));
        let _ = save_state(&self.state);
    }

    fn record_recent(&mut self, hex: String) {
        self.state.recent_colors.retain(|c| c != &hex);
        self.state.recent_colors.insert(0, hex);
        self.state.recent_colors.truncate(16);
    }

    fn drain_worker(&mut self) {
        while let Some(evt) = self.worker.try_recv() {
            match evt {
                Event::Online { pilot, latency_ms } => {
                    self.link = Link::Online;
                    self.latency_ms = latency_ms;
                    self.rssi = pilot.rssi;

                    if let Some(on) = pilot.state {
                        self.state.power = on;
                    }
                    if let Some(b255) = pilot.brightness_255() {
                        self.state.brightness = b255.max(1);
                        self.brightness = self.state.brightness as f32;
                    }
                    if let Some(k) = pilot.temp.filter(|k| *k > 0) {
                        self.state.kelvin = k;
                        self.kelvin = k.clamp(2200, 6500) as f32;
                        self.state.mode = "kelvin".to_string();
                    } else if let Some((r, g, b)) = pilot.rgb() {
                        // Adopting the readback of our own pick would drift the
                        // hex, since the RGB/white split is lossy in brightness.
                        if !pilot.matches_rgb(self.state.rgb) {
                            self.state.rgb = [r, g, b];
                            self.state.hex = rgb_to_hex(r, g, b);
                        }
                        self.state.mode = "color".to_string();
                    }
                    if let Some(sid) = pilot.scene_id {
                        if sid != 0 {
                            self.state.scene_id = sid;
                            self.state.mode = "scene".to_string();
                        }
                    }
                    let _ = save_state(&self.state);
                }
                Event::Offline(_) => {
                    self.link = Link::Offline;
                    self.rssi = None;
                }
            }
        }
    }

    fn drain_palette(&mut self) {
        let done = match self.palette_rx.as_ref() {
            Some(rx) => match rx.try_recv() {
                Ok(result) => Some(result),
                Err(_) => None,
            },
            None => None,
        };

        if let Some(result) = done {
            self.palette_rx = None;
            self.extracting = false;
            match result {
                Ok(p) => {
                    self.palette_error = None;
                    self.palette = Some(p);
                }
                Err(e) => {
                    self.palette = None;
                    self.palette_error = Some(e);
                }
            }
        }
    }

    fn start_extraction(&mut self, ctx: &egui::Context, path: PathBuf) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        self.palette_rx = Some(rx);
        self.extracting = true;
        self.palette_error = None;

        thread::spawn(move || {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            let result = extract_palette(&path, 8).map(|colors| DroppedPalette { name, colors });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let picked = dropped
            .iter()
            .filter_map(|f| f.path.clone())
            .find(|p| is_supported_image(p));

        match picked {
            Some(path) => self.start_extraction(ctx, path),
            None => {
                self.palette = None;
                self.palette_error = Some("Not an image file".to_string());
            }
        }
    }

    /// Popover height depends on what is expanded; resize the X11 window to fit.
    fn sync_height(&mut self, ctx: &egui::Context) {
        let mut wanted = BASE_H;
        if self.show_picker {
            wanted += PICKER_H;
        }
        if self.palette.is_some() || self.palette_error.is_some() || self.extracting {
            wanted += PALETTE_ROW_H;
        }
        if (wanted - self.requested_height).abs() > 0.5 {
            if self.grows_upward {
                let bottom = self.origin.y + self.requested_height;
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(
                    self.origin.x,
                    (bottom - wanted).max(26.0),
                )));
            }
            self.requested_height = wanted;
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(WIN_W, wanted)));
        }
    }
}

// ---------------------------------------------------------------------------
// Painted header controls (no emoji fonts — no tofu boxes)
// ---------------------------------------------------------------------------
fn pin_button(ui: &mut egui::Ui, pinned: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(46.0, 20.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let color = if pinned {
            theme::ACCENT_BLUE
        } else if response.hovered() {
            theme::TEXT_SECONDARY
        } else {
            theme::TEXT_MUTED
        };
        if response.hovered() || pinned {
            painter.rect_filled(rect, CornerRadius::same(4), theme::INPUT_BG);
        }

        // Push-pin: head, stem, point.
        let head = Pos2::new(rect.left() + 10.0, rect.center().y - 3.0);
        painter.circle_filled(head, 3.5_f32, color);
        painter.line_segment(
            [head, Pos2::new(head.x, head.y + 7.0)],
            Stroke::new(1.6_f32, color),
        );

        painter.text(
            Pos2::new(rect.left() + 18.0, rect.center().y),
            Align2::LEFT_CENTER,
            if pinned { "Pinned" } else { "Pin" },
            FontId::proportional(9.5),
            color,
        );
    }
    response
}

/// Circular "+" swatch that expands the inline color picker.
fn custom_color_dot(ui: &mut egui::Ui, current: Color32, open: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let c = rect.center();
        painter.circle_filled(c, 10.0_f32, current);
        painter.circle_stroke(
            c,
            10.0_f32,
            Stroke::new(
                if open { 2.0_f32 } else { 1.0_f32 },
                if open {
                    Color32::WHITE
                } else {
                    theme::TEXT_MUTED
                },
            ),
        );
        let arm = 4.0;
        let ink = if current.r() as u32 + current.g() as u32 + current.b() as u32 > 380 {
            Color32::from_black_alpha(180)
        } else {
            Color32::WHITE
        };
        let stroke = Stroke::new(1.8_f32, ink);
        painter.line_segment([Pos2::new(c.x - arm, c.y), Pos2::new(c.x + arm, c.y)], stroke);
        painter.line_segment([Pos2::new(c.x, c.y - arm), Pos2::new(c.x, c.y + arm)], stroke);
    }
    response.on_hover_text("Custom color")
}

impl eframe::App for PopoverApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let elapsed = self.opened_at.elapsed().as_millis();

        // Grab focus during the initial X11 mapping window.
        if elapsed < 80 {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }

        let is_focused = ctx.input(|i| i.viewport().focused);
        if is_focused == Some(true) {
            self.has_gained_focus = true;
        }

        // Click-away dismissal (skipped while pinned or mid-drag).
        if !self.is_pinned && elapsed > 150 && self.has_gained_focus && is_focused == Some(false) {
            let busy = ctx.input(|i| i.pointer.primary_down());
            if !busy {
                self.close(ctx);
                return;
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close(ctx);
            return;
        }

        self.drain_worker();
        self.drain_palette();
        self.handle_dropped_files(ctx);

        let hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let glow = self.glow();

        let frame = egui::Frame::new()
            .fill(theme::BG_DARK)
            .stroke(Stroke::new(1.0_f32, theme::CARD_BORDER))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(12, 10));

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 7.0);

            // ---------------------------------------------------------------
            // Header: bulb, title, live status, pin & close
            // ---------------------------------------------------------------
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::hover());
                theme::bulb_icon(ui.painter(), icon.center(), 7.0, self.state.power, glow);

                ui.label(
                    egui::RichText::new("WiZ Light")
                        .color(theme::TEXT_PRIMARY)
                        .strong()
                        .size(12.0),
                );

                let (dot_color, status) = match self.link {
                    Link::Online => {
                        let detail = match self.rssi {
                            Some(r) => format!("{r} dBm"),
                            None => format!("{}ms", self.latency_ms),
                        };
                        (
                            theme::ACCENT_GREEN,
                            format!("{} ({detail})", self.state.ip),
                        )
                    }
                    Link::Pinging => (
                        theme::ACCENT_AMBER,
                        format!("Pinging {}...", self.state.ip),
                    ),
                    Link::Offline => (theme::ACCENT_RED, "Offline".to_string()),
                };
                theme::status_dot(ui, dot_color);
                ui.label(
                    egui::RichText::new(status)
                        .color(dot_color)
                        .size(8.5)
                        .family(egui::FontFamily::Monospace),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::window_button(ui, theme::WinButton::Close).clicked() {
                        self.close(ctx);
                    }
                    if pin_button(ui, self.is_pinned).clicked() {
                        self.is_pinned = !self.is_pinned;
                    }
                });
            });

            // ---------------------------------------------------------------
            // Power banner
            // ---------------------------------------------------------------
            if theme::power_banner(ui, self.state.power, self.link != Link::Offline).clicked() {
                if self.link == Link::Offline {
                    // Unreachable: retry the connection instead of blind-toggling.
                    self.link = Link::Pinging;
                    self.worker.send(Cmd::Ping);
                } else {
                    self.state.power = !self.state.power;
                    self.worker.send(Cmd::Power(self.state.power));
                    self.worker.send(Cmd::Ping);
                    let _ = save_state(&self.state);
                }
            }

            // ---------------------------------------------------------------
            // Brightness: slider + quick chips
            // ---------------------------------------------------------------
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Brightness")
                            .color(theme::TEXT_PRIMARY)
                            .strong()
                            .size(10.5),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let pct = (self.brightness * 100.0 / 255.0).round() as i32;
                        ui.label(
                            egui::RichText::new(format!("{pct}%"))
                                .color(theme::ACCENT_BLUE)
                                .strong()
                                .size(10.5)
                                .family(egui::FontFamily::Monospace),
                        );
                    });
                });

                let out = theme::track_slider(ui, &mut self.brightness, 1.0, 255.0, theme::ACCENT_BLUE);
                if out.changed {
                    self.state.brightness = self.brightness.round().clamp(1.0, 255.0) as u8;
                    self.state.power = true;
                    self.worker.send(Cmd::Brightness(self.state.brightness));
                }
                if out.released {
                    let _ = save_state(&self.state);
                }

                if let Some(val) = theme::chip_row(ui, QUICK_BRIGHTNESS, |v| self.state.brightness == v, 18.0)
                {
                    self.brightness = val as f32;
                    self.state.brightness = val;
                    self.state.power = true;
                    self.worker.send(Cmd::Brightness(val));
                    let _ = save_state(&self.state);
                }
            });

            // ---------------------------------------------------------------
            // White presets: Kelvin gradient slider + chips
            // ---------------------------------------------------------------
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::section(ui, "WHITE PRESETS");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{}K", self.kelvin.round() as u16))
                                .color(theme::TEXT_SECONDARY)
                                .size(10.0)
                                .family(egui::FontFamily::Monospace),
                        );
                    });
                });

                let out = theme::gradient_slider(ui, &mut self.kelvin, 2200.0, 6500.0, |t| {
                    let (r, g, b) = kelvin_to_rgb((2200.0 + t * 4300.0) as u16);
                    Color32::from_rgb(r, g, b)
                });
                if out.changed {
                    self.state.kelvin = self.kelvin.round() as u16;
                    self.state.mode = "kelvin".to_string();
                    self.state.power = true;
                    self.worker.send(Cmd::Kelvin(self.state.kelvin));
                }
                if out.released {
                    let _ = save_state(&self.state);
                }

                let active_k = if self.state.mode == "kelvin" {
                    self.state.kelvin
                } else {
                    0
                };
                if let Some(k) = theme::chip_row(ui, QUICK_KELVIN, |v| v == active_k, 20.0) {
                    self.kelvin = k as f32;
                    self.state.kelvin = k;
                    self.state.mode = "kelvin".to_string();
                    self.state.power = true;
                    self.worker.send(Cmd::Kelvin(k));
                    let _ = save_state(&self.state);
                }
            });

            // ---------------------------------------------------------------
            // Quick scenes
            // ---------------------------------------------------------------
            theme::card(ui, |ui| {
                theme::section(ui, "QUICK SCENES");
                let active_scene = if self.state.mode == "scene" {
                    self.state.scene_id
                } else {
                    0
                };
                if let Some(sid) = theme::chip_row(ui, QUICK_SCENES, |v| v == active_scene, 20.0) {
                    self.state.scene_id = sid;
                    self.state.mode = "scene".to_string();
                    self.state.power = true;
                    self.worker.send(Cmd::Scene(sid));
                    let _ = save_state(&self.state);
                }
            });

            // ---------------------------------------------------------------
            // Quick colors — or the palette extracted from a dropped image
            // ---------------------------------------------------------------
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.palette.is_some() {
                        theme::section(ui, "IMAGE PALETTE");
                    } else {
                        theme::section(ui, "QUICK COLORS");
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.palette.is_some() && ui.small_button("Reset").clicked() {
                            self.palette = None;
                            self.palette_error = None;
                        }
                    });
                });

                if self.extracting {
                    ui.label(
                        egui::RichText::new("Extracting colors...")
                            .color(theme::TEXT_MUTED)
                            .size(9.0),
                    );
                } else if let Some(err) = &self.palette_error {
                    ui.label(
                        egui::RichText::new(err.clone())
                            .color(theme::ACCENT_RED)
                            .size(9.0),
                    );
                } else if let Some(p) = &self.palette {
                    let shown = if p.name.chars().count() > 36 {
                        let head: String = p.name.chars().take(33).collect();
                        format!("{head}...")
                    } else {
                        p.name.clone()
                    };
                    ui.label(
                        egui::RichText::new(shown)
                            .color(theme::TEXT_MUTED)
                            .size(9.0),
                    )
                    .on_hover_text(p.name.clone());
                }

                let entries: Vec<(Color32, bool, String)> = match &self.palette {
                    Some(p) => p
                        .colors
                        .iter()
                        .map(|c| {
                            (
                                Color32::from_rgb(c.r, c.g, c.b),
                                self.state.rgb == [c.r, c.g, c.b],
                                format!("{} - {:.1}%", c.hex().to_uppercase(), c.percentage),
                            )
                        })
                        .collect(),
                    None => self
                        .state
                        .recent_colors
                        .iter()
                        .take(8)
                        .filter_map(|hex| parse_color(hex).ok())
                        .map(|(r, g, b)| {
                            (
                                Color32::from_rgb(r, g, b),
                                self.state.rgb == [r, g, b],
                                rgb_to_hex(r, g, b).to_uppercase(),
                            )
                        })
                        .collect(),
                };

                let mut picked: Option<Color32> = None;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (color, active, tip) in &entries {
                        let is_color_mode = self.state.mode == "color";
                        if theme::color_dot(ui, *color, 9.0, *active && is_color_mode)
                            .on_hover_text(tip.clone())
                            .clicked()
                        {
                            picked = Some(*color);
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if custom_color_dot(ui, self.custom_color, self.show_picker).clicked() {
                            self.show_picker = !self.show_picker;
                        }
                    });
                });

                if let Some(c) = picked {
                    self.apply_rgb(c.r(), c.g(), c.b());
                }

                if self.show_picker {
                    ui.add_space(4.0);
                    let mut chosen = self.custom_color;
                    if egui::color_picker::color_picker_color32(
                        ui,
                        &mut chosen,
                        egui::color_picker::Alpha::Opaque,
                    ) {
                        self.apply_rgb(chosen.r(), chosen.g(), chosen.b());
                    }
                }
            });

            // ---------------------------------------------------------------
            // Footer: open the full studio in a fresh process
            // ---------------------------------------------------------------
            if theme::wide_button(ui, "Open Full Studio...", 26.0).clicked() {
                let _ = save_state(&self.state);
                if let Ok(exe) = std::env::current_exe() {
                    let _ = Command::new(exe)
                        .arg("studio")
                        .arg("--ip")
                        .arg(&self.state.ip)
                        .spawn();
                }
                self.close(ctx);
            }
        });

        // Drop hint overlay.
        if hovering_files {
            let screen = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_hint"),
            ));
            painter.rect_filled(screen, CornerRadius::same(10), Color32::from_black_alpha(200));
            painter.rect_stroke(
                screen.shrink(3.0),
                CornerRadius::same(10),
                Stroke::new(2.0_f32, theme::ACCENT_BLUE),
                egui::StrokeKind::Inside,
            );
            painter.text(
                screen.center(),
                Align2::CENTER_CENTER,
                "Drop an image to extract its palette",
                FontId::proportional(13.0),
                theme::TEXT_PRIMARY,
            );
        }

        self.sync_height(ctx);

        // Keep the status line honest while the first ping is still in flight.
        if self.link == Link::Pinging {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}

// ---------------------------------------------------------------------------
// Launcher
// ---------------------------------------------------------------------------
pub fn run_widget(target_ip: Option<String>) -> Result<(), eframe::Error> {
    let pid_guard = match check_single_instance() {
        Some(g) => g,
        None => return Ok(()), // Toggled closed or debounced.
    };

    let (mx, my) = get_mouse_position();
    let pos_x = (mx - WIN_W / 2.0).clamp(10.0, 1920.0 - WIN_W - 10.0);
    // Launched from a bottom panel the popover hangs above the cursor, so any
    // later growth has to push its top edge up rather than its bottom edge down.
    let grows_upward = my >= 60.0;
    let pos_y = if grows_upward {
        (my - BASE_H - 10.0).max(26.0)
    } else {
        26.0
    };
    let origin = Pos2::new(pos_x, pos_y);

    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("wizctl - Quick Control")
            .with_inner_size(Vec2::new(WIN_W, BASE_H))
            .with_position(Pos2::new(pos_x, pos_y))
            .with_resizable(false)
            .with_decorations(false)
            .with_always_on_top()
            .with_transparent(true)
            .with_taskbar(false)
            .with_window_type(X11WindowType::Utility),
        ..Default::default()
    };

    // EWMH property injection: no taskbar tab, no pager slot, stays above.
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(40));
        let _ = Command::new("bash")
            .arg("-c")
            .arg("WIN_ID=$(xdotool search --name 'wizctl - Quick Control' 2>/dev/null | tail -1); \
                  if [ -n \"$WIN_ID\" ]; then \
                      xprop -id \"$WIN_ID\" -f _NET_WM_WINDOW_TYPE 32a -set _NET_WM_WINDOW_TYPE '_NET_WM_WINDOW_TYPE_UTILITY'; \
                      xprop -id \"$WIN_ID\" -f _NET_WM_STATE 32a -set _NET_WM_STATE '_NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER, _NET_WM_STATE_ABOVE'; \
                      xdotool windowactivate \"$WIN_ID\" 2>/dev/null || true; \
                  fi")
            .status();
    });

    eframe::run_native(
        "wizctl - Quick Control",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(PopoverApp::new(
                cc,
                target_ip,
                Some(pid_guard),
                origin,
                grows_upward,
            )))
        }),
    )
}
