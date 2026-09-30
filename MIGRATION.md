# Rust 0.2 migration and feature coverage

PR #4 replaces the Python CLI/library with a Rust CLI, reusable LAN protocol
module, studio, and Linux/XFCE popover. It carries the product features from
PRs #1 and #2 forward in Rust; importing the old Python package is a breaking
change. Keep the final Python source revision for applications that still
import `wizctl`.

| Planned feature | Rust implementation |
| --- | --- |
| Power, brightness, Kelvin, RGB, scenes, WiZclick | CLI in `main.rs`/`bulb.rs`; studio controls in `studio.rs` |
| HSV wheel, RGB/HEX synchronization, scene browser, brightness and Kelvin chips | `studio.rs`, shared drawing in `theme.rs` |
| Median-cut image palette, dominance percentages, clickable swatches | `palette.rs` and studio palette tab |
| Numbered palette output, deterministic `--apply`, ANSI terminal picker | `main.rs`, including Windows terminal handling |
| Native image drop, image chooser, pasted file paths and URIs | `studio.rs`; chooser runs off the UI thread |
| IP, mode, power, brightness, RGB/Kelvin/scene, recent palette persistence | `state.rs`; corrupted fields are sanitized and temporary CLI IP overrides are not saved |
| Background networking, latency, RSSI and MAC status | `worker.rs` and studio connection card |
| IPv4 and Kelvin safety; slider flood protection | `state.rs`, `colors.rs`, `bulb.rs`, serialized/coalesced worker |
| Linux, macOS and Windows binaries | Locked test/build matrix and artifacts in `.github/workflows/ci.yml` |
| State token, rhythm/source metadata, stale-state rejection | `PilotResult` and conflict-safe update API in `bulb.rs` |
| Fresh pre-write read, delta-only write, no-op ON, state-only OFF | Core update API, used by CLI and GUI worker |
| Post-write actual-state readback, no conflict retry | Core update API and worker feedback |

The GUI treats a conflict as a request to refresh: it reports the conflict and
adopts the fresh device state without replaying the rejected command. A WiZ
bulb offers no atomic compare-and-swap operation, so another controller can
still write between our pre-read and mutation. Readback reports the observed
result rather than guaranteeing exclusive control.

Reconnect preset restoration is an explicit opt-in. Restoring an OFF preset
sends only power OFF. Restoring an ON preset sends its selected mode and
brightness in one update; normal controls send only their requested fields.

The six review regressions have dedicated coverage: timeout/rejected mutation,
OFF restoration, black RGB, temporary studio IP, exact accepted Kelvin, and
obsolete image extraction completions. State/schema, path normalization,
protocol conflicts, readback, terminal CLI, and monitor placement also have
automated coverage.

Performance figures in `WIZCTL_RUST_REWRITE_PLAN.md` are targets, not measured
guarantees. Single-monitor X11 smoke checks and monitor-geometry tests do not
replace a physical multi-monitor acceptance run. Hosted macOS/Windows builds
verify packaging and CLI behavior; they do not verify interactive native GUI
behavior on physical macOS/Windows desktops.
