# Rust merge validation (2026-09-30)

Runtime changes were verified at `d9f7868`. The subsequent validation commit
only adds this record and makes startup focus explicit in the X11 test script.

- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked`: 66 passed, zero failures or ignored tests.
- `cargo build --release --locked`: passed.
- `bash -n scripts/setup_panel_widget.sh`: passed.
- Interactive release palette picker in a PTY: `j` moved selection; `q`
  cancelled with exit code zero and restored terminal mode.
- Release X11 smoke: temporary studio IP stayed temporary after normal close;
  utility/skip-taskbar/skip-pager/above properties were observed; Escape,
  second-launch toggle and focus loss each closed the popover.
- [Hosted three-OS CI](https://github.com/sagniKdas53/wizctl/actions/runs/36673966277):
  Linux, macOS and Windows passed check, formatting, strict lint, tests, release
  build, CLI smoke and binary artifact upload. OS-specific dependency-install
  steps were deliberately skipped on non-Linux runners.

The X11 smoke used a local fake UDP bulb and isolated configuration/runtime
directories; no physical light was mutated. Its desktop was one 1920x1080 X11
monitor. Negative-offset and narrow-monitor placement has unit coverage, while
physical multi-monitor and native macOS/Windows GUI interactions remain
unmeasured. Standardized cold-start, memory and binary-size performance targets
also remain unproven.

Reproduce the desktop checks with:

```sh
cargo build --release --locked
python3 scripts/verify_x11.py --binary target/release/wizctl
```

The script writes structured evidence to `/tmp/wizctl-x11-validation.json`.
Run it on an X11 desktop with `xdotool`, `xprop`, and `xrandr` available, after
the socket test suite finishes (both use the local WiZ UDP port).
