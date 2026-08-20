# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`wizctl` is a Python CLI and library for controlling WiZ Connected smart light bulbs directly over LAN via UDP (using `pywizlight`) — no cloud or bridge required. It's distributed both as a pip-installable package and as a standalone PyInstaller binary.

## Commands

```bash
make install-dev      # create .venv and install package + dev deps (pytest, pyinstaller, build)
make test              # run unit test suite: .venv/bin/pytest -v
make test-live         # run integration tests against a real bulb: pytest -v --live tests/test_integration.py
make build             # build wheel/sdist: python -m build
make binary            # compile standalone binary to dist/wizctl via pyinstaller
make clean             # remove build/, dist/, *.egg-info/, .pytest_cache/, *.spec, __pycache__
```

Run a single test: `.venv/bin/pytest -v tests/test_parsers.py::test_name`

There's no separate lint/format command configured — none of pyproject.toml, Makefile, or CI define one.

Local dev without installing: `python wizctl.py <command>` (adjusts `sys.path` to prefer `src/` over the repo root) or `python -m wizctl <command>` once installed in editable mode.

The default target bulb IP is `192.168.0.102`, overridable via `--ip`, `WIZ_IP`, or `BULB_IP` (see `DEFAULT_BULB_IP` in `src/wizctl/bulb.py`).

## Architecture

Everything lives under `src/wizctl/`, split strictly by responsibility:

- **`cli.py`** — argparse setup and the async main loop only. Maps each subcommand to a `bulb.py` function and translates exceptions (`WizLightTimeOutError`, `WizLightConnectionError`, `OSError`, `ValueError`, `RuntimeError`) into user-facing errors via `parsers.die()`, returning appropriate exit codes. Contains no bulb-communication or parsing logic itself.
- **`bulb.py`** — all `pywizlight` interaction. `get_bulb(ip)` is an async context manager that guarantees `wizlight.async_close()` cleanup; every `command_*` function opens a fresh connection through it rather than holding a long-lived bulb object. Each `command_*` function both performs the action and prints its own `✓ ...` confirmation line — this dual responsibility (side effect + user-facing output) is intentional and mirrored in tests.
- **`parsers.py`** — pure, synchronous input-parsing functions (`parse_color`, `parse_brightness`, `parse_scene`) that raise `ValueError` on bad input. No I/O, no bulb dependency, fully unit-testable in isolation. `die()` (print to stderr + `sys.exit`) also lives here.
- **`colors.py`** — static `COLORS: Dict[str, Tuple[int,int,int]]` name→RGB table consumed by `parsers.parse_color`.

Data flow for every command: `cli.py` parses argv → calls a `parsers.py` function to validate/convert the raw string → calls a `bulb.py` command function that opens a connection via `get_bulb()`, sends a `PilotBuilder(...)` payload, and prints the result.

`wizctl.py` at the repo root is a thin launcher for running from a checkout without installing — it forcibly removes the repo root and `""` from `sys.path` and prepends `src/`, specifically to avoid a module named `wizctl.py` shadowing the `wizctl` package.

## Testing

- `tests/conftest.py` defines `MockPilotParser` (mimics pywizlight's state object) and the `mock_wizlight` fixture (a `MagicMock` with `AsyncMock` methods for `updateState`, `turn_on`, `turn_off`, `async_close`) — reuse these instead of building new mocks in individual test files.
- `tests/test_integration.py` is gated behind the `live` pytest marker and requires `--live` plus a real bulb reachable at the configured IP; it records the bulb's original state and restores it after the run. It's skipped by default (see `pytest_collection_modifyitems` in `conftest.py`).
- `asyncio_mode = "auto"` is set in `pyproject.toml`, so async test functions don't need `@pytest.mark.asyncio`.
