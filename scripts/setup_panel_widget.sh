#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN_PATH="${WIZCTL_BIN_PATH:-$HOME/.local/bin/wizctl}"
PANEL_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/xfce4/panel"

echo "=== Installing wizctl Panel Widget & Desktop Launcher ==="

# 0. Install binary if available
mkdir -p "$(dirname "$BIN_PATH")"
if [ -f "$BIN_PATH" ]; then
    echo "✓ Using selected wizctl at $BIN_PATH"
elif [ -f "$REPO_DIR/target/release/wizctl" ]; then
    cp "$REPO_DIR/target/release/wizctl" "$BIN_PATH"
    chmod +x "$BIN_PATH"
    echo "✓ Installed native binary to $BIN_PATH"
elif [ -z "${WIZCTL_BIN_PATH:-}" ] && command -v wizctl >/dev/null 2>&1; then
    BIN_PATH="$(command -v wizctl)"
    echo "✓ Using existing wizctl at $BIN_PATH"
else
    echo "wizctl binary unavailable at $BIN_PATH; build release or set WIZCTL_BIN_PATH" >&2
    exit 1
fi

# 1. Ensure user icons directory exists and copy icons
for size in 16 22 24 32 48 64 128 256 512; do
    mkdir -p "$HOME/.local/share/icons/hicolor/${size}x${size}/apps"
    if [ -f "$REPO_DIR/assets/icon_${size}.png" ]; then
        cp "$REPO_DIR/assets/icon_${size}.png" "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/wizctl.png"
    fi
done

# Ensure panel icons directory exists
mkdir -p "$HOME/.local/share/wizctl/assets"
for panel_icon in panel_bulb_on.png panel_bulb_off.png panel_bulb_offline.png; do
    if [ -f "$REPO_DIR/assets/$panel_icon" ]; then
        cp "$REPO_DIR/assets/$panel_icon" "$HOME/.local/share/wizctl/assets/$panel_icon"
    fi
done
echo "✓ Installed application and panel icons"

# 2. Update existing wizctl panel launchers without assuming a machine-specific ID.
for desktop_file in "$PANEL_DIR"/launcher-*/*.desktop; do
    [ -f "$desktop_file" ] || continue
    if grep -Eqi '^Exec=.*wizctl' "$desktop_file"; then
        awk -v bin="$BIN_PATH" '
            /^\[/ { in_entry = ($0 == "[Desktop Entry]") }
            in_entry && /^Exec=/ { print "Exec=" bin " widget --click"; found = 1; next }
            { print }
            END { if (!found) exit 1 }
        ' "$desktop_file" > "$desktop_file.wizctl-tmp"
        mv "$desktop_file.wizctl-tmp" "$desktop_file"
        echo "✓ Updated XFCE panel launcher: $desktop_file"
    fi
done

# 3. Create desktop application launcher
mkdir -p "$HOME/.local/share/applications"
cat << APP_EOF > "$HOME/.local/share/applications/wizctl.desktop"
[Desktop Entry]
Version=1.0
Type=Application
Name=wizctl
GenericName=Smart Light Controller
Comment=Control WiZ smart light bulbs over LAN
Exec=$BIN_PATH gui
Icon=wizctl
Terminal=false
Categories=Utility;HardwareSettings;
StartupNotify=true
Actions=widget;toggle;

[Desktop Action widget]
Name=Quick Control Widget
Exec=$BIN_PATH widget

[Desktop Action toggle]
Name=Toggle Light Power
Exec=$BIN_PATH toggle
APP_EOF

echo "✓ Created application entry: $HOME/.local/share/applications/wizctl.desktop"

# 4. Reload XFCE panel
if [ "${WIZCTL_SKIP_PANEL_RELOAD:-0}" != "1" ] && command -v xfce4-panel >/dev/null 2>&1; then
    echo "Reloading xfce4-panel..."
    xfce4-panel -r || true
    echo "✓ XFCE panel reloaded!"
fi

echo "=== Panel Widget Setup Complete! ==="
