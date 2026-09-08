"""Tests for bulb state synchronization, on-reconnect handling, and Kelvin conversion."""

import tkinter as tk
from unittest.mock import AsyncMock, MagicMock, patch
import pytest

from wizctl.bulb import apply_saved_state, kelvin_to_rgb
from wizctl.gui import WizctlGUI
from wizctl.state import sanitize_state


@pytest.fixture
def tk_root():
    root = tk.Tk()
    root.withdraw()
    yield root
    try:
        root.destroy()
    except Exception:
        pass


def test_kelvin_to_rgb():
    """Verify Kelvin conversion for candle, warm, neutral, and daylight."""
    candle_rgb = kelvin_to_rgb(2200)
    assert candle_rgb[0] == 255
    assert 130 <= candle_rgb[1] <= 160
    assert candle_rgb[2] < 60

    warm_rgb = kelvin_to_rgb(2700)
    assert warm_rgb[0] == 255
    assert 150 <= warm_rgb[1] <= 180
    assert 70 <= warm_rgb[2] <= 110

    neutral_rgb = kelvin_to_rgb(4000)
    assert neutral_rgb[0] == 255
    assert 190 <= neutral_rgb[1] <= 225
    assert 150 <= neutral_rgb[2] <= 180

    daylight_rgb = kelvin_to_rgb(6500)
    assert daylight_rgb[0] == 255
    assert 240 <= daylight_rgb[1] <= 255
    assert 240 <= daylight_rgb[2] <= 255


def test_restore_on_reconnect_state_sanitization():
    """Verify restore_on_reconnect is preserved in state sanitization."""
    raw = {"restore_on_reconnect": True}
    clean = sanitize_state(raw)
    assert clean["restore_on_reconnect"] is True

    raw_false = {"restore_on_reconnect": False}
    clean_false = sanitize_state(raw_false)
    assert clean_false["restore_on_reconnect"] is False

    raw_invalid = {"restore_on_reconnect": "invalid"}
    clean_invalid = sanitize_state(raw_invalid)
    assert clean_invalid["restore_on_reconnect"] is False


@pytest.mark.asyncio
async def test_apply_saved_state_on(mock_wizlight):
    with patch("wizctl.bulb.get_bulb") as mock_get:
        mock_get.return_value.__aenter__.return_value = mock_wizlight

        state = {
            "power": True,
            "brightness": 200,
            "mode": "color",
            "rgb": [255, 0, 128],
            "hex": "#ff0080",
        }
        await apply_saved_state("192.168.1.100", state)
        assert mock_wizlight.turn_on.called


@pytest.mark.asyncio
async def test_apply_saved_state_off(mock_wizlight):
    with patch("wizctl.bulb.get_bulb") as mock_get:
        mock_get.return_value.__aenter__.return_value = mock_wizlight

        state = {"power": False}
        await apply_saved_state("192.168.1.100", state)
        assert mock_wizlight.turn_off.called


def test_reconnect_syncs_ui_from_bulb(tk_root, tmp_path):
    """When bulb was offline and comes online, UI should adopt live hardware state."""
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")
            app.is_online = False
            app.state["restore_on_reconnect"] = False

            live_status = {
                "ip": "192.168.1.100",
                "mac": "123456789abc",
                "rssi": -55,
                "power": True,
                "brightness": 191,  # 75%
                "colortemp": 4000,
                "rgb": None,
                "scene_id": 0,
                "scene": None,
            }

            app._apply_bulb_status(live_status, elapsed_ms=18)

            assert app.is_online is True
            assert app.state["power"] is True
            assert app.state["brightness"] == 191
            assert app.state["kelvin"] == 4000
            assert "75%" in app.brightness_label.cget("text")
            assert "4000 K" in app.kelvin_label.cget("text")
            assert "Bulb online" in app.activity_bar.cget("text")

            app._on_close()


def test_reconnect_pushes_saved_preset_when_configured(tk_root, tmp_path):
    """When restore_on_reconnect is True, reconnecting should push saved state."""
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")
            app.is_online = False
            app.state["restore_on_reconnect"] = True
            app.state["scene_id"] = 3  # Sunset

            with patch.object(app.worker, "submit") as mock_submit:
                live_status = {
                    "ip": "192.168.1.100",
                    "mac": "123456789abc",
                    "rssi": -55,
                    "power": True,
                    "brightness": 255,
                    "colortemp": 2700,
                    "rgb": None,
                    "scene_id": 0,
                }
                app._apply_bulb_status(live_status, elapsed_ms=20)

                assert mock_submit.called
                assert "restoring saved preset" in app.activity_bar.cget("text")

            app._on_close()
