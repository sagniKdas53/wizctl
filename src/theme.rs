//! Shared dark-theme palette and hand-painted widget atoms.
//!
//! Every pixel is painted by `egui::Painter` rather than relying on system icon
//! fonts, so the UI never renders fallback tofu (`□`) boxes.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Response, Sense, Shape, Stroke,
    Ui, Vec2,
};
use eframe::epaint::{Vertex, WHITE_UV};

use crate::colors::{hsv_to_rgb, rgb_to_hsv};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------
pub const BG_DARK: Color32 = Color32::from_rgb(0x12, 0x12, 0x14);
pub const CARD_BG: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1f);
pub const CARD_BORDER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x32);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x25, 0x25, 0x2e);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf6);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xa1, 0xa1, 0xaa);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x71, 0x71, 0x7a);
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);
pub const ACCENT_BLUE_DIM: Color32 = Color32::from_rgb(0x25, 0x63, 0xeb);
pub const ACCENT_GREEN: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
pub const ACCENT_GREEN_DIM: Color32 = Color32::from_rgb(0x16, 0xa3, 0x4a);
pub const ACCENT_RED: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
pub const ACCENT_AMBER: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);
pub const INPUT_BG: Color32 = Color32::from_rgb(0x22, 0x22, 0x2a);

/// Install the shared dark visuals onto an `egui` context.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG_DARK;
    visuals.window_fill = BG_DARK;
    visuals.extreme_bg_color = INPUT_BG;
    visuals.faint_bg_color = CARD_BG;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.selection.bg_fill = ACCENT_BLUE;
    visuals.selection.stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.window_stroke = Stroke::new(1.0_f32, CARD_BORDER);
    visuals.widgets.noninteractive.bg_fill = CARD_BG;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, CARD_BORDER);
    visuals.widgets.inactive.bg_fill = INPUT_BG;
    visuals.widgets.inactive.weak_bg_fill = INPUT_BG;
    visuals.widgets.hovered.bg_fill = CARD_HOVER;
    visuals.widgets.hovered.weak_bg_fill = CARD_HOVER;
    visuals.widgets.active.bg_fill = ACCENT_BLUE;
    visuals.widgets.active.weak_bg_fill = ACCENT_BLUE;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(6.0, 6.0);
    style.spacing.button_padding = Vec2::new(6.0, 3.0);
    ctx.set_style(style);
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

/// A rounded card panel matching the Python widget's `CARD_BG` surfaces.
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD_BG)
        .stroke(Stroke::new(1.0_f32, CARD_BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(9, 7))
        .show(ui, add_contents)
        .inner
}

/// Small muted section caption ("QUICK SCENES", "White Presets", ...).
pub fn section(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .color(TEXT_MUTED)
            .size(9.5)
            .strong(),
    );
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

/// Compact pill button used for brightness / Kelvin / scene presets.
pub fn chip(ui: &mut Ui, label: &str, active: bool, min_size: Vec2) -> Response {
    let (fill, fg) = if active {
        (ACCENT_BLUE, Color32::WHITE)
    } else {
        (INPUT_BG, TEXT_SECONDARY)
    };
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(fg).size(10.0))
            .fill(fill)
            .corner_radius(CornerRadius::same(5))
            .stroke(Stroke::NONE)
            .min_size(min_size),
    )
}

/// Chips laid out edge-to-edge across the full available width.
pub fn chip_row<T: Copy>(
    ui: &mut Ui,
    items: &[(&str, T)],
    is_active: impl Fn(T) -> bool,
    height: f32,
) -> Option<T> {
    let mut picked = None;
    let spacing = 3.0;
    let total = ui.available_width();
    let count = items.len().max(1) as f32;
    let width = ((total - spacing * (count - 1.0)) / count).max(10.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = spacing;
        for &(label, value) in items {
            if chip(ui, label, is_active(value), Vec2::new(width, height)).clicked() {
                picked = Some(value);
            }
        }
    });
    picked
}

/// Full-width power banner ("BULB IS ON (Click to turn OFF)").
pub fn power_banner(ui: &mut Ui, is_on: bool, is_online: bool) -> Response {
    let height = 34.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let fill = match (is_on, hovered) {
            (true, false) => ACCENT_GREEN,
            (true, true) => ACCENT_GREEN_DIM,
            (false, false) => INPUT_BG,
            (false, true) => CARD_HOVER,
        };
        let fg = if is_on { Color32::WHITE } else { TEXT_SECONDARY };

        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(7), fill);

        let icon_center = Pos2::new(rect.left() + 26.0, rect.center().y);
        bulb_icon(painter, icon_center, 8.0, is_on, fg);

        let label = if !is_online {
            "BULB UNREACHABLE (Click to retry)"
        } else if is_on {
            "BULB IS ON (Click to turn OFF)"
        } else {
            "BULB IS OFF (Click to turn ON)"
        };
        painter.text(
            Pos2::new(rect.center().x + 12.0, rect.center().y),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(12.5),
            fg,
        );
    }
    response
}

/// Full-width secondary button (footer actions such as "Open Full Studio...").
pub fn wide_button(ui: &mut Ui, label: &str, height: f32) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if response.hovered() { CARD_HOVER } else { INPUT_BG };
        painter.rect_filled(rect, CornerRadius::same(6), fill);
        painter.rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(1.0_f32, CARD_BORDER),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.5),
            if response.hovered() {
                TEXT_PRIMARY
            } else {
                TEXT_SECONDARY
            },
        );
    }
    response
}

// ---------------------------------------------------------------------------
// Window chrome (both surfaces are frameless; we paint our own controls)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WinButton {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// Painted titlebar button. Glyphs are drawn as line art so no icon font is needed.
pub fn window_button(ui: &mut Ui, kind: WinButton) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(26.0, 22.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let hovered = response.hovered();
    let painter = ui.painter();
    if hovered {
        let fill = if kind == WinButton::Close {
            ACCENT_RED
        } else {
            CARD_HOVER
        };
        painter.rect_filled(rect, CornerRadius::same(4), fill);
    }
    let ink = if hovered { Color32::WHITE } else { TEXT_MUTED };
    let stroke = Stroke::new(1.4_f32, ink);
    let c = rect.center();

    match kind {
        WinButton::Minimize => {
            painter.line_segment([Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x + 5.0, c.y)], stroke);
        }
        WinButton::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(9.0)),
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        WinButton::Restore => {
            painter.rect_stroke(
                Rect::from_center_size(Pos2::new(c.x - 1.5, c.y + 1.5), Vec2::splat(8.0)),
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [Pos2::new(c.x - 1.0, c.y - 2.5), Pos2::new(c.x + 3.0, c.y - 2.5)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x + 3.0, c.y - 2.5), Pos2::new(c.x + 3.0, c.y + 1.5)],
                stroke,
            );
        }
        WinButton::Close => {
            let d = 4.5;
            painter.line_segment(
                [Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x + d, c.y - d), Pos2::new(c.x - d, c.y + d)],
                stroke,
            );
        }
    }
    response
}

/// Invisible drag handles along the window edges, since a frameless window gets
/// no resize border from the window manager.
pub fn resize_grips(ctx: &egui::Context) {
    use egui::{CursorIcon, Id, Order, ResizeDirection};

    let screen = ctx.screen_rect();
    let band = 6.0;
    let corner = 16.0;

    // Corners first so they win the overlap with the edge bands.
    let handles: [(&str, Rect, ResizeDirection, CursorIcon); 5] = [
        (
            "grip_se",
            Rect::from_min_max(screen.max - Vec2::splat(corner), screen.max),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeNwSe,
        ),
        (
            "grip_sw",
            Rect::from_min_max(
                Pos2::new(screen.left(), screen.bottom() - corner),
                Pos2::new(screen.left() + corner, screen.bottom()),
            ),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeNeSw,
        ),
        (
            "grip_s",
            Rect::from_min_max(
                Pos2::new(screen.left() + corner, screen.bottom() - band),
                Pos2::new(screen.right() - corner, screen.bottom()),
            ),
            ResizeDirection::South,
            CursorIcon::ResizeVertical,
        ),
        (
            "grip_e",
            Rect::from_min_max(
                Pos2::new(screen.right() - band, screen.top() + corner),
                Pos2::new(screen.right(), screen.bottom() - corner),
            ),
            ResizeDirection::East,
            CursorIcon::ResizeHorizontal,
        ),
        (
            "grip_w",
            Rect::from_min_max(
                Pos2::new(screen.left(), screen.top() + corner),
                Pos2::new(screen.left() + band, screen.bottom() - corner),
            ),
            ResizeDirection::West,
            CursorIcon::ResizeHorizontal,
        ),
    ];

    for (name, rect, direction, cursor) in handles {
        let id = Id::new(name);
        egui::Area::new(id)
            .order(Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(cursor);
                }
                if response.drag_started() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
            });
    }
}

// ---------------------------------------------------------------------------
// Indicators
// ---------------------------------------------------------------------------

/// Colored status dot (online / pinging / offline).
pub fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().circle_filled(rect.center(), 4.0, color);
    }
}

/// Vector light-bulb glyph — glass, glow halo and screw base.
pub fn bulb_icon(painter: &egui::Painter, center: Pos2, radius: f32, is_on: bool, glow: Color32) {
    if is_on {
        painter.circle_filled(center, radius * 1.55, glow.gamma_multiply(0.18));
        painter.circle_filled(center, radius * 1.25, glow.gamma_multiply(0.35));
        painter.circle_filled(center, radius, glow);
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(1.0_f32, Color32::WHITE.gamma_multiply(0.7)),
        );
    } else {
        painter.circle_filled(center, radius, Color32::from_rgb(55, 56, 58));
        painter.circle_stroke(center, radius, Stroke::new(1.0_f32, CARD_BORDER));
    }

    let base = Rect::from_min_max(
        Pos2::new(center.x - radius * 0.45, center.y + radius * 0.72),
        Pos2::new(center.x + radius * 0.45, center.y + radius * 1.25),
    );
    painter.rect_filled(base, CornerRadius::same(2), Color32::from_rgb(0x55, 0x57, 0x5a));
}

/// iOS-style pill switch. Returns `true` when the user flipped it.
pub fn pill_toggle(ui: &mut Ui, state: &mut bool) -> bool {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(42.0, 22.0), Sense::click());
    let mut changed = false;

    if response.clicked() {
        *state = !*state;
        response.mark_changed();
        changed = true;
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if *state { ACCENT_GREEN } else { INPUT_BG };
        painter.rect_filled(rect, CornerRadius::same(11), fill);

        let knob_r = 8.5;
        let knob_x = if *state {
            rect.right() - knob_r - 2.5
        } else {
            rect.left() + knob_r + 2.5
        };
        let knob = Pos2::new(knob_x, rect.center().y);
        painter.circle_filled(knob, knob_r, Color32::WHITE);
        painter.circle_stroke(knob, knob_r, Stroke::new(1.0_f32, Color32::from_black_alpha(60)));
    }
    changed
}

// ---------------------------------------------------------------------------
// Swatches
// ---------------------------------------------------------------------------

/// Rounded color swatch. `active` draws a white selection ring.
pub fn swatch(ui: &mut Ui, color: Color32, size: Vec2, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(5), color);
        let stroke = if active {
            Stroke::new(2.0_f32, Color32::WHITE)
        } else if response.hovered() {
            Stroke::new(1.5_f32, TEXT_SECONDARY)
        } else {
            Stroke::new(1.0_f32, Color32::from_black_alpha(90))
        };
        painter.rect_stroke(
            rect,
            CornerRadius::same(5),
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// Circular color dot used in the compact popover palette row.
pub fn color_dot(ui: &mut Ui, color: Color32, radius: f32, active: bool) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::splat(radius * 2.0 + 2.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.circle_filled(rect.center(), radius, color);
        let stroke = if active {
            Stroke::new(2.0_f32, Color32::WHITE)
        } else if response.hovered() {
            Stroke::new(1.5_f32, TEXT_SECONDARY)
        } else {
            Stroke::new(1.0_f32, Color32::from_black_alpha(90))
        };
        painter.circle_stroke(rect.center(), radius, stroke);
    }
    response
}

// ---------------------------------------------------------------------------
// Sliders
// ---------------------------------------------------------------------------

pub struct SliderOut {
    pub response: Response,
    pub changed: bool,
    pub released: bool,
}

fn slider_core(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    paint_track: impl FnOnce(&egui::Painter, Rect, f32),
) -> SliderOut {
    let height = 20.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        Sense::click_and_drag(),
    );

    let knob_r = 8.0;
    let usable = Rect::from_min_max(
        Pos2::new(rect.left() + knob_r, rect.top()),
        Pos2::new(rect.right() - knob_r, rect.bottom()),
    );

    let mut changed = false;
    if response.is_pointer_button_down_on() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let t = ((pos.x - usable.left()) / usable.width().max(1.0)).clamp(0.0, 1.0);
            let next = min + t * (max - min);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                changed = true;
            }
        }
    }

    let t = ((*value - min) / (max - min).max(f32::EPSILON)).clamp(0.0, 1.0);

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let track = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 6.0));
        paint_track(painter, track, t);

        let knob = Pos2::new(usable.left() + t * usable.width(), rect.center().y);
        painter.circle_filled(knob, knob_r, Color32::WHITE);
        painter.circle_stroke(knob, knob_r, Stroke::new(1.0_f32, Color32::from_black_alpha(80)));
    }

    SliderOut {
        released: response.drag_stopped() || response.clicked(),
        changed,
        response,
    }
}

/// Flat slider with an accent-filled leading track.
pub fn track_slider(ui: &mut Ui, value: &mut f32, min: f32, max: f32, fill: Color32) -> SliderOut {
    slider_core(ui, value, min, max, |painter, track, t| {
        painter.rect_filled(track, CornerRadius::same(3), INPUT_BG);
        let filled = Rect::from_min_max(
            track.min,
            Pos2::new(track.left() + t * track.width(), track.bottom()),
        );
        painter.rect_filled(filled, CornerRadius::same(3), fill);
    })
}

/// Slider whose trough is painted with a live gradient (used for Kelvin).
pub fn gradient_slider(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    color_at: impl Fn(f32) -> Color32,
) -> SliderOut {
    slider_core(ui, value, min, max, |painter, track, _t| {
        const STEPS: usize = 48;
        let step_w = track.width() / STEPS as f32;
        for i in 0..STEPS {
            let x0 = track.left() + i as f32 * step_w;
            let seg = Rect::from_min_max(
                Pos2::new(x0, track.top()),
                Pos2::new(x0 + step_w + 0.5, track.bottom()),
            );
            painter.rect_filled(seg, CornerRadius::ZERO, color_at(i as f32 / (STEPS - 1) as f32));
        }
    })
}

// ---------------------------------------------------------------------------
// HSV color wheel
// ---------------------------------------------------------------------------

pub struct WheelOut {
    pub changed: bool,
    pub released: bool,
}

/// Interactive hue/saturation disc with a live reticle.
///
/// Value (brightness) is intentionally fixed at 1.0 — the bulb's brightness is
/// driven by the dedicated brightness slider, matching the Python studio.
pub fn color_wheel(ui: &mut Ui, diameter: f32, rgb: &mut [u8; 3]) -> WheelOut {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::splat(diameter), Sense::click_and_drag());
    let center = rect.center();
    let radius = diameter / 2.0 - 2.0;

    let (mut hue, mut sat, _) = rgb_to_hsv(rgb[0], rgb[1], rgb[2]);
    let mut changed = false;

    if response.is_pointer_button_down_on() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let dx = pos.x - center.x;
            let dy = pos.y - center.y;
            let dist = (dx * dx + dy * dy).sqrt();
            hue = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            sat = (dist / radius).clamp(0.0, 1.0);
            let (r, g, b) = hsv_to_rgb(hue, sat, 1.0);
            if [r, g, b] != *rgb {
                *rgb = [r, g, b];
                changed = true;
            }
        }
    }

    if ui.is_rect_visible(rect) {
        const SEGMENTS: usize = 96;
        let mut mesh = Mesh::default();
        mesh.vertices.push(Vertex {
            pos: center,
            uv: WHITE_UV,
            color: Color32::WHITE,
        });
        for i in 0..=SEGMENTS {
            let angle = (i as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
            let (r, g, b) = hsv_to_rgb(angle.to_degrees(), 1.0, 1.0);
            mesh.vertices.push(Vertex {
                pos: Pos2::new(
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                ),
                uv: WHITE_UV,
                color: Color32::from_rgb(r, g, b),
            });
        }
        for i in 1..=SEGMENTS as u32 {
            mesh.indices.extend_from_slice(&[0, i, i + 1]);
        }

        let painter = ui.painter();
        painter.add(Shape::mesh(mesh));
        painter.circle_stroke(center, radius, Stroke::new(1.0_f32, CARD_BORDER));

        let angle = hue.to_radians();
        let reticle = Pos2::new(
            center.x + sat * radius * angle.cos(),
            center.y + sat * radius * angle.sin(),
        );
        painter.circle_filled(reticle, 7.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        painter.circle_stroke(reticle, 7.0, Stroke::new(2.0_f32, Color32::WHITE));
        painter.circle_stroke(reticle, 8.5, Stroke::new(1.0_f32, Color32::from_black_alpha(120)));
    }

    WheelOut {
        released: response.drag_stopped() || response.clicked(),
        changed,
    }
}
