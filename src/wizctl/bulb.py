"""WiZ bulb control functions and communication logic."""

import asyncio
from contextlib import asynccontextmanager
import os
from typing import AsyncGenerator, Dict, Optional, Tuple

from pywizlight import PilotBuilder, SCENES, wizlight

from wizctl.parsers import parse_brightness, parse_color, parse_scene

DEFAULT_BULB_IP = os.environ.get("WIZ_IP", os.environ.get("BULB_IP", "192.168.0.102"))


@asynccontextmanager
async def get_bulb(ip: str) -> AsyncGenerator[wizlight, None]:
    """Context manager to ensure proper cleanup of wizlight connection."""
    bulb = wizlight(ip)
    try:
        yield bulb
    finally:
        await bulb.async_close()


async def get_status_info(ip: str) -> Dict:
    """Fetch status dictionary from the bulb."""
    async with get_bulb(ip) as bulb:
        states = await bulb.updateState()
        if not states or states[0] is None:
            raise RuntimeError(f"Unable to retrieve status from bulb at {ip}")

        state = states[0]
        rgb = state.get_rgb()
        has_rgb = rgb is not None and rgb[0] is not None

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
            "raw": state.pilotResult,
        }


async def command_status(ip: str) -> None:
    """Show formatted bulb status."""
    info = await get_status_info(ip)

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


async def command_color(ip: str, value: str) -> Tuple[int, int, int]:
    """Set the bulb RGB color."""
    rgb = parse_color(value)
    async with get_bulb(ip) as bulb:
        await bulb.turn_on(PilotBuilder(rgb=rgb))

    hex_code = f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"
    print(f"✓ RGB({rgb[0]}, {rgb[1]}, {rgb[2]}) ({hex_code})")
    return rgb


async def command_brightness(ip: str, value: str) -> int:
    """Set the bulb brightness."""
    brightness = parse_brightness(value)
    async with get_bulb(ip) as bulb:
        await bulb.turn_on(PilotBuilder(brightness=brightness))

    percent = brightness * 100 / 255
    print(f"✓ Brightness {brightness}/255 ({percent:.0f}%)")
    return brightness


async def command_kelvin(ip: str, value: int) -> int:
    """Set the bulb color temperature in Kelvin."""
    if not 1000 <= value <= 12000:
        raise ValueError("Kelvin must be between 1000 and 12000 (typical range 2200K - 6500K)")

    async with get_bulb(ip) as bulb:
        await bulb.turn_on(PilotBuilder(colortemp=value))

    print(f"✓ {value}K")
    return value


async def command_scene(ip: str, value: str) -> Tuple[int, str]:
    """Set the bulb scene by ID or name."""
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
