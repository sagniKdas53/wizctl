"""A small terminal picker for image palettes."""

from contextlib import contextmanager
import os
import select
import sys
from typing import Iterator, List, Optional, TextIO

from wizctl.palette import PaletteColor

try:
    import termios
    import tty
except ImportError:
    termios = None  # type: ignore[assignment]
    tty = None  # type: ignore[assignment]

if os.name == "nt":
    import msvcrt


class PaletteTuiError(RuntimeError):
    """Raised when the palette picker cannot use the current terminal."""


RESET = "\033[0m"
CLEAR = "\033[2J\033[H"
HIDE_CURSOR = "\033[?25l"
SHOW_CURSOR = "\033[?25h"


def supports_tui(
    input_stream: TextIO = sys.stdin, output_stream: TextIO = sys.stdout
) -> bool:
    """Return whether an interactive ANSI terminal is available."""
    if not input_stream.isatty() or not output_stream.isatty():
        return False
    if os.name == "nt":
        return True
    return os.environ.get("TERM", "").lower() not in {"", "dumb"}


def render_picker(
    image_path: str, palette: List[PaletteColor], selected: int, ip: str
) -> str:
    """Render one frame of the palette picker with true-color swatches."""
    lines = [
        f"\033[1mImage palette\033[0m  {image_path}",
        f"Selected color will be sent to {ip}",
        "",
    ]
    for number, color in enumerate(palette, start=1):
        marker = ">" if number - 1 == selected else " "
        swatch = f"\033[48;2;{color.red};{color.green};{color.blue}m            {RESET}"
        line = f"{marker} {number}. {swatch}  {color.hex}  {color.percentage:5.1f}%"
        if number - 1 == selected:
            line = f"\033[7m{line}{RESET}"
        lines.append(line)
    lines.extend(["", "Up/Down: choose   Enter: apply   q or Esc: quit"])
    return CLEAR + "\n".join(lines)


def move_selection(selected: int, key: str, count: int) -> Optional[int]:
    """Return the next selection, or None when the picker should close."""
    if key in {"q", "Q", "\033"}:
        return None
    if key in {"\033[A", "k", "K"}:
        return (selected - 1) % count
    if key in {"\033[B", "j", "J"}:
        return (selected + 1) % count
    return selected


@contextmanager
def raw_terminal(stream: TextIO) -> Iterator[None]:
    """Temporarily switch an interactive terminal to raw key input."""
    if os.name == "nt":
        yield
        return

    if termios is None or tty is None:
        raise PaletteTuiError("palette picker needs an interactive terminal")
    try:
        descriptor = stream.fileno()
        original_settings = termios.tcgetattr(descriptor)
    except (AttributeError, OSError, termios.error) as exc:
        raise PaletteTuiError("palette picker needs an interactive terminal") from exc

    try:
        tty.setraw(descriptor)
        yield
    finally:
        termios.tcsetattr(descriptor, termios.TCSADRAIN, original_settings)


def read_key(input_stream: TextIO) -> str:
    """Read one key, including a terminal arrow-key sequence."""
    if os.name == "nt":
        key = msvcrt.getwch()
        if key not in {"\x00", "\xe0"}:
            return key
        return {"H": "\033[A", "P": "\033[B"}.get(msvcrt.getwch(), "")

    key = input_stream.read(1)
    if key != "\033":
        return key

    try:
        descriptor = input_stream.fileno()
    except (AttributeError, OSError):
        return key
    if select.select([descriptor], [], [], 0.03)[0]:
        return key + input_stream.read(2)
    return key


def choose_palette_color(
    image_path: str,
    palette: List[PaletteColor],
    ip: str,
    input_stream: TextIO = sys.stdin,
    output_stream: TextIO = sys.stdout,
) -> Optional[PaletteColor]:
    """Let a user select a color and return it, or None after quitting."""
    if not supports_tui(input_stream, output_stream):
        raise PaletteTuiError("palette picker needs an interactive ANSI terminal")

    selected = 0
    output_stream.write(HIDE_CURSOR)
    output_stream.flush()
    try:
        with raw_terminal(input_stream):
            while True:
                output_stream.write(render_picker(image_path, palette, selected, ip))
                output_stream.flush()
                key = read_key(input_stream)
                if key in {"\r", "\n"}:
                    return palette[selected]
                next_selected = move_selection(selected, key, len(palette))
                if next_selected is None:
                    return None
                selected = next_selected
    finally:
        output_stream.write(RESET + SHOW_CURSOR + "\n")
        output_stream.flush()
