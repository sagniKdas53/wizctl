"""Unit tests for state persistence."""

import json
from pathlib import Path
from unittest.mock import patch

from wizctl.state import (
    DEFAULT_PRESET_COLORS,
    DEFAULT_STATE,
    get_state_file_path,
    load_state,
    save_state,
)


def test_default_state_structure():
    assert "ip" in DEFAULT_STATE
    assert "power" in DEFAULT_STATE
    assert "brightness" in DEFAULT_STATE
    assert "rgb" in DEFAULT_STATE
    assert "hex" in DEFAULT_STATE
    assert "kelvin" in DEFAULT_STATE
    assert "recent_colors" in DEFAULT_STATE
    assert len(DEFAULT_PRESET_COLORS) > 0


def test_save_and_load_state(tmp_path):
    mock_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_file):
        test_state = {
            "ip": "192.168.1.123",
            "power": False,
            "brightness": 128,
            "rgb": [0, 255, 128],
            "hex": "#00ff80",
            "kelvin": 4000,
            "scene_id": 3,
            "mode": "color",
            "recent_colors": ["#00ff80", "#ff0000"],
        }
        save_state(test_state)

        loaded = load_state()
        assert loaded["ip"] == "192.168.1.123"
        assert loaded["power"] is False
        assert loaded["brightness"] == 128
        assert loaded["rgb"] == [0, 255, 128]
        assert loaded["hex"] == "#00ff80"
        assert loaded["kelvin"] == 4000
        assert loaded["scene_id"] == 3


def test_corrupt_state_fallback(tmp_path):
    mock_file = tmp_path / "state.json"
    mock_file.write_text("invalid json content {{{", encoding="utf-8")
    with patch("wizctl.state.get_state_file_path", return_value=mock_file):
        loaded = load_state()
        assert loaded["brightness"] == DEFAULT_STATE["brightness"]
        assert loaded["rgb"] == DEFAULT_STATE["rgb"]


def test_env_ip_override(tmp_path, monkeypatch):
    mock_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_file):
        test_state = {"ip": "192.168.1.50"}
        save_state(test_state)

        monkeypatch.setenv("WIZ_IP", "10.0.0.99")
        loaded = load_state()
        assert loaded["ip"] == "10.0.0.99"


def test_sanitize_state_out_of_bounds(tmp_path):
    mock_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_file):
        # Save unsafe/corrupted values
        bad_state = {
            "ip": "255.255.255.255",  # Broadcast rejected
            "brightness": 9999,        # Out of bounds
            "rgb": [999, -50, 300],    # Out of bounds
            "kelvin": 50,              # Out of bounds
            "scene_id": 999,           # Out of bounds
            "recent_colors": ["invalid_color", "#123456"],
        }
        save_state(bad_state)

        loaded = load_state()
        # IP falls back to default
        assert loaded["ip"] == DEFAULT_STATE["ip"]
        # RGB clamped to 0-255
        assert loaded["rgb"] == [255, 0, 255]
        # Kelvin falls back to default
        assert loaded["kelvin"] == DEFAULT_STATE["kelvin"]
        # Scene ID falls back to default
        assert loaded["scene_id"] == DEFAULT_STATE["scene_id"]
        # Valid recent color preserved
        assert loaded["recent_colors"] == ["#123456"]

