"""State persistence for wizctl GUI with schema and bounds validation."""

from copy import deepcopy
import json
import os
from pathlib import Path
from typing import Any, Dict, List

from wizctl.bulb import DEFAULT_BULB_IP
from wizctl.parsers import parse_brightness, parse_color, parse_kelvin, validate_ip

DEFAULT_PRESET_COLORS: List[str] = [
    "#ff453a",  # Red
    "#ff9f0a",  # Orange
    "#ffd60a",  # Yellow
    "#32d74b",  # Green
    "#64d2ff",  # Cyan
    "#0a84ff",  # Blue
    "#bf5af2",  # Purple
    "#ff375f",  # Pink
    "#ffd1a9",  # Warm White
    "#f0f4f8",  # Cool White
]

DEFAULT_STATE: Dict[str, Any] = {
    "ip": DEFAULT_BULB_IP,
    "power": True,
    "brightness": 255,
    "rgb": [255, 140, 0],
    "hex": "#ff8c00",
    "kelvin": 2700,
    "scene_id": 6,
    "mode": "color",
    "recent_colors": list(DEFAULT_PRESET_COLORS),
}


def get_state_file_path() -> Path:
    """Return path to the JSON state file, creating parent directories if needed."""
    xdg_config = os.environ.get("XDG_CONFIG_HOME")
    if xdg_config:
        config_dir = Path(xdg_config) / "wizctl"
    else:
        config_dir = Path.home() / ".config" / "wizctl"

    try:
        config_dir.mkdir(parents=True, exist_ok=True)
        return config_dir / "state.json"
    except OSError:
        fallback_dir = Path.home() / ".wizctl"
        fallback_dir.mkdir(parents=True, exist_ok=True)
        return fallback_dir / "state.json"


def sanitize_state(raw: Dict[str, Any]) -> Dict[str, Any]:
    """Sanitize and validate state fields against hardware constraints."""
    clean = deepcopy(DEFAULT_STATE)

    if not isinstance(raw, dict):
        return clean

    # Validate IP
    if "ip" in raw and isinstance(raw["ip"], str):
        try:
            clean["ip"] = validate_ip(raw["ip"])
        except ValueError:
            pass

    # Validate Power
    if "power" in raw and isinstance(raw["power"], bool):
        clean["power"] = raw["power"]

    # Validate Brightness (1-255)
    if "brightness" in raw:
        try:
            clean["brightness"] = max(1, parse_brightness(raw["brightness"]))
        except ValueError:
            pass

    # Validate RGB
    if "rgb" in raw and isinstance(raw["rgb"], (list, tuple)) and len(raw["rgb"]) == 3:
        try:
            r = max(0, min(255, int(raw["rgb"][0])))
            g = max(0, min(255, int(raw["rgb"][1])))
            b = max(0, min(255, int(raw["rgb"][2])))
            clean["rgb"] = [r, g, b]
            clean["hex"] = f"#{r:02x}{g:02x}{b:02x}"
        except (ValueError, TypeError):
            pass

    # Validate HEX if RGB wasn't overridden
    if "hex" in raw and isinstance(raw["hex"], str):
        try:
            rgb = parse_color(raw["hex"])
            clean["rgb"] = list(rgb)
            clean["hex"] = f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"
        except ValueError:
            pass

    # Validate Kelvin
    if "kelvin" in raw:
        try:
            clean["kelvin"] = parse_kelvin(raw["kelvin"])
        except ValueError:
            pass

    # Validate Scene ID
    if "scene_id" in raw and isinstance(raw["scene_id"], int):
        if 1 <= raw["scene_id"] <= 40:
            clean["scene_id"] = raw["scene_id"]

    # Validate Recent Colors
    if "recent_colors" in raw and isinstance(raw["recent_colors"], list):
        valid_recents: List[str] = []
        for color_item in raw["recent_colors"]:
            if isinstance(color_item, str):
                try:
                    c_rgb = parse_color(color_item)
                    valid_recents.append(f"#{c_rgb[0]:02x}{c_rgb[1]:02x}{c_rgb[2]:02x}")
                except ValueError:
                    pass
        if valid_recents:
            clean["recent_colors"] = valid_recents[:16]

    return clean


def load_state() -> Dict[str, Any]:
    """Load persisted state dictionary safely with schema validation."""
    path = get_state_file_path()
    state = deepcopy(DEFAULT_STATE)
    if path.is_file():
        try:
            with open(path, "r", encoding="utf-8") as f:
                data = json.load(f)
                state = sanitize_state(data)
        except Exception:
            state = deepcopy(DEFAULT_STATE)

    env_ip = os.environ.get("WIZ_IP") or os.environ.get("BULB_IP")
    if env_ip:
        try:
            state["ip"] = validate_ip(env_ip)
        except ValueError:
            pass

    return state


def save_state(state: Dict[str, Any]) -> None:
    """Save state dictionary to JSON file."""
    clean = sanitize_state(state)
    path = get_state_file_path()
    try:
        temp_path = path.with_suffix(".tmp")
        with open(temp_path, "w", encoding="utf-8") as f:
            json.dump(clean, f, indent=2)
        temp_path.replace(path)
    except Exception:
        pass
