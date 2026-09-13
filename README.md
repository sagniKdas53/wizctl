# wizctl

[![CI](https://github.com/sagniKdas53/wizctl/actions/workflows/ci.yml/badge.svg)](https://github.com/sagniKdas53/wizctl/actions/workflows/ci.yml)

A high-performance native Rust application and smart desktop popover widget for controlling WiZ Connected smart light bulbs directly over local area network (LAN) using UDP JSON-RPC without cloud dependencies.

---

## Performance Highlights

| Metric | Previous Python `wizctl` | Native Rust `wizctl` |
|---|---|---|
| **Binary Size** | ~35 MB (PyInstaller) | **~4.7 MB (Stripped ELF)** |
| **Cold Startup Time** | ~1400 ms | **< 4 ms** |
| **Idle Memory (Closed)** | Leaked / resident | **Strictly 0 bytes (Clean process exit)** |
| **Window Hygiene** | Taskbar tab, pager box | **Clean popup (0 taskbar/pager footprint)** |
| **Click-Away Behavior** | Broken focus / manual hide | **Seamless auto-dismiss behind user** |
| **Runtime Dependencies** | Python 3, Tkinter, pywizlight | **Zero external runtimes (pure self-contained ELF)** |

---

## The "True Popover" Architectural Pillars

1. **X11 Window Management (No Taskbar / Pager Tabs)**:
   - Configured with `_NET_WM_WINDOW_TYPE_UTILITY` and `_NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER, _NET_WM_STATE_ABOVE`.
   - Never pollutes the taskbar or workspace switcher.
2. **Dynamic Cursor-Anchored Placement**:
   - Anchors directly beneath the mouse cursor upon clicking the panel icon.
3. **Smart Click-Away Auto-Dismissal**:
   - Focus tracking with startup grace period and drag protection; automatically closes when clicking away or pressing `Escape`.
4. **Instant Single-Instance Toggling**:
   - PID guard and close-debounce latch so clicking the panel launcher toggles the popover open and closed instantly.
5. **Pixel-Accurate XFCE Dark Theme**:
   - Charcoal/slate dark palette (`#303133`) with custom vector-drawn pill toggles, sliders, glowing bulb icons, and color swatches.

---

## Project Structure

```
wizctl/
├── Cargo.toml           # Rust package definition and dependencies (eframe, serde, image)
├── Makefile             # Build, install, test, and panel setup targets
├── src/
│   ├── lib.rs           # Core library module exports
│   ├── main.rs          # CLI dispatcher, panel click handling, subcommand routing
│   ├── ui.rs            # Compact popover: power banner, brightness/Kelvin presets, scenes,
│   │                    #   quick colors, image drag & drop
│   ├── studio.rs        # Full studio window: color wheel, Kelvin, scene browser, image palette
│   ├── theme.rs         # Shared dark palette and hand-painted widget atoms (no icon fonts)
│   ├── worker.rs        # Background UDP command pump with coalescing and ping backoff
│   ├── palette.rs       # Median-cut dominant color extraction from images
│   ├── bulb.rs          # Pure Rust WiZ UDP protocol (JSON-RPC on port 38899)
│   ├── state.rs         # Local state cache (~/.config/wizctl/state.json)
│   ├── colors.rs        # Preset colors, scene definitions, Kelvin/HSV conversions
│   └── genmon.rs        # XFCE4 Genmon XML status provider with embedded icons
├── assets/              # Lightbulb panel icons (on, off, offline, and app icons)
├── scripts/
│   └── setup_panel_widget.sh # Installs launcher desktop files and reloads xfce4-panel
└── tests/               # Integration tests (UDP mock server, colors, state, genmon, palette)
```

---

## Two Surfaces

| | `wizctl widget` (default) | `wizctl studio` (alias `wizctl gui`) |
|---|---|---|
| **Window** | Frameless popover anchored at the cursor | Frameless resizable window with a painted titlebar (drag to move, double-click to maximize) and egui resize grips |
| **Dismissal** | Click-away, `Escape`, or panel re-click | Titlebar close button |
| **Controls** | Power banner, brightness slider + 25/50/75/100% chips, Kelvin gradient slider + 2200/2700/4000/6500K chips, Cozy/Sunset/Ocean/Night scenes, quick color dots, inline custom color picker | Everything in the widget plus an IP/ping bar, HSV color wheel, full scene browser, and the image palette tab |
| **Image palette** | Drop an image on the popover to replace the quick colors with its dominant colors | Drop an image or use `Select Image...`, with thumbnail, dominance percentages and 4/6/8/12 color counts |

---

## Quickstart

### 1. Build from Source

Requirements: A standard Rust toolchain (`cargo`, `rustc`).

```bash
# Build optimized release binary
make build

# Run tests
make test
```

### 2. Install & Configure XFCE Panel

```bash
# Installs binary to ~/.local/bin/wizctl and configures panel launcher
make install
```

---

## Usage

By default, `wizctl` targets `192.168.0.102` (or the IP configured in `WIZ_IP` / `BULB_IP` environment variables or `~/.config/wizctl/state.json`). You can also pass `--ip <IP>`.

```bash
# Launch the compact desktop popover widget (default)
wizctl
wizctl widget

# Launch the full studio window (color wheel, Kelvin, scenes, image palette)
wizctl gui
wizctl studio

# Check bulb status
wizctl status

# Turn bulb on / off / toggle
wizctl on
wizctl off
wizctl toggle

# Set brightness (percentage or 0-255)
wizctl brightness 80%
wizctl brightness 200

# Set color (name, hex, or RGB)
wizctl color blue
wizctl color #ff5500
wizctl color "255, 128, 0"
wizctl color warmwhite

# Set color temperature in Kelvin (2200K - 6500K)
wizctl kelvin 2700
wizctl kelvin 4000K

# Set WiZ scene by name or numeric ID
wizctl scene cozy
wizctl scene sunset
wizctl scene 6

# List all available scenes
wizctl scenes

# View or activate WiZclick wall switch modes
wizctl wizclick
wizctl wizclick 1
wizctl wizclick 2

# XFCE Genmon plugin status provider
wizctl genmon
```

---

## License

MIT License. See [LICENSE](LICENSE) for details.
