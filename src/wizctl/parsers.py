"""Input parsing functions for color, brightness, scenes, kelvin, and IP addresses."""

import ipaddress
import math
import re
import sys
from typing import Any, Tuple, Union

from pywizlight import SCENES

from wizctl.colors import COLORS


def die(message: str, exit_code: int = 1) -> None:
    """Print an error message to stderr and exit."""
    print(f"wizctl: {message}", file=sys.stderr)
    sys.exit(exit_code)


def validate_ip(ip: str) -> str:
    """
    Validate an IPv4 address to ensure it is safe and reachable on the local LAN.

    Rejects broadcast, multicast, loopback abuse, or malformed strings.
    """
    if not isinstance(ip, str):
        raise ValueError(f"IP address must be a string, got {type(ip).__name__}")

    val = ip.strip()
    if not val:
        raise ValueError("IP address cannot be empty")

    try:
        addr = ipaddress.ip_address(val)
    except ValueError:
        raise ValueError(f"Invalid IP address format: '{ip}'")

    if not isinstance(addr, ipaddress.IPv4Address):
        raise ValueError(f"Only IPv4 addresses are supported by WiZ smart bulbs: '{ip}'")

    if addr.is_multicast:
        raise ValueError(f"Multicast IP addresses are not permitted: '{ip}'")

    if addr == ipaddress.IPv4Address("255.255.255.255"):
        raise ValueError(f"Global broadcast IP address is not permitted: '{ip}'")

    return val


def parse_color(value: str) -> Tuple[int, int, int]:
    """
    Parse a color string into a safe (R, G, B) tuple of ints in range [0, 255].

    Supported formats:
    - Named colors (e.g. 'red', 'warmwhite', 'warm-white')
    - 6-digit hex: '#ff5500' or 'ff5500'
    - 3-digit hex: '#f50' or 'f50'
    - RGB triple: '255,128,0' or '255, 128, 0'
    """
    if not isinstance(value, str):
        raise ValueError(f"Color value must be a string, got {type(value).__name__}")

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


def parse_brightness(value: Union[str, int, float]) -> int:
    """
    Parse a brightness value into a safe integer (0-255).

    Supported formats:
    - Percentage string: '50%' or '100%'
    - Raw integer/string: '128' or 255
    """
    if isinstance(value, (int, float)):
        if isinstance(value, float) and (math.isnan(value) or math.isinf(value)):
            raise ValueError("Brightness must be a finite number")
        b = int(round(value))
        if not 0 <= b <= 255:
            raise ValueError("Brightness must be between 0 and 255")
        return b

    if not isinstance(value, str):
        raise ValueError(f"Invalid brightness type: {type(value).__name__}")

    val = value.strip()
    if not val:
        raise ValueError("Brightness value cannot be empty")

    # Percentage: 50%
    if val.endswith("%"):
        try:
            percent = float(val[:-1])
        except ValueError:
            raise ValueError(f"Invalid brightness percentage: '{val}'")

        if math.isnan(percent) or math.isinf(percent):
            raise ValueError("Brightness percentage must be a finite number")

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


def parse_kelvin(value: Union[str, int, float]) -> int:
    """
    Parse and validate color temperature in Kelvin (safe range: 1000K - 10000K).

    Supported formats:
    - String with optional unit: '2700', '2700K', '4000k'
    - Integer / Float: 2700, 4000
    """
    if isinstance(value, (int, float)):
        if isinstance(value, float) and (math.isnan(value) or math.isinf(value)):
            raise ValueError("Kelvin value must be a finite number")
        val = int(round(value))
    elif isinstance(value, str):
        val_str = value.strip().rstrip("kK").strip()
        if not val_str:
            raise ValueError("Kelvin value cannot be empty")
        try:
            val = int(val_str)
        except ValueError:
            raise ValueError(
                f"Invalid Kelvin temperature: '{value}'. Expected integer e.g. 2700 or 4000K"
            )
    else:
        raise ValueError(f"Invalid Kelvin temperature type: {type(value).__name__}")

    if not 1000 <= val <= 10000:
        raise ValueError(
            f"Kelvin temperature {val}K is outside safe hardware range (1000K - 10000K, typical: 2200K - 6500K)"
        )

    return val


def parse_scene(value: Union[str, int]) -> Tuple[int, str]:
    """
    Parse a scene ID or scene name into safe (scene_id, scene_name).

    Supported formats:
    - Numeric ID: '1', '6', '29', 1, etc.
    - Scene name (case-insensitive): 'cozy', 'sunset', 'deep-dive', 'candlelight'
    """
    if isinstance(value, int):
        if value in SCENES:
            return value, SCENES[value]
        raise ValueError(
            f"Unknown scene ID: {value}. Run 'wizctl scenes' to list available scenes."
        )

    if not isinstance(value, str):
        raise ValueError(f"Scene value must be a string or integer, got {type(value).__name__}")

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
