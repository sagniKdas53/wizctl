# wizctl 0.2.1

Rust desktop and LAN-control release, with a compact Linux/XFCE popover,
cross-platform studio and CLI, image palettes, scenes, WiZclick, and
conflict-safe device updates.

- Match the XFCE panel's charcoal grey background and improve text contrast.
- Replace `xrandr` monitor discovery with cached Xinerama screen rectangles,
  avoiding the command that flashes a disabled laptop display on affected systems.
- Remove delayed window activation/raising and request startup focus once.
- Keep the pin and close controls accessible when connection text is long;
  make pinning visibly retain the popover when switching windows.
- Include tested protocol error handling, actual-state readback, queued-control
  conflict protection, safe state saving, and installer targeting from #4.

The Rust release replaces the Python package API. See `MIGRATION.md` for
feature coverage and compatibility details. Linux/XFCE uses the compact
popover; macOS and Windows use the studio for the widget command.

Downloads contain standalone binaries. Native macOS/Windows GUI behavior,
physical multi-monitor acceptance, and standardized performance targets remain
unverified. The disabled-display flash fix avoids the identified hardware
polling path; absence of a physical flash still needs user observation.
