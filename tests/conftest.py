"""Pytest configuration, fixtures, and mocks for wizctl tests."""

from typing import Any, Dict, Optional, Tuple
from unittest.mock import AsyncMock, MagicMock

import pytest


class MockPilotParser:
    """Mock implementation of pywizlight PilotParser."""

    def __init__(self, data: Optional[Dict[str, Any]] = None):
        self.pilotResult = data or {
            "mac": "cc4085e299f4",
            "rssi": -40,
            "state": True,
            "sceneId": 0,
            "r": 255,
            "g": 0,
            "b": 0,
            "c": 0,
            "w": 0,
            "dimming": 100,
        }

    def get_state(self) -> Optional[bool]:
        return self.pilotResult.get("state")

    def get_mac(self) -> Optional[str]:
        return self.pilotResult.get("mac")

    def get_brightness(self) -> Optional[int]:
        dim = self.pilotResult.get("dimming")
        return round(dim * 255 / 100) if dim is not None else None

    def get_scene(self) -> Optional[str]:
        from pywizlight import SCENES
        sid = self.get_scene_id()
        return SCENES.get(sid) if sid else None

    def get_scene_id(self) -> Optional[int]:
        sid = self.pilotResult.get("sceneId")
        return sid if sid and sid != 0 else None

    def get_rgb(self) -> Tuple[Optional[int], Optional[int], Optional[int]]:
        r = self.pilotResult.get("r")
        g = self.pilotResult.get("g")
        b = self.pilotResult.get("b")
        if r is not None and g is not None and b is not None:
            return (r, g, b)
        return (None, None, None)

    def get_colortemp(self) -> Optional[int]:
        return self.pilotResult.get("temp")


@pytest.fixture
def mock_pilot_parser():
    return MockPilotParser()


@pytest.fixture
def mock_wizlight(mock_pilot_parser):
    """Fixture providing a mocked wizlight instance."""
    bulb = MagicMock()
    bulb.ip = "192.168.0.102"
    bulb.updateState = AsyncMock(return_value=[mock_pilot_parser])
    bulb.turn_on = AsyncMock()
    bulb.turn_off = AsyncMock()
    bulb.lightSwitch = AsyncMock()
    bulb.async_close = AsyncMock()
    return bulb


def pytest_addoption(parser):
    parser.addoption(
        "--live",
        action="store_true",
        default=False,
        help="Run live integration tests against real bulb",
    )


def pytest_collection_modifyitems(config, items):
    if not config.getoption("--live"):
        skip_live = pytest.mark.skip(reason="Pass --live to run live device tests")
        for item in items:
            if "live" in item.keywords:
                item.add_marker(skip_live)
