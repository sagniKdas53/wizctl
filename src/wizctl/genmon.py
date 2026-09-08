"""XFCE Generic Monitor (xfce4-genmon-plugin) status provider."""

import os
from pathlib import Path
import sys
from typing import Optional

from pywizlight import SCENES

from wizctl.bulb import DEFAULT_BULB_IP
from wizctl.state import load_state


def get_panel_asset_dir() -> Path:
    """Ensure panel icons are saved in a persistent user directory for XFCE genmon."""
    user_assets = Path.home() / ".local" / "share" / "wizctl" / "assets"
    user_assets.mkdir(parents=True, exist_ok=True)
    
    bundled = Path(__file__).parent / "assets"
    if bundled.is_dir():
        for f in bundled.glob("*.png"):
            target = user_assets / f.name
            if not target.exists() or target.stat().st_size != f.stat().st_size:
                try:
                    target.write_bytes(f.read_bytes())
                except Exception:
                    pass
    return user_assets


def generate_genmon_xml(target_ip: Optional[str] = None) -> str:
    """Generate XML output conforming to xfce4-genmon-plugin specification."""
    state = load_state()
    ip = target_ip or state.get("ip", DEFAULT_BULB_IP)

    assets_dir = get_panel_asset_dir()
    
    power = state.get("power", True)
    brightness = state.get("brightness", 255)
    pct = int(brightness * 100 / 255)
    scene_id = state.get("scene_id")
    mode = state.get("mode", "color")
    kelvin = state.get("kelvin", 2700)
    hex_code = state.get("hex", "#ff8c00")

    # Icon selection
    if power:
        icon_path = assets_dir / "panel_bulb_on.png"
        status_text = f"{pct}%"
        if mode == "scene" and scene_id:
            sname = SCENES.get(scene_id, f"Scene {scene_id}")
            status_text = f"{sname} ({pct}%)"
        power_str = "ON"
    else:
        icon_path = assets_dir / "panel_bulb_off.png"
        status_text = "OFF"
        power_str = "OFF"

    # Tooltip
    tooltip_lines = [
        f"WiZ Smart Light ({ip})",
        f"Status: {power_str}",
        f"Brightness: {pct}% ({brightness}/255)",
    ]
    if mode == "scene" and scene_id:
        tooltip_lines.append(f"Scene: {SCENES.get(scene_id, f'ID {scene_id}')}")
    elif mode == "kelvin":
        tooltip_lines.append(f"White Temp: {kelvin}K")
    else:
        tooltip_lines.append(f"Color: {hex_code}")
    tooltip_lines.append("Left-click: Quick Widget | Double-click: Toggle")
    tooltip = "\n".join(tooltip_lines)

    # Click command: uses our double-click/single-click handler
    # Locate wizctl binary or python script
    click_cmd = "wizctl widget --click"

    xml = (
        f"<img>{icon_path.resolve()}</img>\n"
        f"<txt> {status_text} </txt>\n"
        f"<tool>{tooltip}</tool>\n"
        f"<click>{click_cmd}</click>\n"
    )
    return xml


def run_genmon(target_ip: Optional[str] = None) -> int:
    """CLI runner for genmon output."""
    sys.stdout.write(generate_genmon_xml(target_ip=target_ip))
    return 0
