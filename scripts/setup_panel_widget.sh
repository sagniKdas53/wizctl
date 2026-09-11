#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN_PATH="$HOME/.local/bin/wizctl"
LAUNCHER_DIR="$HOME/.config/xfce4/panel/launcher-21"
DESKTOP_FILE="$LAUNCHER_DIR/17888794171.desktop"

echo "=== Installing wizctl Panel Widget & Desktop Launcher ==="

# 0. Install binary if available
mkdir -p "$HOME/.local/bin"
if [ -f "$REPO_DIR/target/release/wizctl" ]; then
    cp "$REPO_DIR/target/release/wizctl" "$BIN_PATH"
    chmod +x "$BIN_PATH"
    echo "✓ Installed native binary to $BIN_PATH"
elif command -v wizctl >/dev/null 2>&1; then
    BIN_PATH="$(command -v wizctl)"
    echo "✓ Using existing wizctl at $BIN_PATH"
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

# 2. Update panel launcher if present
if [ -d "$LAUNCHER_DIR" ]; then
    cat << PANEL_EOF > "$DESKTOP_FILE"
[Desktop Entry]
Version=1.0
Type=Application
Name=wizctl
Comment=WiZ Smart Light Controller (Single-click: Quick Widget | Double-click: Toggle)
Exec=$BIN_PATH widget --click
Icon=$HOME/.local/share/icons/hicolor/48x48/apps/wizctl.png
Path=$REPO_DIR
Terminal=false
StartupNotify=false
PANEL_EOF
    chmod +x "$DESKTOP_FILE"
    echo "✓ Updated active XFCE panel launcher: $DESKTOP_FILE"
fi

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
Path=$REPO_DIR
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
if command -v xfce4-panel >/dev/null 2>&1; then
    echo "Reloading xfce4-panel..."
    xfce4-panel -r || true
    echo "✓ XFCE panel reloaded!"
fi

echo "=== Panel Widget Setup Complete! ==="
