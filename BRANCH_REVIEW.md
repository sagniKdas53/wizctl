# Branch review and Rust migration handoff (2026-09-30)

The project direction is Rust 0.2. PR #4 replaces the Python implementation,
ports the product features from #1 and the optimistic update behavior from #2,
and adds a Linux/XFCE popover alongside the cross-platform studio and CLI.
This documentation branch is rebased on that Rust delivery.

| Branch / PR | Disposition | Remaining purpose |
| --- | --- | --- |
| `main` / #4 | Rust 0.2 primary implementation | CLI, studio, protocol library, XFCE integration and per-OS binary CI |
| `worktree-ci-setup` | Redundant reference to the old Python baseline | Can be removed separately; no implementation to merge |
| `feat/gui-and-palette` / #1 | Superseded implementation; leave open for owner cleanup | Product feature coverage is recorded in `MIGRATION.md`; sample photos and generated archives are not imported |
| `feat/conflict-safe-state` / #2 | Superseded implementation; leave open for owner cleanup | Delta updates, state tokens, conflicts and readback are implemented and tested in Rust |
| `codex/review-branches-and-plan-android-widget` / #3 | Keep as documentation-only follow-up | Android MVP planning using the shared WiZ protocol semantics |

## Compatibility boundary

Rust 0.2 removes Python packaging and the importable Python API. The final
Python baseline remains available at source revision `5ff0c53`; the GUI
experiment remains on #1 for reference. This handoff does not create a Python
release, remove branches, or close #1/#2. Rust library callers use `wizctl::bulb`.

The core protocol is separate from the desktop UI in `src/bulb.rs`. Sanitized
request/response fixtures in `tests/fixtures/wiz/` can be copied into Android
protocol tests without embedding Rust desktop or Python dependencies.

## Delivery checks

The Rust pipeline uses `cargo fmt --check`, `cargo clippy --locked --all-targets
-- -D warnings`, `cargo test --locked`, and locked release builds on Linux,
macOS and Windows. It uploads executable artifacts for each OS. The panel
installer is specific to Linux/XFCE and is outside the CLI/studio build path.

The automated cases cover all twelve #4 review findings and the #2 behaviors:
fresh pre-write reads, stale-state rejection with fresh state attached,
delta-only mutations, no-op ON, state-only OFF, and post-write readback.
The LAN protocol still has no atomic compare-and-swap primitive. UI conflicts
refresh state and are never retried automatically.

`python3 scripts/verify_x11.py --binary target/release/wizctl` provides a
repeatable X11 smoke test against a local fake bulb with isolated settings.
It checks normal studio close persistence, utility/taskbar/pager window
properties, Escape, focus-loss dismissal, and second-launch toggling.
Physical multi-monitor GUI behavior and standardized cold-start/performance
measurements remain separate acceptance work. Hosted macOS/Windows packaging
checks do not prove native interactive behavior on physical desktops.

## Historical audit

The 2026-09-22 review found 126 passing Python baseline tests with one optional
live-device skip, 39 passing early Rust tests, a failing assertion in #2, and
headless GUI setup failures in #1. Those historical results explain why the
Python branches were used as feature references rather than merged wholesale.
They do not validate the current Rust head; use the current CI and migration
validation record.

## Android next step

Follow `ANDROID_WIDGET_PLAN.md` after the desktop delivery. Start with a pure
Kotlin UDP client and shared JSON fixtures, then verify on a physical phone and
bulb. No Android runtime implementation is included in this PR.
