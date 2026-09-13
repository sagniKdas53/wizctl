"""Tests for interactive panel widget, genmon output, and panel click handling."""

import tempfile
import time
import tkinter as tk
from unittest.mock import AsyncMock, patch
import pytest

from wizctl.genmon import generate_genmon_xml
from wizctl.widget import WizctlWidget, handle_panel_click


@pytest.fixture
def tk_root():
    root = tk.Tk()
    root.withdraw()
    yield root
    try:
        root.destroy()
    except Exception:
        pass


def test_widget_initialization(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlWidget, "ping_bulb"):
            widget = WizctlWidget(tk_root, target_ip="192.168.0.102")
            assert widget.power_btn is not None
            assert widget.bright_slider is not None
            assert len(widget.scene_buttons) == 4
            assert widget.is_pinned is False

            widget._toggle_pin()
            assert widget.is_pinned is True

            widget._on_close()


def test_widget_controls(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlWidget, "ping_bulb"):
            widget = WizctlWidget(tk_root, target_ip="192.168.0.102")

            def mock_submit(coro, on_success=None, on_error=None):
                coro.close()

            with patch.object(widget.worker, "submit", side_effect=mock_submit) as mock_sub:
                widget.set_brightness(128)
                assert widget.state["brightness"] == 128
                assert "50%" in widget.bright_label.cget("text")
                assert mock_sub.called

            with patch.object(widget.worker, "submit", side_effect=mock_submit) as mock_sub:
                widget.set_kelvin(4000)
                assert widget.state["kelvin"] == 4000
                assert mock_sub.called

            with patch.object(widget.worker, "submit", side_effect=mock_submit) as mock_sub:
                widget.set_scene(6, "Cozy")
                assert widget.state["scene_id"] == 6
                assert mock_sub.called

            with patch.object(widget.worker, "submit", side_effect=mock_submit) as mock_sub:
                widget.set_color_hex("#00ff88")
                assert widget.state["hex"] == "#00ff88"
                assert mock_sub.called

            widget._on_close()


def test_genmon_xml_generation(tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        xml = generate_genmon_xml(target_ip="192.168.0.102")
        assert "<img>" in xml
        assert "<txt>" in xml
        assert "<tool>" in xml
        assert "<click>wizctl widget --click</click>" in xml
        assert "192.168.0.102" in xml


def test_handle_panel_click_single_vs_double():
    with patch("wizctl.widget.run_widget") as mock_widget:
        with patch("wizctl.bulb.command_toggle", new_callable=AsyncMock) as mock_toggle:
            # First call: simulate single click (no fast second click)
            with patch("time.sleep"):
                res = handle_panel_click(target_ip="192.168.0.102")
                assert mock_widget.called
                assert not mock_toggle.called
