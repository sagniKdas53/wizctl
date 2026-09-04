"""Unit tests for wizctl Tkinter GUI components."""

import asyncio
import threading
import time
import tkinter as tk
from unittest.mock import AsyncMock, MagicMock, patch
import pytest

from wizctl.gui import AsyncBulbWorker, ColorWheelCanvas, WizctlGUI


@pytest.fixture
def tk_root():
    root = tk.Tk()
    root.withdraw()
    yield root
    try:
        root.destroy()
    except Exception:
        pass


def test_color_wheel_canvas(tk_root):
    colors_changed = []
    colors_released = []

    wheel = ColorWheelCanvas(
        tk_root,
        size=150,
        on_color_change=lambda rgb, h: colors_changed.append((rgb, h)),
        on_color_release=lambda rgb, h: colors_released.append((rgb, h)),
    )
    assert wheel.current_rgb == (255, 140, 0)
    assert wheel.current_hex == "#ff8c00"

    # Set new RGB programmatically
    wheel.set_rgb((0, 255, 0), notify=True)
    assert wheel.current_rgb == (0, 255, 0)
    assert wheel.current_hex == "#00ff00"
    assert len(colors_changed) == 1
    assert colors_changed[0] == ((0, 255, 0), "#00ff00")


def test_async_bulb_worker():
    worker = AsyncBulbWorker()

    results = []
    errors = []
    done_event = threading.Event()

    async def sample_coro():
        await asyncio.sleep(0.01)
        return "success_val"

    worker.submit(
        sample_coro(),
        on_success=lambda res: (results.append(res), done_event.set()),
        on_error=lambda exc: (errors.append(exc), done_event.set()),
    )

    assert done_event.wait(timeout=2.0)
    assert results == ["success_val"]
    assert errors == []

    worker.stop()


def test_gui_initialization_and_ping(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")
            assert app.ip_entry.get() == "192.168.1.100"
            assert app.power_btn is not None
            assert app.color_wheel is not None
            assert app.bright_slider is not None
            assert app.kelvin_slider is not None
            app._on_close()


def test_gui_apply_bulb_status(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")

            status_info = {
                "ip": "192.168.1.100",
                "mac": "a1b2c3d4e5f6",
                "rssi": -65,
                "power": True,
                "brightness": 128,
                "rgb": (0, 128, 255),
                "colortemp": 3000,
                "scene": "Cozy",
                "scene_id": 6,
            }

            app._apply_bulb_status(status_info, elapsed_ms=25)

            assert app.is_online is True
            assert "Online (25ms)" in app.status_label.cget("text")
            assert "-65 dBm" in app.signal_label.cget("text")
            assert app.state["power"] is True
            assert app.state["brightness"] == 128
            assert app.state["rgb"] == [0, 128, 255]
            assert app.state["kelvin"] == 3000
            assert app.state["hex"] == "#0080ff"
            assert app.hex_entry.get() == "#0080ff"
            assert "50% (128/255)" in app.brightness_label.cget("text")

            app._on_close()


def test_gui_apply_bulb_offline(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")

            app._apply_bulb_offline("Connection timed out")

            assert app.is_online is False
            assert "Offline" in app.status_label.cget("text")
            assert "Connection failed" in app.activity_bar.cget("text")

            app._on_close()


def test_gui_color_wheel_interactions(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")

            with patch.object(app, "_send_color") as mock_send:
                app._on_wheel_color_drag((255, 0, 0), "#ff0000")
                assert app.state["hex"] == "#ff0000"
                assert app.hex_entry.get() == "#ff0000"
                assert mock_send.called

            with patch.object(app, "_send_color") as mock_send:
                app._on_wheel_color_release((0, 255, 0), "#00ff00")
                assert app.state["hex"] == "#00ff00"
                assert "#00ff00" in app.state["recent_colors"]
                assert mock_send.called

            app._on_close()


def test_gui_brightness_and_kelvin_controls(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")

            with patch.object(app, "_send_brightness") as mock_b:
                app.set_brightness(64)
                assert app.state["brightness"] == 64
                assert "25%" in app.brightness_label.cget("text")
                mock_b.assert_called_with(64)

            with patch.object(app, "_send_kelvin") as mock_k:
                app.set_kelvin(4000)
                assert app.state["kelvin"] == 4000
                assert "4000 K" in app.kelvin_label.cget("text")
                mock_k.assert_called_with(4000)

            with patch.object(app, "_send_brightness"):
                app._on_brightness_slider("200")
                assert app.state["brightness"] == 200

            with patch.object(app, "_send_kelvin"):
                app._on_kelvin_slider("3500")
                assert app.state["kelvin"] == 3500

            app._on_close()


def test_gui_scene_and_power_toggle(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")

            def mock_submit(coro, on_success=None, on_error=None):
                coro.close()

            with patch.object(app.worker, "submit", side_effect=mock_submit) as mock_s:
                app.set_scene(3, "Sunset")
                assert app.state["scene_id"] == 3
                assert mock_s.called

            with patch.object(app.worker, "submit", side_effect=mock_submit) as mock_s:
                init_power = app.state.get("power", True)
                app.toggle_power()
                assert app.state["power"] == (not init_power)
                assert mock_s.called

            app._on_close()


def test_gui_palette_picker_load_image(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")

            # Create a simple test image
            from PIL import Image
            test_img_path = tmp_path / "test.png"
            img = Image.new("RGB", (100, 100), color=(255, 0, 128))
            img.save(test_img_path)

            app.load_image_palette(str(test_img_path), colors=4)

            assert len(app._current_palette) > 0
            assert app._current_image_path == str(test_img_path)
            assert len(app.palette_swatches_frame.winfo_children()) == len(app._current_palette)

            # Test clicking a swatch
            first_color = app._current_palette[0]
            with patch.object(app, "_send_color") as mock_send:
                app._on_palette_swatch_click(first_color)
                assert app.state["hex"] == first_color.hex
                assert first_color.hex in app.state["recent_colors"]
                assert mock_send.called

            app._on_close()


def test_gui_palette_picker_missing_image(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")

            app.load_image_palette(str(tmp_path / "non_existent.jpg"))
            assert "Image not found" in app.activity_bar.cget("text")

            app._on_close()


def test_parse_dropped_paths(tmp_path):
    from wizctl.gui import parse_dropped_paths

    assert parse_dropped_paths("") == []
    assert parse_dropped_paths("/home/user/photo.jpg") == ["/home/user/photo.jpg"]
    assert parse_dropped_paths("//home/user/photo.jpg") == ["/home/user/photo.jpg"]
    assert parse_dropped_paths("file:///home/user/my%20photo.png") == ["/home/user/my photo.png"]
    assert parse_dropped_paths("{/home/user/my photo.png}") == ["/home/user/my photo.png"]


def test_gui_drag_and_drop_event(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")

            from PIL import Image
            test_img_path = tmp_path / "dropped.png"
            img = Image.new("RGB", (60, 60), color=(0, 200, 100))
            img.save(test_img_path)

            class MockDropEvent:
                data = str(test_img_path)
                action = "copy"

            app._on_drop_event(MockDropEvent())

            assert app._current_image_path == str(test_img_path)
            assert len(app._current_palette) > 0

            app._on_close()


def test_gui_detects_external_changes(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")
            app.state["power"] = True
            app.state["scene_id"] = 6  # Cozy
            app.state["brightness"] = 255

            # Simulate external change from WiZclick (toggled twice to Night light with lower brightness)
            external_status = {
                "ip": "192.168.1.100",
                "mac": "a1b2c3d4e5f6",
                "rssi": -60,
                "power": True,
                "brightness": 26,  # 10%
                "scene": "Night light",
                "scene_id": 14,
            }

            app._apply_bulb_status(external_status, elapsed_ms=15)

            assert app.state["scene_id"] == 14
            assert app.state["brightness"] == 26
            assert "⚡ Live update" in app.activity_bar.cget("text")
            assert "Night light" in app.activity_bar.cget("text")

            app._on_close()


def test_gui_power_toggle_stale_state_reconciliation(tk_root, tmp_path):
    mock_state_file = tmp_path / "state.json"
    with patch("wizctl.state.get_state_file_path", return_value=mock_state_file):
        with patch.object(WizctlGUI, "ping_bulb"):
            app = WizctlGUI(tk_root, target_ip="192.168.1.100")
            # GUI thinks bulb is ON
            app.state["power"] = True

            # In background, bulb was actually turned OFF externally
            # When toggle_power runs and worker succeeds, test callback reconciliation
            submitted_coros = []

            def mock_submit(coro, on_success=None, on_error=None):
                submitted_coros.append(coro)
                # Call on_success with (target_power=True, live_power=False)
                # because the live state was OFF, so toggling turned it ON
                if on_success:
                    on_success((True, False))

            with patch.object(app.worker, "submit", side_effect=mock_submit):
                app.toggle_power()
                assert app.state["power"] is True
                assert "Synced external state (OFF) → toggled ON" in app.activity_bar.cget("text")

            for c in submitted_coros:
                c.close()
            app._on_close()




