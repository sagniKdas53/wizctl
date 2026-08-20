"""Color definitions and mappings for wizctl."""

from typing import Dict, Tuple

COLORS: Dict[str, Tuple[int, int, int]] = {
    "red":        (255, 0, 0),
    "green":      (0, 255, 0),
    "blue":       (0, 0, 255),
    "white":      (255, 255, 255),
    "yellow":     (255, 255, 0),
    "cyan":       (0, 255, 255),
    "magenta":    (255, 0, 255),
    "orange":     (255, 128, 0),
    "purple":     (128, 0, 255),
    "pink":       (255, 80, 160),
    "warmwhite":  (255, 214, 170),
    "coolwhite":  (212, 235, 255),
    "daylight":   (255, 255, 251),
    "gold":       (255, 215, 0),
    "lime":       (50, 205, 50),
    "teal":       (0, 128, 128),
    "violet":     (238, 130, 238),
    "indigo":     (75, 0, 130),
    "amber":      (255, 191, 0),
    "crimson":    (220, 20, 60),
    "turquoise":  (64, 224, 208),
    "coral":      (255, 127, 80),
}
