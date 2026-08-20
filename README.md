# wizctl

A fast, lightweight CLI tool and Python library for controlling WiZ Connected smart light bulbs directly over the local area network (LAN) using UDP broadcast/unicast protocol without cloud dependencies.

---

## Features

- **Fast Local Control**: Directly controls WiZ bulbs over LAN UDP protocol (no cloud / bridge required).
- **Comprehensive Controls**: Power (`on`, `off`, `toggle`), brightness, RGB colors, color temperatures (Kelvin), and dynamic WiZ scenes.
- **Flexible Color Input**: Supports named colors (`warmwhite`, `red`, `cyan`, etc.), 6-digit hex (`#ff5500`), shorthand 3-digit hex (`#f50`), and RGB triples (`255, 128, 0`).
- **Scene Presets**: Switch scenes by name (`cozy`, `sunset`, `ocean`, `candlelight`) or ID (`1`-`36`, `40`), and list all scenes with `wizctl scenes`.
- **Standalone Binary**: Includes a standalone compiled binary executable with zero external runtime dependencies.
- **Robust Error Handling**: Automatic UDP transport cleanup, timeout detection, and device reachability checks.

---

## Project Structure

```
.
├── src/
│   └── wizctl/
│       ├── __init__.py      # Package metadata & version
│       ├── __main__.py      # python -m wizctl entry point
│       ├── cli.py           # CLI argument parsing & commands
│       ├── bulb.py          # WiZ communication & async connection manager
│       ├── colors.py        # Named colors dictionary & RGB mappings
│       └── parsers.py       # Color, brightness, and scene parsers
├── tests/
│   ├── conftest.py          # Pytest fixtures & mocks
│   ├── test_parsers.py      # Unit tests for input parsing
│   ├── test_cli.py          # Unit tests for CLI options & commands
│   ├── test_bulb.py         # Async unit tests with mocked WiZ device
│   └── test_integration.py  # Live device integration tests
├── dist/
│   └── wizctl               # Standalone compiled ELF binary
├── pyproject.toml           # PEP 517/518 build configuration & dependencies
├── Makefile                 # Developer build & test targets
├── LICENSE                  # MIT license
└── wizctl.py                # Direct repository launcher
```

---

## Quickstart

### 1. Setup Virtual Environment

```bash
# Clone or navigate to the repository
cd /path/to/light

# Create and activate virtual environment
python3 -m venv .venv
source .venv/bin/activate

# Install the package in editable mode
pip install -e ".[dev]"
```

Alternatively using the `Makefile`:
```bash
make install-dev
```

### 2. Basic Usage

By default, `wizctl` targets `192.168.0.102` (or the IP configured in `WIZ_IP` / `BULB_IP` environment variables).

```bash
# Check bulb status
wizctl status

# Turn bulb on / off / toggle
wizctl on
wizctl off
wizctl toggle

# Set brightness (percentage or 0-255)
wizctl brightness 80%
wizctl brightness 200

# Set color (name, hex, or RGB)
wizctl color blue
wizctl color #ff5500
wizctl color "255, 128, 0"
wizctl color warmwhite

# Set color temperature in Kelvin (2200K - 6500K)
wizctl kelvin 2700
wizctl kelvin 4000

# Set dynamic scene preset (by name or ID)
wizctl scene cozy
wizctl scene sunset
wizctl scene 1

# List all available WiZ scenes
wizctl scenes
```

---

## CLI Reference

```
usage: wizctl [-h] [-v] [--ip IP] {on,off,toggle,status,color,brightness,kelvin,scene,scenes} ...

positional arguments:
  on                    turn the bulb on
  off                   turn the bulb off
  toggle                toggle bulb power state
  status                show bulb status
  color                 set RGB color (name, #RRGGBB, #RGB, or R,G,B)
  brightness            set brightness (0-255 or 0%-100%)
  kelvin                set color temperature in Kelvin (e.g. 2700, 4000)
  scene                 set WiZ scene preset by name or ID (e.g. cozy, sunset, 1)
  scenes                list all available WiZ scenes

options:
  -h, --help            show this help message and exit
  -v, --version         show program's version number and exit
  --ip IP               IP address of the WiZ bulb (default: 192.168.0.102)
```

### Specifying Target Bulb IP

You can target a specific bulb using any of the following methods:

1. **CLI Flag**: `wizctl --ip 192.168.0.102 status`
2. **Environment Variable**: `export WIZ_IP=192.168.0.102` or `export BULB_IP=192.168.0.102`
3. **Default Config**: Default fallback is `192.168.0.102`.

---

## Standalone Compiled Binary

A standalone executable binary is generated in `dist/wizctl`. It requires no Python installation or virtual environment to run.

### Running the Binary

```bash
# Make sure binary is executable
chmod +x ./dist/wizctl

# Run commands directly
./dist/wizctl status
./dist/wizctl on
./dist/wizctl color red
./dist/wizctl scene cozy
```

### Rebuilding the Binary

To compile the standalone binary from source using PyInstaller:

```bash
make binary
```
Or directly:
```bash
.venv/bin/pyinstaller --onefile --clean --name wizctl --paths src src/wizctl/__main__.py
```

The compiled binary will be placed at `dist/wizctl`.

---

## Testing

The project includes unit tests with mocks and integration tests for physical devices:

### Run Unit Tests (125+ tests)

```bash
make test
# or
.venv/bin/pytest -v
```

### Run Live Device Integration Tests

To run the live test cycle against the physical bulb at `192.168.0.102`:

```bash
make test-live
# or
.venv/bin/pytest -v --live tests/test_integration.py
```

*Note: The live test records the bulb's initial power, brightness, and color state, performs the test suite, and automatically restores the device back to its original state.*

---

## Python API Usage

`wizctl` can also be used as a Python library:

```python
import asyncio
from wizctl.bulb import get_bulb, get_status_info, command_color
from pywizlight import PilotBuilder

async def main():
    # Fetch bulb status
    status = await get_status_info("192.168.0.102")
    print(f"Power: {status['power']}, Brightness: {status['brightness']}")

    # Set color
    await command_color("192.168.0.102", "cyan")

    # Or use async context manager for custom pilot controls
    async with get_bulb("192.168.0.102") as bulb:
        await bulb.turn_on(PilotBuilder(brightness=128, colortemp=2700))

asyncio.run(main())
```

---

## License

MIT License.
