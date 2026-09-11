# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`wizctl` is a high-performance native Rust application for controlling WiZ Connected smart light bulbs directly over LAN via UDP JSON-RPC. It features both a headless CLI and a native `egui`/`eframe` XFCE4 panel popover widget with zero runtime dependencies.

## Commands

```bash
make build       # build release binary (target/release/wizctl)
make test        # run complete unit and integration test suite (cargo test)
make check       # fast syntax and type checking (cargo check)
make install     # install release binary to ~/.local/bin/wizctl and configure panel
make panel       # install panel launcher desktop entries and reload xfce4-panel
make clean       # remove cargo target directory
make run         # launch the interactive popover widget
```

Run a single test: `cargo test --test test_colors_and_state test_named_colors`

The default target bulb IP is `192.168.0.102`, overridable via `--ip`, `-i`, `WIZ_IP`, or `BULB_IP`. State is saved to `~/.config/wizctl/state.json`.

## Architecture

- **`src/main.rs`** — CLI dispatcher, argument parsing, single-instance PID guard, panel click debouncer, and GUI launcher.
- **`src/ui.rs`** — native `egui`/`eframe` desktop popover widget implementing the 5 True Popover pillars (EWMH utility type, cursor-anchored placement, click-away auto-dismissal, single-instance PID toggle, pixel-accurate XFCE dark theme, non-blocking asynchronous background worker).
- **`src/bulb.rs`** — pure Rust UDP communication core for WiZ bulbs on port 38899 (`getPilot`, `setPilot`, `getFavs`).
- **`src/state.rs`** — persistent state management with schema sanitization (`~/.config/wizctl/state.json`).
- **`src/colors.rs`** — color parsing (hex, named, RGB), Planckian Kelvin-to-RGB conversion, and WiZ scene mappings.
- **`src/genmon.rs`** — `xfce4-genmon-plugin` XML status generator with dynamic embedded bulb icons and single/double-click action bindings.
- **`assets/`** — lightbulb panel icons (on, off, offline, and app launcher icons).
