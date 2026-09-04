"""Unit tests for wizctl bulb control functions (using mocks)."""

from unittest.mock import patch

import pytest
from pywizlight.exceptions import WizLightConnectionError, WizLightTimeOutError

from wizctl.bulb import (
    StateConflictError,
    apply_update,
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
    state_token_from_raw,
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
        assert info["state_token"]
        assert mock_wizlight.async_close.called


def test_state_token_ignores_telemetry():
    raw = {
        "state": True,
        "dimming": 50,
        "sceneId": 6,
        "rssi": -40,
    }
    changed_rssi = {**raw, "rssi": -70}
    changed_brightness = {**raw, "dimming": 60}

    assert state_token_from_raw(raw) == state_token_from_raw(changed_rssi)
    assert state_token_from_raw(raw) != state_token_from_raw(changed_brightness)


@pytest.mark.asyncio
async def test_apply_update_rejects_stale_state(mock_wizlight):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        with pytest.raises(StateConflictError) as excinfo:
            await apply_update(
                "192.168.0.102",
                expected_state_token="stale-token",
                brightness=128,
            )

    assert not mock_wizlight.turn_on.called
    assert not mock_wizlight.turn_off.called
    assert excinfo.value.current_state["power"] is True
    assert excinfo.value.current_state_token == excinfo.value.current_state["state_token"]


@pytest.mark.asyncio
async def test_apply_update_sends_only_requested_delta(mock_wizlight):
    # First updateState() is the pre-write check; the second is post-write readback.
    mock_wizlight.updateState.side_effect = [
        mock_wizlight.updateState.return_value,
        mock_wizlight.updateState.return_value,
    ]

    current = mock_wizlight.updateState.return_value[0]
    token = state_token_from_raw(current.pilotResult)

    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        result = await apply_update(
            "192.168.0.102",
            expected_state_token=token,
            brightness=128,
        )

    mock_wizlight.turn_on.assert_called_once()
    pilot = mock_wizlight.turn_on.call_args.args[0]
    assert pilot.pilot_params == {"brightness": 128}
    assert result["state_token"] == token


@pytest.mark.asyncio
async def test_apply_update_noop_power_does_not_send(mock_wizlight):
    current = mock_wizlight.updateState.return_value[0]
    token = state_token_from_raw(current.pilotResult)

    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        result = await apply_update(
            "192.168.0.102",
            expected_state_token=token,
            power=True,
        )

    assert result["power"] is True
    assert not mock_wizlight.turn_on.called
    assert not mock_wizlight.turn_off.called


@pytest.mark.asyncio
async def test_apply_update_turn_off_is_state_only(mock_wizlight):
    current = mock_wizlight.updateState.return_value[0]
    token = state_token_from_raw(current.pilotResult)

    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        await apply_update(
            "192.168.0.102",
            expected_state_token=token,
            power=False,
        )

    mock_wizlight.turn_off.assert_called_once_with()
    assert not mock_wizlight.turn_on.called


@pytest.mark.asyncio
async def test_apply_update_rejects_off_with_mode_change(mock_wizlight):
    with patch("wizctl.bulb.wizlight", return_value=mock_wizlight):
        with pytest.raises(ValueError):
            await apply_update(
                "192.168.0.102",
                power=False,
                brightness=128,
            )


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
