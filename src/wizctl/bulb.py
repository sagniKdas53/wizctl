"""WiZ bulb control functions and communication logic."""

import asyncio
from contextlib import asynccontextmanager
import math
import os
from typing import AsyncGenerator, Dict, List, Optional, Tuple, Union

from pywizlight import PilotBuilder, SCENES, wizlight

from wizctl.parsers import parse_brightness, parse_color, parse_kelvin, parse_scene, validate_ip

DEFAULT_BULB_IP = os.environ.get("WIZ_IP", os.environ.get("BULB_IP", "192.168.0.102"))


def kelvin_to_rgb(kelvin: int) -> Tuple[int, int, int]:
    """Convert color temperature in Kelvin (1000K-10000K) to an approximate RGB tuple.

    Uses Tanner Helland's algorithm (based on Planckian blackbody radiator curve).
    """
    temp = max(1000, min(40000, int(kelvin))) / 100.0

    # Calculate Red
    if temp <= 66:
        red = 255.0
    else:
        red = temp - 60
        red = 329.698727446 * (red ** -0.1332047592)
        red = max(0.0, min(255.0, red))

    # Calculate Green
    if temp <= 66:
        green = max(1.0, temp)
        green = 99.4708025861 * math.log(green) - 161.1195681661
        green = max(0.0, min(255.0, green))
    else:
        green = max(1.0, temp - 60)
        green = 288.1221695283 * (green ** -0.0755148492)
        green = max(0.0, min(255.0, green))

    # Calculate Blue
    if temp >= 66:
        blue = 255.0
    elif temp <= 19:
        blue = 0.0
    else:
        blue = max(1.0, temp - 10)
        blue = 138.5177312231 * math.log(blue) - 305.0447927307
        blue = max(0.0, min(255.0, blue))

    return int(round(red)), int(round(green)), int(round(blue))


async def apply_saved_state(ip: str, state: Dict) -> None:
    """Push saved configuration state to the bulb."""
    async with get_bulb(ip) as bulb:
        power = state.get("power", True)
        if not power:
            await bulb.turn_off()
            return

        brightness = state.get("brightness", 255)
        mode = state.get("mode")
        scene_id = state.get("scene_id")
        kelvin = state.get("kelvin")
        rgb = state.get("rgb")

        builder_kwargs = {"brightness": brightness}
        if mode == "scene" and scene_id:
            builder_kwargs["scene"] = scene_id
        elif mode == "kelvin" and kelvin:
            builder_kwargs["colortemp"] = kelvin
        elif rgb and len(rgb) == 3:
            builder_kwargs["rgb"] = (int(rgb[0]), int(rgb[1]), int(rgb[2]))
        elif kelvin:
            builder_kwargs["colortemp"] = kelvin
        elif scene_id:
            builder_kwargs["scene"] = scene_id

        await bulb.turn_on(PilotBuilder(**builder_kwargs))


@asynccontextmanager
async def get_bulb(ip: str) -> AsyncGenerator[wizlight, None]:
    """Context manager to ensure proper cleanup of wizlight connection with IP validation."""
    clean_ip = validate_ip(ip)
    bulb = wizlight(clean_ip)
    try:
        yield bulb
    finally:
        await bulb.async_close()


async def get_favorites(ip: str) -> List[Dict]:
    """Fetch WiZclick favorite modes from the bulb."""
    async with get_bulb(ip) as bulb:
        try:
            resp = await bulb.send({"method": "getFavs", "params": {}})
            favs_raw = resp.get("result", {}).get("favs", []) if isinstance(resp, dict) else []
            favorites = []
            for idx, fav in enumerate(favs_raw):
                if isinstance(fav, (list, tuple)) and len(fav) > 0:
                    sid = fav[0]
                elif isinstance(fav, int):
                    sid = fav
                else:
                    continue
                favorites.append({
                    "mode": idx + 1,
                    "scene_id": sid,
                    "scene_name": SCENES.get(sid, f"Scene {sid}") if sid else "None",
                })
            return favorites
        except Exception:
            return []


async def get_status_info(ip: str, fetch_favorites: bool = False) -> Dict:
    """Fetch status dictionary from the bulb."""
    async with get_bulb(ip) as bulb:
        states = await bulb.updateState()
        if not states or states[0] is None:
            raise RuntimeError(f"Unable to retrieve status from bulb at {ip}")

        state = states[0]
        rgb = state.get_rgb()
        has_rgb = rgb is not None and rgb[0] is not None

        favorites = []
        if fetch_favorites:
            try:
                resp = await bulb.send({"method": "getFavs", "params": {}})
                favs_raw = resp.get("result", {}).get("favs", []) if isinstance(resp, dict) else []
                for idx, fav in enumerate(favs_raw):
                    if isinstance(fav, (list, tuple)) and len(fav) > 0:
                        sid = fav[0]
                    elif isinstance(fav, int):
                        sid = fav
                    else:
                        continue
                    favorites.append({
                        "mode": idx + 1,
                        "scene_id": sid,
                        "scene_name": SCENES.get(sid, f"Scene {sid}") if sid else "None",
                    })
            except Exception:
                pass

        source = state.get_source() if hasattr(state, "get_source") else state.pilotResult.get("src")

        return {
            "ip": ip,
            "mac": state.get_mac(),
            "rssi": state.pilotResult.get("rssi"),
            "power": state.get_state(),
            "brightness": state.get_brightness(),
            "scene": state.get_scene(),
            "scene_id": state.get_scene_id(),
            "rgb": rgb if has_rgb else None,
            "colortemp": state.get_colortemp(),
            "source": source,
            "favorites": favorites,
            "raw": state.pilotResult,
        }


async def command_status(ip: str) -> None:
    """Show formatted bulb status."""
    info = await get_status_info(ip, fetch_favorites=True)

    print(f"Bulb:       {info['ip']}")

    if info["mac"]:
        print(f"MAC:        {info['mac']}")

    if info["rssi"] is not None:
        print(f"Signal:     {info['rssi']} dBm")

    print(f"Power:      {'ON' if info['power'] else 'OFF'}")

    brightness = info["brightness"]
    if brightness is not None:
        percent = brightness * 100 / 255
        print(f"Brightness: {brightness}/255 ({percent:.0f}%)")

    scene = info["scene"]
    scene_id = info["scene_id"]
    if scene:
        print(f"Scene:      {scene} (ID: {scene_id})")
    elif scene_id and scene_id != 0:
        print(f"Scene:      ID {scene_id}")
    else:
        print("Scene:      None")

    rgb = info["rgb"]
    if rgb and rgb[0] is not None:
        hex_code = f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"
        print(f"RGB:        {rgb[0]},{rgb[1]},{rgb[2]} ({hex_code})")

    kelvin = info["colortemp"]
    if kelvin:
        print(f"Kelvin:     {kelvin}K")

    if info.get("source"):
        print(f"Source:     {info['source']}")

    favorites = info.get("favorites")
    if favorites:
        fav_strs = [f"Mode {f['mode']}: {f['scene_name']}" for f in favorites]
        print(f"WiZclick:   {' | '.join(fav_strs)}")


async def command_wizclick(ip: str, mode: Optional[int] = None) -> Tuple[int, str]:
    """Show or activate WiZclick favorite modes."""
    favorites = await get_favorites(ip)
    if not favorites:
        favorites = [
            {"mode": 1, "scene_id": 6, "scene_name": "Cozy"},
            {"mode": 2, "scene_id": 14, "scene_name": "Night light"},
        ]

    if mode is not None:
        matched = next((f for f in favorites if f["mode"] == mode), None)
        if not matched:
            raise ValueError(f"WiZclick mode must be between 1 and {len(favorites)}, got {mode}")
        sid = matched["scene_id"]
        sname = matched["scene_name"]
        async with get_bulb(ip) as bulb:
            await bulb.turn_on(PilotBuilder(scene=sid))
        print(f"✓ WiZclick Mode {mode} ({sname})")
        return sid, sname

    print("WiZclick Settings (Wall Switch Modes):")
    print("-" * 40)
    for fav in favorites:
        print(f"  Mode {fav['mode']} (Click {fav['mode']}): {fav['scene_name']} (Scene ID: {fav['scene_id']})")
    print("-" * 40)
    print("Toggle physical wall switch once for Mode 1, twice quickly for Mode 2.")
    print("Usage: wizctl wizclick [1|2]")
    return 0, ""


async def command_on(ip: str) -> None:
    """Turn the bulb on."""
    async with get_bulb(ip) as bulb:
        await bulb.turn_on()
    print("✓ ON")


async def command_off(ip: str) -> None:
    """Turn the bulb off."""
    async with get_bulb(ip) as bulb:
        await bulb.turn_off()
    print("✓ OFF")


async def command_toggle(ip: str) -> bool:
    """Toggle the bulb power state and return the new power state (True=ON, False=OFF)."""
    async with get_bulb(ip) as bulb:
        states = await bulb.updateState()
        if not states or not states[0]:
            raise RuntimeError(f"Unable to retrieve bulb state from {ip}")

        if states[0].get_state():
            await bulb.turn_off()
            print("✓ OFF (toggled)")
            return False
        else:
            await bulb.turn_on()
            print("✓ ON (toggled)")
            return True


async def command_color(ip: str, value: Union[str, Tuple[int, int, int]]) -> Tuple[int, int, int]:
    """Set the bulb RGB color safely."""
    if isinstance(value, (tuple, list)):
        if len(value) != 3 or not all(isinstance(x, int) and 0 <= x <= 255 for x in value):
            raise ValueError(f"RGB tuple must contain 3 integers in [0, 255], got {value}")
        rgb = (int(value[0]), int(value[1]), int(value[2]))
    else:
        rgb = parse_color(str(value))

    async with get_bulb(ip) as bulb:
        await bulb.turn_on(PilotBuilder(rgb=rgb))

    hex_code = f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"
    print(f"✓ RGB({rgb[0]}, {rgb[1]}, {rgb[2]}) ({hex_code})")
    return rgb


async def command_brightness(ip: str, value: Union[str, int, float]) -> int:
    """Set the bulb brightness safely (0-255)."""
    brightness = parse_brightness(value)
    async with get_bulb(ip) as bulb:
        await bulb.turn_on(PilotBuilder(brightness=brightness))

    percent = brightness * 100 / 255
    print(f"✓ Brightness {brightness}/255 ({percent:.0f}%)")
    return brightness


async def command_kelvin(ip: str, value: Union[str, int, float]) -> int:
    """Set the bulb color temperature in Kelvin (1000K-10000K)."""
    kelvin = parse_kelvin(value)
    async with get_bulb(ip) as bulb:
        await bulb.turn_on(PilotBuilder(colortemp=kelvin))

    print(f"✓ {kelvin}K")
    return kelvin


async def command_scene(ip: str, value: Union[str, int]) -> Tuple[int, str]:
    """Set the bulb scene safely by ID or name."""
    scene_id, scene_name = parse_scene(value)
    async with get_bulb(ip) as bulb:
        await bulb.turn_on(PilotBuilder(scene=scene_id))

    print(f"✓ Scene {scene_id} ({scene_name})")
    return scene_id, scene_name


def command_scenes() -> None:
    """List all available WiZ scenes."""
    print("Available WiZ Scenes:")
    print("-" * 35)
    for sid, sname in sorted(SCENES.items(), key=lambda x: x[0]):
        if sid <= 40:
            print(f"  {sid:2d}: {sname}")
    print("-" * 35)
    print("Usage: wizctl scene <id|name>")
