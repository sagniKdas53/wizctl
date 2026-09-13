"""Extract dominant colors from images for use with WiZ bulbs."""

from dataclasses import dataclass
from pathlib import Path
from typing import List

from PIL import Image, UnidentifiedImageError


class PaletteError(ValueError):
    """Raised when an image cannot produce a usable palette."""


@dataclass(frozen=True)
class PaletteColor:
    """A dominant RGB color and its share of sampled pixels."""

    red: int
    green: int
    blue: int
    pixels: int
    percentage: float

    @property
    def hex(self) -> str:
        """Return the color in wizctl's accepted hexadecimal form."""
        return f"#{self.red:02x}{self.green:02x}{self.blue:02x}"


def extract_palette(image_path: str, colors: int = 6) -> List[PaletteColor]:
    """Return up to ``colors`` dominant colors from an image.

    The image is reduced before quantization so very large photos remain quick to
    process. Fully transparent pixels do not affect the result.
    """
    if not 1 <= colors <= 16:
        raise PaletteError("palette size must be between 1 and 16")

    path = Path(image_path)
    if not path.is_file():
        raise PaletteError(f"image not found: {image_path}")

    try:
        with Image.open(path) as source:
            source.thumbnail((512, 512))
            image = source.convert("RGBA")
    except (OSError, UnidentifiedImageError) as exc:
        raise PaletteError(f"cannot read image '{image_path}': {exc}") from exc

    raw_pixels = image.tobytes()
    visible_pixels = [
        (raw_pixels[index], raw_pixels[index + 1], raw_pixels[index + 2])
        for index in range(0, len(raw_pixels), 4)
        if raw_pixels[index + 3] > 0
    ]
    if not visible_pixels:
        raise PaletteError(f"image has no visible pixels: {image_path}")

    sampled = Image.new("RGB", (len(visible_pixels), 1))
    sampled.putdata(visible_pixels)
    quantized = sampled.quantize(colors=colors, method=Image.Quantize.MEDIANCUT)
    entries = quantized.getcolors(maxcolors=colors)
    if not entries:
        raise PaletteError(f"could not extract a palette from: {image_path}")

    palette = quantized.getpalette()
    total = len(visible_pixels)
    result = []
    for pixels, index in sorted(entries, reverse=True):
        offset = index * 3
        red, green, blue = palette[offset:offset + 3]
        result.append(
            PaletteColor(red, green, blue, pixels, pixels * 100 / total)
        )
    return result


def format_palette(image_path: str, palette: List[PaletteColor]) -> str:
    """Format a palette with ready-to-run wizctl commands."""
    lines = [f"Palette from {image_path}:"]
    for number, color in enumerate(palette, start=1):
        lines.append(
            f"  {number}. {color.hex}  {color.percentage:5.1f}%  "
            f"wizctl color '{color.hex}'"
        )
    return "\n".join(lines)
