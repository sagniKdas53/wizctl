# Reusable Smart Popover Widget Pattern & `wizctl` Rust/egui Rewrite Plan

This document details the architecture, window management mechanisms, and lifecycle patterns perfected in **`debugmic` (AudioToggle)**. It serves as an actionable blueprint to rewrite **`wizctl`** from Python/Tkinter into a high-performance native Rust application with an `egui`/`eframe` XFCE4 panel popover widget.

---

## 1. Core Architectural Philosophy

### 1.1 Ephemeral Process vs. Resident Daemon
| Attribute | Traditional Applet (Python / Electron / GTK) | **Smart Popover Widget (Rust + egui)** |
|---|---|---|
| **Close Action** | `window.hide()` or minimizes | **Clean OS Process Exit (`exit(0)`)** |
| **Idle Memory Footprint** | 80 MB – 500 MB continuously | **0 KB (0 bytes)** |
| **Open File Descriptors** | Held indefinitely (X11, sockets, locks) | **0 (Immediately purged by Linux kernel)** |
| **Re-launch Latency** | ~1.5s cold / ~150ms hidden unhide | **< 30ms cold start** (stripped native ELF binary) |
| **Crash Surface** | Long-lived memory leaks, zombie event loops | **Stateless execution, self-contained per invocation** |

### 1.2 The "True Popover" Contract
A panel popover widget must behave like a native dropdown menu:
1. **Zero Panel Clutter**: Never create a tab/button on the taskbar or an icon in the workspace pager.
2. **Auto-Dismiss on Click-Away**: Automatically close behind the user the moment they click on any other window or desktop space.
3. **Instant Toggle**: Clicking the launcher opens the widget; clicking the launcher again closes it.
4. **Keyboard Dismissal**: Pressing `Escape` closes the widget instantly.
5. **No Visual Glitches**: Zero fallback tofu (`□`) boxes, frameless floating window, pixel-accurate dark theme matching XFCE.

---

## 2. Technical Implementation: The Five Pillars

### Pillar 1: X11 Window Management (No Taskbar / Pager Tabs)

By default, window managers (like XFWM4) treat GUI windows as `_NET_WM_WINDOW_TYPE_NORMAL`, placing them on the taskbar and workspace pager. 

To eliminate taskbar tabs and pager representations, apply a two-tier configuration:

#### A. Winit / ViewportBuilder Configuration
```rust
let native_options = eframe::NativeOptions {
    viewport: ViewportBuilder::default()
        .with_title("App Control")
        .with_inner_size(Vec2::new(win_w, win_h))
        .with_position(Pos2::new(pos_x, pos_y))
        .with_resizable(false)
        .with_decorations(false)        // Frameless popup
        .with_always_on_top()           // Floats above other windows
        .with_transparent(true)
        .with_taskbar(false)            // Informs winit to omit from taskbar
        .with_window_type(eframe::egui::X11WindowType::Utility), // EWMH Utility type
    ..Default::default()
};
```

#### B. Active EWMH Property Injection
Winit's internal X11 mapping can be augmented by explicitly enforcing EWMH atoms via a lightweight background helper:
```rust
std::thread::spawn(|| {
    std::thread::sleep(std::time::Duration::from_millis(40));
    let _ = Command::new("bash")
        .arg("-c")
        .arg("WIN_ID=$(xdotool search --name 'App Control' 2>/dev/null | tail -1); \
              if [ -n \"$WIN_ID\" ]; then \
                  xprop -id \"$WIN_ID\" -f _NET_WM_WINDOW_TYPE 32a -set _NET_WM_WINDOW_TYPE '_NET_WM_WINDOW_TYPE_UTILITY'; \
                  xprop -id \"$WIN_ID\" -f _NET_WM_STATE 32a -set _NET_WM_STATE '_NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER, _NET_WM_STATE_ABOVE'; \
                  xdotool windowactivate \"$WIN_ID\" 2>/dev/null || true; \
              fi")
        .status();
});
```
- **`_NET_WM_STATE_SKIP_TASKBAR`**: Guarantees zero tabs on bottom or side panels.
- **`_NET_WM_STATE_SKIP_PAGER`**: Guarantees the popover never occupies space in workspace switchers.
- **`_NET_WM_STATE_ABOVE`**: Keeps the popover floating directly beneath the panel.

---

### Pillar 2: Dynamic Cursor-Anchored Placement
Instead of hardcoding screen coordinates, anchor the popover dynamically to the user's mouse position when clicking the panel icon:
```rust
fn get_mouse_position() -> (f32, f32) {
    if let Ok(out) = Command::new("xdotool").arg("getmouselocation").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        let mut x = 600.0;
        let mut y = 26.0;
        for part in s.split_whitespace() {
            if let Some(val) = part.strip_prefix("x:") {
                if let Ok(v) = val.parse::<f32>() { x = v; }
            } else if let Some(val) = part.strip_prefix("y:") {
                if let Ok(v) = val.parse::<f32>() { y = v; }
            }
        }
        return (x, y);
    }
    (600.0, 26.0)
}

// In run_gui():
let (mx, my) = get_mouse_position();
let win_w = 340.0;
let win_h = 350.0;
let pos_x = (mx - win_w / 2.0).clamp(10.0, 1920.0 - win_w - 10.0);
let pos_y = if my < 60.0 { 26.0 } else { (my - win_h - 10.0).max(26.0) };
```

---

### Pillar 3: Smart Click-Away Auto-Dismissal

In `egui`, track window focus transitions via `ctx.input(|i| i.viewport().focused)`:

```rust
pub struct PopoverApp {
    opened_at: Instant,
    has_gained_focus: bool,
    // ... app state
}

impl Default for PopoverApp {
    fn default() -> Self {
        Self {
            opened_at: Instant::now(),
            has_gained_focus: false,
        }
    }
}

impl eframe::App for PopoverApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let elapsed = self.opened_at.elapsed().as_millis();

        // 1. Explicitly request focus during the initial frame window
        if elapsed < 80 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // 2. Latch focus acquisition
        let is_focused = ctx.input(|i| i.viewport().focused);
        if is_focused == Some(true) {
            self.has_gained_focus = true;
        }

        // 3. Auto-close when user clicks away / navigates away
        // A 150ms startup grace period ensures initial X11 mapping doesn't falsely trigger closure
        if elapsed > 150 && self.has_gained_focus && is_focused == Some(false) {
            let mouse_down = ctx.input(|i| i.pointer.primary_down());
            // Do not dismiss if the user is actively dragging a slider or holding mouse down
            if !mouse_down {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }

        // 4. Escape key dismissal
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Render UI...
    }
}
```

---

### Pillar 4: Instant Single-Instance Toggling

To prevent double launches and support toggle-to-close behavior from the panel button:

```rust
struct PidGuard;
impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file("/tmp/app_gui.pid");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = fs::write("/tmp/app_closed_stamp", format!("{}", now));
    }
}

fn check_single_instance() -> Option<PidGuard> {
    let pid_file = Path::new("/tmp/app_gui.pid");
    if pid_file.exists() {
        if let Ok(content) = fs::read_to_string(pid_file) {
            if let Ok(pid) = content.trim().parse::<i32>() {
                if Path::new(&format!("/proc/{}", pid)).exists() {
                    let _ = Command::new("kill").arg(format!("{}", pid)).status();
                    let _ = fs::remove_file(pid_file);
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let _ = fs::write("/tmp/app_closed_stamp", format!("{}", now));
                    return None; // Toggled off!
                }
            }
        }
    }

    // Debounce latch: if the window closed < 250ms ago (e.g. from focus loss on clicking the panel icon),
    // do not immediately re-open — the user intended to toggle it closed.
    let stamp_file = Path::new("/tmp/app_closed_stamp");
    if stamp_file.exists() {
        if let Ok(content) = fs::read_to_string(stamp_file) {
            if let Ok(last_closed) = content.trim().parse::<u128>() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                if now.saturating_sub(last_closed) < 250 {
                    let _ = fs::remove_file(stamp_file);
                    return None; // Stay closed!
                }
            }
        }
    }

    let my_pid = std::process::id();
    let _ = fs::write(pid_file, format!("{}", my_pid));
    Some(PidGuard)
}
```

---

### Pillar 5: Pixel-Accurate XFCE Dark Theme & Vector Painter

Never rely on system fonts for UI icons (which cause tofu `□` boxes on missing glyphs). Draw icons directly with `egui::Painter`:

#### Theme Palette:
```rust
const COLOR_BG: Color32 = Color32::from_rgb(48, 49, 51);         // #303133 (XFCE dark charcoal)
const COLOR_BORDER: Color32 = Color32::from_rgb(32, 33, 35);     // #202123
const COLOR_ACCENT_BLUE: Color32 = Color32::from_rgb(17, 124, 221);// #117cdd (Solid row highlight)
const COLOR_TRACK_BLUE: Color32 = Color32::from_rgb(24, 115, 204); // #1873cc (Active slider/toggle fill)
const COLOR_TRACK_BG: Color32 = Color32::from_rgb(60, 61, 63);   // #3c3d3f (Trough)
const COLOR_SLIDER_KNOB: Color32 = Color32::from_rgb(72, 74, 76);// #484a4c (Dark slate button)
const COLOR_SWITCH_KNOB: Color32 = Color32::from_rgb(58, 60, 62);// #3a3c3e
const COLOR_SWITCH_OFF: Color32 = Color32::from_rgb(52, 53, 55);
const COLOR_TEXT_PRIMARY: Color32 = Color32::from_rgb(235, 235, 235);
const COLOR_TEXT_MUTED: Color32 = Color32::from_rgb(160, 162, 165);
```

#### Pill Toggle Switch:
```rust
fn draw_pill_toggle(ui: &mut egui::Ui, state: &mut bool) -> bool {
    let desired_size = Vec2::new(38.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    let mut changed = false;

    if response.clicked() {
        *state = !*state;
        response.mark_changed();
        changed = true;
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let bg_fill = if *state { COLOR_TRACK_BLUE } else { COLOR_SWITCH_OFF };
        painter.rect_filled(rect, CornerRadius::same(10), bg_fill);

        let knob_radius = 8.0;
        let knob_x = if *state { rect.right() - knob_radius - 2.0 } else { rect.left() + knob_radius + 2.0 };
        let knob_center = Pos2::new(knob_x, rect.center().y);
        painter.circle_filled(knob_center, knob_radius, COLOR_SWITCH_KNOB);
        painter.circle_stroke(knob_center, knob_radius, Stroke::new(1.0_f32, Color32::from_rgb(38, 40, 42)));
    }
    changed
}
```

---

## 3. The `wizctl` Rewrite Blueprint

`wizctl` is currently written in Python using Tkinter and `pywizlight` (PyInstaller binary ~30MB, cold start ~1.5s, idle RAM ~100MB).

### 3.1 Target Architecture in Rust
```
wizctl/
├── Cargo.toml           # eframe 0.31, serde, serde_json, tokio / std::net
├── Makefile             # build, install, test
├── src/
│   ├── main.rs          # CLI dispatcher, single-instance PID guard, run_gui
│   ├── ui.rs            # egui Popover: Power pill, Brightness slider, Kelvin slider, Color Wheel, Scenes
│   ├── bulb.rs          # WiZ UDP protocol (JSON-RPC over UDP port 38899)
│   ├── state.rs         # Local state cache (~/.config/wizctl/state.json)
│   ├── colors.rs        # Preset colors and Kelvin mappings
│   └── genmon.rs        # XFCE4 Genmon XML status provider
└── assets/              # Lightbulb panel icons (on, off, warm, cool)
```

### 3.2 WiZ UDP Protocol in Pure Rust (No Python / No pywizlight)
WiZ bulbs use simple JSON-RPC over UDP to port `38899`. No external cloud or heavy client libraries are required:

#### A. Set State (Power / Brightness / Kelvin / RGB)
```rust
use std::net::UdpSocket;
use serde_json::json;

pub fn send_pilot(ip: &str, params: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(std::time::Duration::from_millis(500)))?;

    let payload = json!({
        "method": "setPilot",
        "params": params
    });

    let msg = serde_json::to_vec(&payload)?;
    socket.send_to(&msg, format!("{}:38899", ip))?;
    Ok(())
}

// Power On/Off
pub fn set_power(ip: &str, state: bool) -> Result<(), Box<dyn std::error::Error>> {
    send_pilot(ip, json!({ "state": state }))
}

// Brightness (10 - 100)
pub fn set_brightness(ip: &str, dimming: u8) -> Result<(), Box<dyn std::error::Error>> {
    let dim = dimming.clamp(10, 100);
    send_pilot(ip, json!({ "dimming": dim }))
}

// White Temperature (2700K - 6500K)
pub fn set_temperature(ip: &str, kelvin: u16) -> Result<(), Box<dyn std::error::Error>> {
    let temp = kelvin.clamp(2700, 6500);
    send_pilot(ip, json!({ "temp": temp }))
}

// RGB Color
pub fn set_rgb(ip: &str, r: u8, g: u8, b: u8) -> Result<(), Box<dyn std::error::Error>> {
    send_pilot(ip, json!({ "r": r, "g": g, "b": b }))
}
```

#### B. Get State (Fast Query)
```rust
pub fn get_pilot(ip: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(std::time::Duration::from_millis(600)))?;

    let payload = json!({ "method": "getPilot" });
    socket.send_to(&serde_json::to_vec(&payload)?, format!("{}:38899", ip))?;

    let mut buf = [0u8; 1024];
    let (amt, _) = socket.recv_from(&mut buf)?;
    let res: serde_json::Value = serde_json::from_slice(&buf[..amt])?;
    Ok(res["result"].clone())
}
```

### 3.3 Popover UI Wireframe for `wizctl`
Using the exact same visual frame as `debugmic`:
```
┌───────────────────────────────────────────────┐
│ 💡  LIVING ROOM BULB                  [ ON ]  │  <-- Power Pill Switch
│ ───────────────────────────────────────────── │
│ 🔆  [═════════════════════●═════|═══]   85%   │  <-- Brightness Slider
│ 🌡  [════════●══════════════════════]  3500K   │  <-- Color Temperature Slider
│ ───────────────────────────────────────────── │
│ PRESET SCENES                                 │
│ [ Warm White ]  [ Daylight ]  [ Candlelight ] │  <-- Rounded Pill Buttons
│ [ Night Light]  [ Cozy     ]  [ Party       ] │
│ ───────────────────────────────────────────── │
│ COLOR PALETTE                                 │
│ [🔴] [🟠] [🟡] [🟢] [🔵] [🟣] [Custom Color]   │  <-- Color dots + RGB picker
│ ───────────────────────────────────────────── │
│ 🔄 Toggle Bulb State                          │  <-- Action Row (Hover Highlight)
└───────────────────────────────────────────────┘
```

---

## 4. Migration Execution Plan (for `wizctl` Session)

When opening the new session in `/home/sagnik/Projects/wizctl`:

### Phase 1: Cargo Project Setup
1. Initialize Rust package:
   ```bash
   cargo init --bin
   ```
2. Configure `Cargo.toml`:
   ```toml
   [package]
   name = "wizctl"
   version = "0.2.0"
   edition = "2021"

   [dependencies]
   eframe = { version = "0.31", default-features = false, features = ["default_fonts", "glow", "x11"] }
   serde = { version = "1.0", features = ["derive"] }
   serde_json = "1.0"
   glob = "0.3"

   [profile.release]
   opt-level = 3
   lto = true
   codegen-units = 1
   panic = "abort"
   strip = true
   ```

### Phase 2: UDP Communication Core (`src/bulb.rs`)
- Port `getPilot` and `setPilot` UDP socket handling from Python `pywizlight` to pure Rust.
- Implement async or non-blocking queries so the GUI never stutters during network latency.

### Phase 3: State & Settings (`src/state.rs`)
- Store bulb IP, last known state, and recent palette in `~/.config/wizctl/state.json`.

### Phase 4: Smart Popover Window (`src/main.rs` & `src/ui.rs`)
- Port the 5 pillars from `debugmic`:
  - `_NET_WM_STATE_SKIP_TASKBAR` & `_NET_WM_STATE_SKIP_PAGER`.
  - Mouse-anchored dynamic positioning (`y = 26.0`).
  - Auto-close on click-away / focus loss (`is_focused == Some(false)`).
  - Single-instance PID lock & close timestamp debounce.
  - Pixel-perfect XFCE dark theme.

### Phase 5: XFCE Panel Integration & Genmon
- `wizctl genmon` outputting XML for `xfce4-genmon-plugin` (dynamic bulb icon reflecting brightness/power, tooltip with current IP & Kelvin, click bound to `wizctl widget`).
- Replace Python launcher in `~/.config/xfce4/panel/launcher-*/` with the new native Rust binary.

---

## 5. Performance Target Comparison

| Metric | Current Python `wizctl` | Planned Rust + egui `wizctl` |
|---|---|---|
| **Binary Size** | ~35 MB (PyInstaller) | **~4.2 MB (Stripped ELF)** |
| **Startup Time** | ~1400 ms | **< 30 ms** |
| **Memory while Open** | ~110 MB | **~65 MB** |
| **Memory when Closed** | ~0 MB (exits) or leaked | **Strictly 0 bytes** |
| **Window Hygiene** | Taskbar tab, pager box | **Clean popup (0 taskbar/pager footprint)** |
| **Click-Away Behavior**| Stays open or broken focus | **Seamless auto-dismiss behind user** |
| **Dependencies** | Python 3, Tkinter, pywizlight | **Zero external runtimes (pure self-contained ELF)** |
