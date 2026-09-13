"""Tests for image palette extraction."""

from PIL import Image

from wizctl.palette import extract_palette, format_palette
from wizctl.palette_tui import move_selection, render_picker


def test_extract_palette_returns_dominant_colors(tmp_path):
    image_path = tmp_path / "colors.png"
    image = Image.new("RGB", (4, 1))
    image.putdata([(255, 0, 0), (255, 0, 0), (0, 0, 255), (0, 0, 255)])
    image.save(image_path)

    palette = extract_palette(str(image_path), colors=2)

    assert {color.hex for color in palette} == {"#ff0000", "#0000ff"}
    assert sum(color.pixels for color in palette) == 4


def test_extract_palette_ignores_transparent_pixels(tmp_path):
    image_path = tmp_path / "transparent.png"
    image = Image.new("RGBA", (2, 1))
    image.putdata([(255, 0, 0, 255), (0, 0, 255, 0)])
    image.save(image_path)

    palette = extract_palette(str(image_path), colors=1)

    assert palette[0].hex == "#ff0000"


def test_format_palette_includes_ready_to_use_command(tmp_path):
    image_path = tmp_path / "red.png"
    image = Image.new("RGB", (1, 1), (255, 0, 0))
    image.save(image_path)

    formatted = format_palette(str(image_path), extract_palette(str(image_path), 1))

    assert "1. #ff0000" in formatted
    assert "wizctl color '#ff0000'" in formatted


def test_picker_renders_a_true_color_swatch(tmp_path):
    image_path = tmp_path / "red.png"
    Image.new("RGB", (1, 1), (255, 0, 0)).save(image_path)
    palette = extract_palette(str(image_path), 1)

    picker = render_picker(str(image_path), palette, selected=0, ip="192.168.0.50")

    assert "\033[48;2;255;0;0m" in picker
    assert "Enter: apply" in picker


def test_picker_navigation_wraps_and_quits():
    assert move_selection(0, "\033[A", 3) == 2
    assert move_selection(2, "\033[B", 3) == 0
    assert move_selection(1, "x", 3) == 1
    assert move_selection(1, "q", 3) is None
