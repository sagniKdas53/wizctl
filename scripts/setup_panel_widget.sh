#!/usr/bin/env bash
set -e

REPO_DIR="/home/sagnik/Projects/wizctl"
LAUNCHER_DIR="$HOME/.config/xfce4/panel/launcher-21"
DESKTOP_FILE="$LAUNCHER_DIR/17888794171.desktop"

echo "=== Installing wizctl Panel Widget & Desktop Launcher ==="

# 1. Ensure user icons directory exists and copy icons
mkdir -p "$HOME/.local/share/icons/hicolor/512x512/apps"
mkdir -p "$HOME/.local/share/icons/hicolor/256x256/apps"
mkdir -p "$HOME/.local/share/icons/hicolor/128x128/apps"
mkdir -p "$HOME/.local/share/icons/hicolor/64x64/apps"
mkdir -p "$HOME/.local/share/icons/hicolor/48x48/apps"
mkdir -p "$HOME/.local/share/icons/hicolor/32x32/apps"
mkdir -p "$HOME/.local/share/icons/hicolor/16x16/apps"

cp "$REPO_DIR/src/wizctl/assets/icon_512.png" "$HOME/.local/share/icons/hicolor/512x512/apps/wizctl.png"
cp "$REPO_DIR/src/wizctl/assets/icon_256.png" "$HOME/.local/share/icons/hicolor/256x256/apps/wizctl.png"
cp "$REPO_DIR/src/wizctl/assets/icon_128.png" "$HOME/.local/share/icons/hicolor/128x128/apps/wizctl.png"
cp "$REPO_DIR/src/wizctl/assets/icon_64.png" "$HOME/.local/share/icons/hicolor/64x64/apps/wizctl.png"
cp "$REPO_DIR/src/wizctl/assets/icon_48.png" "$HOME/.local/share/icons/hicolor/48x48/apps/wizctl.png"
cp "$REPO_DIR/src/wizctl/assets/icon_32.png" "$HOME/.local/share/icons/hicolor/32x32/apps/wizctl.png"
cp "$REPO_DIR/src/wizctl/assets/icon_16.png" "$HOME/.local/share/icons/hicolor/16x16/apps/wizctl.png"

# 2. Update panel launcher if present
if [ -d "$LAUNCHER_DIR" ]; then
    cat << 'EOF' > "$DESKTOP_FILE"
[Desktop Entry]
Version=1.0
Type=Application
Name=wizctl
Comment=WiZ Smart Light Controller (Single-click: Quick Widget | Double-click: Toggle)
Exec=/home/sagnik/Projects/wizctl/dist/wizctl widget --click
Icon=/home/sagnik/Projects/wizctl/src/wizctl/assets/icon_48.png
Path=/home/sagnik/Projects/wizctl
Terminal=false
StartupNotify=false
EOF
    chmod +x "$DESKTOP_FILE"
    echo "✓ Updated active XFCE panel launcher: $DESKTOP_FILE"
fi

# 3. Create desktop application launcher
mkdir -p "$HOME/.local/share/applications"
cat << 'EOF' > "$HOME/.local/share/applications/wizctl.desktop"
[Desktop Entry]
Version=1.0
Type=Application
Name=wizctl
GenericName=Smart Light Controller
Comment=Control WiZ smart light bulbs over LAN
Exec=/home/sagnik/Projects/wizctl/dist/wizctl gui
Icon=/home/sagnik/Projects/wizctl/src/wizctl/assets/icon_64.png
Path=/home/sagnik/Projects/wizctl
Terminal=false
Categories=Utility;HardwareSettings;
StartupNotify=true
Actions=widget;toggle;

[Desktop Action widget]
Name=Quick Control Widget
Exec=/home/sagnik/Projects/wizctl/dist/wizctl widget

[Desktop Action toggle]
Name=Toggle Light Power
Exec=/home/sagnik/Projects/wizctl/dist/wizctl toggle
EOF

echo "✓ Created application entry: $HOME/.local/share/applications/wizctl.desktop"

# 4. Reload XFCE panel
if command -v xfce4-panel >/dev/null 2>&1; then
    echo "Reloading xfce4-panel..."
    xfce4-panel -r || true
    echo "✓ XFCE panel reloaded!"
fi

echo "=== Panel Widget Setup Complete! ==="
