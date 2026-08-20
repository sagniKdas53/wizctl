"""Command-line interface and entry points for wizctl."""

import argparse
import asyncio
import sys
from typing import List, Optional

from pywizlight.exceptions import WizLightConnectionError, WizLightTimeOutError

from wizctl import __version__
from wizctl.bulb import (
    DEFAULT_BULB_IP,
    command_brightness,
    command_color,
    command_kelvin,
    command_off,
    command_on,
    command_scene,
    command_scenes,
    command_status,
    command_toggle,
)
from wizctl.parsers import die


def create_parser() -> argparse.ArgumentParser:
    """Create and configure command-line argument parser."""
    parser = argparse.ArgumentParser(
        prog="wizctl",
        description="Control WiZ smart light bulbs over the local LAN",
    )

    parser.add_argument(
        "-v", "--version",
        action="version",
        version=f"%(prog)s {__version__}",
    )

    parser.add_argument(
        "--ip",
        default=DEFAULT_BULB_IP,
        help=f"IP address of the WiZ bulb (default: {DEFAULT_BULB_IP})",
    )

    sub = parser.add_subparsers(
        dest="command",
        required=True,
    )

    sub.add_parser("on", help="turn the bulb on")
    sub.add_parser("off", help="turn the bulb off")
    sub.add_parser("toggle", help="toggle bulb power state")
    sub.add_parser("status", help="show bulb status")

    color = sub.add_parser("color", help="set RGB color")
    color.add_argument(
        "value",
        help="color name (e.g. red, cyan, warmwhite), hex (#ff5500, #f50), or RGB (255,128,0)",
    )

    brightness = sub.add_parser("brightness", help="set brightness")
    brightness.add_argument(
        "value",
        help="0-255 or 0%-100% (e.g. 128, 50%)",
    )

    kelvin = sub.add_parser("kelvin", help="set color temperature in Kelvin")
    kelvin.add_argument(
        "value",
        type=int,
        help="temperature in Kelvin (e.g. 2700, 4000, 6500)",
    )

    scene = sub.add_parser("scene", help="set WiZ scene")
    scene.add_argument(
        "value",
        help="scene ID (1-36) or scene name (e.g. cozy, sunset, ocean)",
    )

    sub.add_parser("scenes", help="list all available WiZ scenes")

    return parser


async def async_main(argv: Optional[List[str]] = None) -> int:
    """Async main logic."""
    parser = create_parser()
    args = parser.parse_args(argv)

    ip = args.ip

    try:
        if args.command == "on":
            await command_on(ip)
        elif args.command == "off":
            await command_off(ip)
        elif args.command == "toggle":
            await command_toggle(ip)
        elif args.command == "status":
            await command_status(ip)
        elif args.command == "color":
            await command_color(ip, args.value)
        elif args.command == "brightness":
            await command_brightness(ip, args.value)
        elif args.command == "kelvin":
            await command_kelvin(ip, args.value)
        elif args.command == "scene":
            await command_scene(ip, args.value)
        elif args.command == "scenes":
            command_scenes()
        return 0

    except (WizLightTimeOutError, asyncio.TimeoutError):
        die(f"request to bulb at {ip} timed out (check power and network connection)")
        return 1
    except WizLightConnectionError as exc:
        die(f"connection error with bulb at {ip}: {exc}")
        return 1
    except OSError as exc:
        die(f"network error: {exc}")
        return 1
    except ValueError as exc:
        die(str(exc))
        return 1
    except RuntimeError as exc:
        die(str(exc))
        return 1
    except KeyboardInterrupt:
        return 130
    except Exception as exc:
        die(str(exc))
        return 1


def main(argv: Optional[List[str]] = None) -> int:
    """Synchronous CLI entry point returning exit code."""
    try:
        return asyncio.run(async_main(argv))
    except KeyboardInterrupt:
        return 130


def entry_point() -> None:
    """Console script entry point."""
    sys.exit(main())


if __name__ == "__main__":
    entry_point()
