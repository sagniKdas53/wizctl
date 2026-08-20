"""Unit tests for wizctl bulb control functions (using mocks)."""

from unittest.mock import patch

import pytest
from pywizlight.exceptions import WizLightConnectionError, WizLightTimeOutError

from wizctl.bulb import (
    command_brightness,
    command_color,
    command_kelvin,
    command_off,
    command_on,
    command_scene,
    command_scenes,
    command_status,
    command_toggle,
    get_status_info,
)


@pytest.mark.asyncio
async def test_get_status_info(mock_wizlight):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        info = await get_status_info("192.168.0.102")
        assert info["ip"] == "192.168.0.102"
        assert info["mac"] == "cc4085e299f4"
        assert info["power"] is True
        assert info["brightness"] == 255
        assert info["rgb"] == (255, 0, 0)
        assert mock_wizlight.async_close.called


@pytest.mark.asyncio
async def test_command_status(mock_wizlight, capsys):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        await command_status("192.168.0.102")
        captured = capsys.readouterr()
        assert "Bulb:       192.168.0.102" in captured.out
        assert "MAC:        cc4085e299f4" in captured.out
        assert "Power:      ON" in captured.out
        assert "Brightness: 255/255 (100%)" in captured.out
        assert "RGB:        255,0,0 (#ff0000)" in captured.out


@pytest.mark.asyncio
async def test_command_on(mock_wizlight, capsys):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        await command_on("192.168.0.102")
        assert mock_wizlight.turn_on.called
        assert mock_wizlight.async_close.called
        captured = capsys.readouterr()
        assert "✓ ON" in captured.out


@pytest.mark.asyncio
async def test_command_off(mock_wizlight, capsys):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        await command_off("192.168.0.102")
        assert mock_wizlight.turn_off.called
        assert mock_wizlight.async_close.called
        captured = capsys.readouterr()
        assert "✓ OFF" in captured.out


@pytest.mark.asyncio
async def test_command_toggle_on_to_off(mock_wizlight, capsys):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        new_state = await command_toggle("192.168.0.102")
        assert new_state is False
        assert mock_wizlight.turn_off.called
        captured = capsys.readouterr()
        assert "✓ OFF (toggled)" in captured.out


@pytest.mark.asyncio
async def test_command_color(mock_wizlight, capsys):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        rgb = await command_color("192.168.0.102", "blue")
        assert rgb == (0, 0, 255)
        assert mock_wizlight.turn_on.called
        captured = capsys.readouterr()
        assert "✓ RGB(0, 0, 255) (#0000ff)" in captured.out


@pytest.mark.asyncio
async def test_command_brightness(mock_wizlight, capsys):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        b = await command_brightness("192.168.0.102", "50%")
        assert b == 128
        assert mock_wizlight.turn_on.called
        captured = capsys.readouterr()
        assert "✓ Brightness 128/255 (50%)" in captured.out


@pytest.mark.asyncio
async def test_command_kelvin(mock_wizlight, capsys):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        k = await command_kelvin("192.168.0.102", 3000)
        assert k == 3000
        assert mock_wizlight.turn_on.called
        captured = capsys.readouterr()
        assert "✓ 3000K" in captured.out


@pytest.mark.asyncio
async def test_command_kelvin_invalid(mock_wizlight):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        with pytest.raises(ValueError):
            await command_kelvin("192.168.0.102", 500)


@pytest.mark.asyncio
async def test_command_scene(mock_wizlight, capsys):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        sid, sname = await command_scene("192.168.0.102", "cozy")
        assert sid == 6
        assert sname == "Cozy"
        assert mock_wizlight.turn_on.called
        captured = capsys.readouterr()
        assert "✓ Scene 6 (Cozy)" in captured.out


def test_command_scenes(capsys):
    command_scenes()
    captured = capsys.readouterr()
    assert "Available WiZ Scenes:" in captured.out
    assert "Sunset" in captured.out
