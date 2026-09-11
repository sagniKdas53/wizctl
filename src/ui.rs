use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2, ViewportBuilder, ViewportCommand, X11WindowType};

use crate::bulb::{get_pilot, set_brightness, set_power, set_rgb, set_scene, set_temperature, PilotResult};
use crate::colors::{kelvin_to_rgb, parse_color, rgb_to_hex};
use crate::state::{load_state, save_state, State};

// ---------------------------------------------------------------------------
// Theme Palette (Matching XFCE charcoal and dark slate)
// ---------------------------------------------------------------------------
pub const COLOR_BG: Color32 = Color32::from_rgb(48, 49, 51);         // #303133 (XFCE dark charcoal)
pub const COLOR_BORDER: Color32 = Color32::from_rgb(32, 33, 35);     // #202123
pub const COLOR_ACCENT_BLUE: Color32 = Color32::from_rgb(17, 124, 221);// #117cdd (Solid highlight)
pub const COLOR_TRACK_BLUE: Color32 = Color32::from_rgb(24, 115, 204); // #1873cc (Active slider/toggle)
pub const COLOR_TRACK_BG: Color32 = Color32::from_rgb(60, 61, 63);   // #3c3d3f (Trough)
pub const COLOR_SWITCH_KNOB: Color32 = Color32::from_rgb(58, 60, 62);// #3a3c3e
pub const COLOR_SWITCH_OFF: Color32 = Color32::from_rgb(52, 53, 55);
pub const COLOR_TEXT_PRIMARY: Color32 = Color32::from_rgb(235, 235, 235);
pub const COLOR_TEXT_MUTED: Color32 = Color32::from_rgb(160, 162, 165);

const QUICK_SCENES: &[(u32, &str)] = &[
    (6, "Cozy"),
    (11, "Warm White"),
    (12, "Daylight"),
    (3, "Sunset"),
    (14, "Night Light"),
    (4, "Party"),
];

// ---------------------------------------------------------------------------
// Pillar 4: Single-Instance PID Guard & Debounce Latch
// ---------------------------------------------------------------------------
pub struct PidGuard;
impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file("/tmp/wizctl_gui.pid");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = fs::write("/tmp/wizctl_closed_stamp", format!("{now}"));
    }
}

pub fn check_single_instance() -> Option<PidGuard> {
    let pid_file = Path::new("/tmp/wizctl_gui.pid");
    if pid_file.exists() {
        if let Ok(content) = fs::read_to_string(pid_file) {
            if let Ok(pid) = content.trim().parse::<i32>() {
                if Path::new(&format!("/proc/{pid}")).exists() {
                    let _ = Command::new("kill").arg(format!("{pid}")).status();
                    let _ = fs::remove_file(pid_file);
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let _ = fs::write("/tmp/wizctl_closed_stamp", format!("{now}"));
                    return None; // Toggled off!
                }
            }
        }
    }

    let stamp_file = Path::new("/tmp/wizctl_closed_stamp");
    if stamp_file.exists() {
        if let Ok(content) = fs::read_to_string(stamp_file) {
            if let Ok(last_closed) = content.trim().parse::<u128>() {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                if now.saturating_sub(last_closed) < 250 {
                    let _ = fs::remove_file(stamp_file);
                    return None; // Stay closed (debounce latch)
                }
            }
        }
    }

    let my_pid = std::process::id();
    let _ = fs::write(pid_file, format!("{my_pid}"));
    Some(PidGuard)
}

// ---------------------------------------------------------------------------
// Pillar 2: Dynamic Cursor-Anchored Placement
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
// Worker Messages & Asynchronous Backend Client
// ---------------------------------------------------------------------------
enum WorkerCmd {
    SetPower(bool),
    SetBrightness(u8),
    SetKelvin(u16),
    SetRgb(u8, u8, u8),
    SetScene(u32),
}

enum WorkerEvent {
    BulbOnline(PilotResult),
    BulbOffline,
}

struct AsyncBulbWorker {
    tx: Sender<WorkerCmd>,
    rx: Receiver<WorkerEvent>,
}

impl AsyncBulbWorker {
    fn new(ip: String) -> Self {
        let (cmd_tx, cmd_rx) = channel::<WorkerCmd>();
        let (event_tx, event_rx) = channel::<WorkerEvent>();

        thread::spawn(move || {
            // Initial probe
            if let Ok(pilot) = get_pilot(&ip) {
                let _ = event_tx.send(WorkerEvent::BulbOnline(pilot));
            } else {
                let _ = event_tx.send(WorkerEvent::BulbOffline);
            }

            // Command loop with debouncing
            while let Ok(cmd) = cmd_rx.recv() {
                // Drain rapid redundant commands
                let mut latest_cmd = cmd;
                while let Ok(next) = cmd_rx.try_recv() {
                    match (&latest_cmd, &next) {
                        (WorkerCmd::SetBrightness(_), WorkerCmd::SetBrightness(_)) => {
                            latest_cmd = next;
                        }
                        (WorkerCmd::SetKelvin(_), WorkerCmd::SetKelvin(_)) => {
                            latest_cmd = next;
                        }
                        (WorkerCmd::SetRgb(_, _, _), WorkerCmd::SetRgb(_, _, _)) => {
                            latest_cmd = next;
                        }
                        _ => {
                            execute_cmd(&ip, &latest_cmd);
                            latest_cmd = next;
                        }
                    }
                }

                execute_cmd(&ip, &latest_cmd);
            }
        });

        Self { tx: cmd_tx, rx: event_rx }
    }

    fn send(&self, cmd: WorkerCmd) {
        let _ = self.tx.send(cmd);
    }

    fn try_recv(&self) -> Option<WorkerEvent> {
        self.rx.try_recv().ok()
    }
}

fn execute_cmd(ip: &str, cmd: &WorkerCmd) {
    match cmd {
        WorkerCmd::SetPower(st) => {
            let _ = set_power(ip, *st);
        }
        WorkerCmd::SetBrightness(b) => {
            let _ = set_brightness(ip, *b);
        }
        WorkerCmd::SetKelvin(k) => {
            let _ = set_temperature(ip, *k);
        }
        WorkerCmd::SetRgb(r, g, b) => {
            let _ = set_rgb(ip, *r, *g, *b);
        }
        WorkerCmd::SetScene(sid) => {
            let _ = set_scene(ip, *sid);
        }
    }
}

// ---------------------------------------------------------------------------
// Vector Painter Helper Widgets
// ---------------------------------------------------------------------------
pub fn draw_pill_toggle(ui: &mut egui::Ui, state: &mut bool) -> bool {
    let desired_size = Vec2::new(42.0_f32, 22.0_f32);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::click());
    let mut changed = false;

    if response.clicked() {
        *state = !*state;
        response.mark_changed();
        changed = true;
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let bg_fill = if *state { COLOR_TRACK_BLUE } else { COLOR_SWITCH_OFF };
        painter.rect_filled(rect, CornerRadius::same(11), bg_fill);

        let knob_radius = 8.5_f32;
        let knob_x = if *state {
            rect.right() - knob_radius - 2.5_f32
        } else {
            rect.left() + knob_radius + 2.5_f32
        };
        let knob_center = Pos2::new(knob_x, rect.center().y);
        painter.circle_filled(knob_center, knob_radius, COLOR_SWITCH_KNOB);
        painter.circle_stroke(
            knob_center,
            knob_radius,
            Stroke::new(1.0_f32, Color32::from_rgb(38, 40, 42)),
        );
    }
    changed
}

pub fn draw_lightbulb_icon(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    is_on: bool,
    glow_color: Color32,
) {
    if is_on {
        // Soft outer glow
        painter.circle_filled(center, radius * 1.5_f32, glow_color.gamma_multiply(0.20));
        painter.circle_filled(center, radius * 1.2_f32, glow_color.gamma_multiply(0.40));
        // Glass
        painter.circle_filled(center, radius, glow_color);
        painter.circle_stroke(center, radius, Stroke::new(1.2_f32, Color32::WHITE.gamma_multiply(0.8)));
    } else {
        painter.circle_filled(center, radius, Color32::from_rgb(55, 56, 58));
        painter.circle_stroke(center, radius, Stroke::new(1.0_f32, Color32::from_rgb(70, 72, 75)));
    }

    // Screw base
    let base_rect = Rect::from_min_max(
        Pos2::new(center.x - radius * 0.45_f32, center.y + radius * 0.7_f32),
        Pos2::new(center.x + radius * 0.45_f32, center.y + radius * 1.2_f32),
    );
    painter.rect_filled(base_rect, CornerRadius::same(2), Color32::from_rgb(85, 87, 90));
}

// ---------------------------------------------------------------------------
// Popover App Struct
// ---------------------------------------------------------------------------
pub struct PopoverApp {
    opened_at: Instant,
    has_gained_focus: bool,
    state: State,
    brightness_pct: u8,
    is_online: bool,
    custom_color: [u8; 3],
    worker: AsyncBulbWorker,
    _pid_guard: Option<PidGuard>,
}

impl PopoverApp {
    pub fn new(target_ip: Option<String>, pid_guard: Option<PidGuard>) -> Self {
        let mut state = load_state();
        if let Some(ip) = target_ip {
            state.ip = ip;
        }

        let brightness_pct = ((state.brightness as f64 * 100.0 / 255.0).round() as u8).clamp(10, 100);
        let custom_color = state.rgb;
        let worker = AsyncBulbWorker::new(state.ip.clone());

        Self {
            opened_at: Instant::now(),
            has_gained_focus: false,
            state,
            brightness_pct,
            is_online: true,
            custom_color,
            worker,
            _pid_guard: pid_guard,
        }
    }
}

impl eframe::App for PopoverApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let elapsed = self.opened_at.elapsed().as_millis();

        // 1. Explicitly request focus during initial frame window
        if elapsed < 80 {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }

        // 2. Latch focus acquisition
        let is_focused = ctx.input(|i| i.viewport().focused);
        if is_focused == Some(true) {
            self.has_gained_focus = true;
        }

        // 3. Auto-close when user clicks away / navigates away
        if elapsed > 150 && self.has_gained_focus && is_focused == Some(false) {
            let mouse_down = ctx.input(|i| i.pointer.primary_down());
            if !mouse_down {
                let _ = save_state(&self.state);
                ctx.send_viewport_cmd(ViewportCommand::Close);
                return;
            }
        }

        // 4. Escape key dismissal
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            let _ = save_state(&self.state);
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        // Check for worker events
        while let Some(evt) = self.worker.try_recv() {
            match evt {
                WorkerEvent::BulbOnline(pilot) => {
                    self.is_online = true;
                    if let Some(st) = pilot.state {
                        self.state.power = st;
                    }
                    if let Some(dim) = pilot.dimming {
                        self.brightness_pct = dim.clamp(10, 100);
                        self.state.brightness = ((dim as f64 * 255.0 / 100.0).round() as u8).clamp(1, 255);
                    }
                    if let Some(k) = pilot.temp {
                        self.state.kelvin = k;
                        self.state.mode = "kelvin".to_string();
                    } else if let Some((r, g, b)) = pilot.rgb() {
                        self.state.rgb = [r, g, b];
                        self.state.hex = rgb_to_hex(r, g, b);
                        self.state.mode = "color".to_string();
                    }
                    if let Some(sid) = pilot.scene_id {
                        if sid != 0 {
                            self.state.scene_id = sid;
                            self.state.mode = "scene".to_string();
                        }
                    }
                }
                WorkerEvent::BulbOffline => {
                    self.is_online = false;
                }
            }
        }

        // Determine current glow color
        let glow_color = if self.state.mode == "kelvin" {
            let (r, g, b) = kelvin_to_rgb(self.state.kelvin);
            Color32::from_rgb(r, g, b)
        } else if self.state.mode == "scene" {
            Color32::from_rgb(255, 180, 50)
        } else {
            Color32::from_rgb(self.state.rgb[0], self.state.rgb[1], self.state.rgb[2])
        };

        // Frameless panel styling
        let frame = egui::Frame::new()
            .fill(COLOR_BG)
            .stroke(Stroke::new(1.0_f32, COLOR_BORDER))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::same(14));

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0_f32, 10.0_f32);

            // -------------------------------------------------------------
            // 1. Header: Lightbulb icon + IP / Status + Power Pill Switch
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                let (bulb_rect, _) = ui.allocate_exact_size(Vec2::new(32.0_f32, 32.0_f32), Sense::hover());
                draw_lightbulb_icon(ui.painter(), bulb_rect.center(), 11.0_f32, self.state.power, glow_color);

                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("WiZ SMART BULB")
                            .color(COLOR_TEXT_PRIMARY)
                            .strong()
                            .size(13.5_f32),
                    );

                    let status_str = if !self.is_online {
                        format!("{} (Offline)", self.state.ip)
                    } else if self.state.power {
                        format!("{} • ON", self.state.ip)
                    } else {
                        format!("{} • OFF", self.state.ip)
                    };

                    let status_color = if !self.is_online {
                        Color32::from_rgb(230, 80, 80)
                    } else {
                        COLOR_TEXT_MUTED
                    };

                    ui.label(egui::RichText::new(status_str).color(status_color).size(10.5_f32));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if draw_pill_toggle(ui, &mut self.state.power) {
                        self.worker.send(WorkerCmd::SetPower(self.state.power));
                        let _ = save_state(&self.state);
                    }
                });
            });

            ui.add(egui::Separator::default().spacing(4.0_f32));

            // -------------------------------------------------------------
            // 2. Brightness Slider
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🔆").size(14.0_f32));
                ui.label(egui::RichText::new("Brightness").color(COLOR_TEXT_PRIMARY).size(12.0_f32));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{}%", self.brightness_pct))
                            .color(COLOR_TEXT_MUTED)
                            .size(11.5_f32),
                    );
                });
            });

            let b_slider = egui::Slider::new(&mut self.brightness_pct, 10..=100)
                .show_value(false)
                .trailing_fill(true);
            let b_resp = ui.add(b_slider);
            if b_resp.changed() {
                let val_255 = ((self.brightness_pct as f64 * 255.0 / 100.0).round() as u8).clamp(1, 255);
                self.state.brightness = val_255;
                if !self.state.power {
                    self.state.power = true;
                }
                self.worker.send(WorkerCmd::SetBrightness(val_255));
                let _ = save_state(&self.state);
            }

            // -------------------------------------------------------------
            // 3. Color Temperature (Kelvin) Slider
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🌡").size(14.0_f32));
                ui.label(egui::RichText::new("Color Temp").color(COLOR_TEXT_PRIMARY).size(12.0_f32));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{}K", self.state.kelvin))
                            .color(COLOR_TEXT_MUTED)
                            .size(11.5_f32),
                    );
                });
            });

            let k_slider = egui::Slider::new(&mut self.state.kelvin, 2700..=6500)
                .show_value(false)
                .trailing_fill(true);
            let k_resp = ui.add(k_slider);
            if k_resp.changed() {
                self.state.mode = "kelvin".to_string();
                if !self.state.power {
                    self.state.power = true;
                }
                self.worker.send(WorkerCmd::SetKelvin(self.state.kelvin));
                let _ = save_state(&self.state);
            }

            ui.add(egui::Separator::default().spacing(4.0_f32));

            // -------------------------------------------------------------
            // 4. Preset Scenes (Pill Buttons)
            // -------------------------------------------------------------
            ui.label(
                egui::RichText::new("PRESET SCENES")
                    .color(COLOR_TEXT_MUTED)
                    .size(10.0_f32)
                    .strong(),
            );

            egui::Grid::new("scenes_grid")
                .spacing(Vec2::new(6.0_f32, 6.0_f32))
                .show(ui, |ui| {
                    for (i, &(sid, sname)) in QUICK_SCENES.iter().enumerate() {
                        let is_selected = self.state.mode == "scene" && self.state.scene_id == sid;
                        let btn_fill = if is_selected { COLOR_ACCENT_BLUE } else { COLOR_TRACK_BG };
                        let text_color = if is_selected { Color32::WHITE } else { COLOR_TEXT_PRIMARY };

                        let btn = egui::Button::new(egui::RichText::new(sname).color(text_color).size(11.0_f32))
                            .fill(btn_fill)
                            .corner_radius(CornerRadius::same(6))
                            .min_size(Vec2::new(94.0_f32, 24.0_f32));

                        if ui.add(btn).clicked() {
                            self.state.scene_id = sid;
                            self.state.mode = "scene".to_string();
                            self.state.power = true;
                            self.worker.send(WorkerCmd::SetScene(sid));
                            let _ = save_state(&self.state);
                        }

                        if (i + 1) % 3 == 0 {
                            ui.end_row();
                        }
                    }
                });

            ui.add(egui::Separator::default().spacing(4.0_f32));

            // -------------------------------------------------------------
            // 5. Color Palette (Preset Swatches + Custom Picker)
            // -------------------------------------------------------------
            ui.label(
                egui::RichText::new("COLOR PALETTE")
                    .color(COLOR_TEXT_MUTED)
                    .size(10.0_f32)
                    .strong(),
            );

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(7.0_f32, 6.0_f32);

                for hex in &self.state.recent_colors {
                    if let Ok((r, g, b)) = parse_color(hex) {
                        let color = Color32::from_rgb(r, g, b);
                        let is_active = self.state.mode == "color" && self.state.rgb == [r, g, b];
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(20.0_f32, 20.0_f32), Sense::click());

                        if resp.clicked() {
                            self.state.rgb = [r, g, b];
                            self.state.hex = hex.clone();
                            self.state.mode = "color".to_string();
                            self.state.power = true;
                            self.custom_color = [r, g, b];
                            self.worker.send(WorkerCmd::SetRgb(r, g, b));
                            let _ = save_state(&self.state);
                        }

                        if ui.is_rect_visible(rect) {
                            let painter = ui.painter();
                            painter.circle_filled(rect.center(), 9.0_f32, color);
                            let stroke_color = if is_active {
                                Color32::WHITE
                            } else if resp.hovered() {
                                Color32::from_rgb(180, 180, 180)
                            } else {
                                Color32::from_rgb(30, 31, 33)
                            };
                            let stroke_w = if is_active { 2.0_f32 } else { 1.0_f32 };
                            painter.circle_stroke(rect.center(), 9.0_f32, Stroke::new(stroke_w, stroke_color));
                        }
                    }
                }

                // Custom Color Picker Button
                let mut srgba = egui::Color32::from_rgb(
                    self.custom_color[0],
                    self.custom_color[1],
                    self.custom_color[2],
                );
                if egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut srgba,
                    egui::color_picker::Alpha::Opaque,
                ).changed() {
                    let r = srgba.r();
                    let g = srgba.g();
                    let b = srgba.b();
                    self.custom_color = [r, g, b];
                    self.state.rgb = [r, g, b];
                    self.state.hex = rgb_to_hex(r, g, b);
                    self.state.mode = "color".to_string();
                    self.state.power = true;
                    self.worker.send(WorkerCmd::SetRgb(r, g, b));
                    let _ = save_state(&self.state);
                }
            });

            ui.add(egui::Separator::default().spacing(4.0_f32));

            // -------------------------------------------------------------
            // 6. Action Row: Toggle Button & Esc Hint
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                let toggle_text = if self.state.power { "⚡ Turn Off" } else { "⚡ Turn On" };
                let btn = egui::Button::new(
                    egui::RichText::new(toggle_text).color(COLOR_TEXT_PRIMARY).size(11.5_f32),
                )
                .fill(COLOR_TRACK_BG)
                .corner_radius(CornerRadius::same(6))
                .min_size(Vec2::new(100.0_f32, 24.0_f32));

                if ui.add(btn).clicked() {
                    self.state.power = !self.state.power;
                    self.worker.send(WorkerCmd::SetPower(self.state.power));
                    let _ = save_state(&self.state);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("Esc to close").color(COLOR_TEXT_MUTED).size(10.0_f32));
                });
            });
        });
    }
}

// ---------------------------------------------------------------------------
// run_gui: Launcher configuring NativeOptions and EWMH properties
// ---------------------------------------------------------------------------
pub fn run_gui(target_ip: Option<String>) -> Result<(), eframe::Error> {
    let pid_guard = match check_single_instance() {
        Some(g) => g,
        None => return Ok(()), // Toggled closed or debounced!
    };

    let (mx, my) = get_mouse_position();
    let win_w = 340.0_f32;
    let win_h = 445.0_f32;
    let pos_x = (mx - win_w / 2.0_f32).clamp(10.0_f32, 1920.0_f32 - win_w - 10.0_f32);
    let pos_y = if my < 60.0_f32 { 26.0_f32 } else { (my - win_h - 10.0_f32).max(26.0_f32) };

    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("wizctl - Quick Control")
            .with_inner_size(Vec2::new(win_w, win_h))
            .with_position(Pos2::new(pos_x, pos_y))
            .with_resizable(false)
            .with_decorations(false)        // Frameless popup
            .with_always_on_top()           // Floats above other windows
            .with_transparent(true)
            .with_taskbar(false)            // Informs winit to omit from taskbar
            .with_window_type(X11WindowType::Utility), // EWMH Utility type
        ..Default::default()
    };

    // Active EWMH Property Injection
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
        Box::new(move |_cc| Ok(Box::new(PopoverApp::new(target_ip, Some(pid_guard))))),
    )
}
