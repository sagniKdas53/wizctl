"""WiZ bulb control functions and communication logic."""

import asyncio
from contextlib import asynccontextmanager
import hashlib
import json
import os
from typing import Any, AsyncGenerator, Dict, Optional, Tuple

from pywizlight import PilotBuilder, SCENES, wizlight

from wizctl.parsers import parse_brightness, parse_color, parse_scene

DEFAULT_BULB_IP = os.environ.get("WIZ_IP", os.environ.get("BULB_IP", "192.168.0.102"))

# Only fields that describe the bulb's effective controllable light state belong
# in the optimistic-concurrency token. Telemetry such as RSSI is intentionally
# excluded, otherwise harmless signal-strength changes would look like conflicts.
CONTROL_STATE_KEYS = (
    "state",
    "dimming",
    "sceneId",
    "r",
    "g",
    "b",
    "c",
    "w",
    "temp",
    "speed",
    "schdPsetId",
    "ratio",
)


class StateConflictError(RuntimeError):
    """Raised when the bulb changed after a caller last observed its state.

    WiZ does not expose an atomic compare-and-set primitive over the LAN
    protocol. This exception implements optimistic concurrency on the client:
    callers retain a state token, and writes are refused when a fresh read no
    longer matches that token.
    """

    def __init__(
        self,
        expected_state_token: str,
        current_state: Dict[str, Any],
    ) -> None:
        self.expected_state_token = expected_state_token
        self.current_state = current_state
        self.current_state_token = current_state["state_token"]
        super().__init__(
            "bulb state changed since it was last read "
            f"(expected {expected_state_token}, current {self.current_state_token}); "
            "refresh before applying the change"
        )


def state_token_from_raw(raw: Dict[str, Any]) -> str:
    """Return a stable fingerprint of the effective controllable bulb state.

    The token is deliberately *not* a device revision number. It is a compact
    content fingerprint used to notice app/schedule/rhythm/physical-control
    changes between a UI read and a later write.
    """
    snapshot = {key: raw.get(key) for key in CONTROL_STATE_KEYS}
    payload = json.dumps(snapshot, sort_keys=True, separators=(",", ":"), default=str)
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()[:16]


@asynccontextmanager
async def get_bulb(ip: str) -> AsyncGenerator[wizlight, None]:
    """Context manager to ensure proper cleanup of wizlight connection."""
    bulb = wizlight(ip)
    try:
        yield bulb
    finally:
        await bulb.async_close()


def _state_to_info(ip: str, state: Any) -> Dict[str, Any]:
    """Convert a pywizlight PilotParser state into wizctl's status dictionary."""
    raw = state.pilotResult
    rgb = state.get_rgb()
    has_rgb = rgb is not None and rgb[0] is not None

    return {
        "ip": ip,
        "mac": state.get_mac(),
        "rssi": raw.get("rssi"),
        "power": state.get_state(),
        "brightness": state.get_brightness(),
        "scene": state.get_scene(),
        "scene_id": state.get_scene_id(),
        "rgb": rgb if has_rgb else None,
        "colortemp": state.get_colortemp(),
        # schdPsetId is the room/rhythm preset reported by WiZ. Keeping it in
        # the public status makes rhythm-driven changes visible to a future UI.
        "rhythm_id": raw.get("schdPsetId"),
        "source": raw.get("src"),
        "state_token": state_token_from_raw(raw),
        "raw": raw,
    }


async def _get_status_info_from_bulb(bulb: wizlight, ip: str) -> Dict[str, Any]:
    """Fetch status using an already-open bulb connection."""
    states = await bulb.updateState()
    if not states or states[0] is None:
        raise RuntimeError(f"Unable to retrieve status from bulb at {ip}")
    return _state_to_info(ip, states[0])


def _check_expected_state(
    expected_state_token: Optional[str],
    current_state: Dict[str, Any],
) -> None:
    """Refuse a write when the caller's last-seen state is stale."""
    if (
        expected_state_token is not None
        and expected_state_token != current_state["state_token"]
    ):
        raise StateConflictError(expected_state_token, current_state)


async def get_status_info(ip: str) -> Dict[str, Any]:
    """Fetch status dictionary from the bulb."""
    async with get_bulb(ip) as bulb:
        return await _get_status_info_from_bulb(bulb, ip)


async def apply_update(
    ip: str,
    *,
    expected_state_token: Optional[str] = None,
    power: Optional[bool] = None,
    brightness: Optional[int] = None,
    rgb: Optional[Tuple[int, int, int]] = None,
    colortemp: Optional[int] = None,
    scene: Optional[int] = None,
    speed: Optional[int] = None,
) -> Dict[str, Any]:
    """Apply a conflict-aware, delta-only update and return fresh bulb state.

    This is the preferred API for a long-lived GUI/web app:

    1. Read the bulb immediately before the write.
    2. If ``expected_state_token`` no longer matches, perform no write and
       raise :class:`StateConflictError` with the freshly-read state attached.
    3. Build a PilotBuilder from *only* the fields explicitly supplied here,
       never from an entire cached UI model.
    4. Read the bulb again after the command and return that actual state.

    The WiZ LAN protocol has no atomic compare-and-swap operation, so another
    controller can still write in the tiny interval between step 1 and step 3.
    The readback in step 4 makes that result visible rather than silently
    assuming our command won.
    """
    pilot_kwargs: Dict[str, Any] = {}
    if brightness is not None:
        pilot_kwargs["brightness"] = brightness
    if rgb is not None:
        pilot_kwargs["rgb"] = rgb
    if colortemp is not None:
        pilot_kwargs["colortemp"] = colortemp
    if scene is not None:
        pilot_kwargs["scene"] = scene
    if speed is not None:
        pilot_kwargs["speed"] = speed

    if power is None and not pilot_kwargs:
        raise ValueError("apply_update requires at least one change")
    if power is False and pilot_kwargs:
        raise ValueError("a turn-off update cannot also contain light-mode changes")

    async with get_bulb(ip) as bulb:
        current = await _get_status_info_from_bulb(bulb, ip)
        _check_expected_state(expected_state_token, current)

        # Avoid a no-op power write. In particular, sending another ON command
        # can re-assert WiZ rhythm behavior even though the bulb was already on.
        if not pilot_kwargs and power is not None and current["power"] is power:
            return current

        if power is False:
            await bulb.turn_off()
        else:
            # turn_on adds only state=true to these explicitly requested fields.
            # That keeps unrelated scene/RGB/temperature values out of the write.
            await bulb.turn_on(PilotBuilder(**pilot_kwargs))

        return await _get_status_info_from_bulb(bulb, ip)


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
