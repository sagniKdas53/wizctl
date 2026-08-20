"""Input parsing functions for color, brightness, and scenes."""

import re
import sys
from typing import Tuple

from pywizlight import SCENES

from wizctl.colors import COLORS


def die(message: str, exit_code: int = 1) -> None:
    """Print an error message to stderr and exit."""
    print(f"wizctl: {message}", file=sys.stderr)
    sys.exit(exit_code)


def parse_color(value: str) -> Tuple[int, int, int]:
    """
    Parse a color string into an (R, G, B) tuple.

    Supported formats:
    - Named colors (e.g. 'red', 'warmwhite', 'warm-white')
    - 6-digit hex: '#ff5500' or 'ff5500'
    - 3-digit hex: '#f50' or 'f50'
    - RGB triple: '255,128,0' or '255, 128, 0'
    """
    val = value.strip().lower()
    if not val:
        raise ValueError("Color value cannot be empty")

    # Direct match in named colors
    if val in COLORS:
        return COLORS[val]

    # Normalized named color (ignoring spaces, dashes, underscores)
    norm_val = re.sub(r"[\s\-_]+", "", val)
    if norm_val in COLORS:
        return COLORS[norm_val]

    # #RRGGBB or RRGGBB
    hex_val = val[1:] if val.startswith("#") else val
    if re.fullmatch(r"[0-9a-f]{6}", hex_val):
        return (
            int(hex_val[0:2], 16),
            int(hex_val[2:4], 16),
            int(hex_val[4:6], 16),
        )

    # #RGB or RGB (shorthand)
    if re.fullmatch(r"[0-9a-f]{3}", hex_val):
        return (
            int(hex_val[0] * 2, 16),
            int(hex_val[1] * 2, 16),
            int(hex_val[2] * 2, 16),
        )

    # R,G,B (with optional whitespace)
    match = re.fullmatch(r"(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})", val)
    if match:
        rgb = tuple(int(x) for x in match.groups())
        if all(0 <= x <= 255 for x in rgb):
            return rgb  # type: ignore[return-value]

    raise ValueError(
        f"Invalid color '{value}'. Use a name (e.g. red, warmwhite), "
        "#RRGGBB (#ff5500), #RGB (#f50), or R,G,B (255,128,0)"
    )


def parse_brightness(value: str) -> int:
    """
    Parse a brightness string into a raw integer (0-255).

    Supported formats:
    - Percentage: '50%' or '100%'
    - Raw integer: '128' or '255'
    """
    val = value.strip()
    if not val:
        raise ValueError("Brightness value cannot be empty")

    # Percentage: 50%
    if val.endswith("%"):
        try:
            percent = float(val[:-1])
        except ValueError:
            raise ValueError(f"Invalid brightness percentage: '{val}'")

        if not 0 <= percent <= 100:
            raise ValueError("Brightness percentage must be between 0% and 100%")

        return round(percent * 255 / 100)

    # Raw WiZ value: 0-255
    try:
        brightness = int(val)
    except ValueError:
        raise ValueError(f"Invalid brightness value '{val}'. Use 0-255 or 0%-100%")

    if not 0 <= brightness <= 255:
        raise ValueError("Brightness must be between 0 and 255")

    return brightness


def parse_scene(value: str) -> Tuple[int, str]:
    """
    Parse a scene ID or scene name into (scene_id, scene_name).

    Supported formats:
    - Numeric ID: '1', '6', '29', etc.
    - Scene name (case-insensitive): 'cozy', 'sunset', 'deep-dive', 'candlelight'
    """
    val = value.strip().lower()
    if not val:
        raise ValueError("Scene value cannot be empty")

    # Numeric ID
    try:
        scene_id = int(val)
    except ValueError:
        scene_id = None

    if scene_id is not None:
        if scene_id in SCENES:
            return scene_id, SCENES[scene_id]
        raise ValueError(
            f"Unknown scene ID: {scene_id}. Run 'wizctl scenes' to list available scenes."
        )

    # Try matching scene name (case-insensitive, ignoring spaces/hyphens)
    norm_val = re.sub(r"[\s\-_]+", "", val)
    for sid, sname in SCENES.items():
        norm_name = re.sub(r"[\s\-_]+", "", sname.lower())
        if norm_val == norm_name:
            return sid, sname

    raise ValueError(
        f"Unknown scene: '{value}'. Run 'wizctl scenes' to list available scenes."
    )
