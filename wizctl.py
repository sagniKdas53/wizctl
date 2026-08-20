#!/usr/bin/env python3
"""wizctl - WiZ Smart Bulb Local Control CLI launcher."""

from pathlib import Path
import sys

# Ensure src/ package is prioritized over root directory to avoid module shadowing
root_dir = str(Path(__file__).resolve().parent)
src_dir = str(Path(__file__).resolve().parent / "src")

while root_dir in sys.path:
    sys.path.remove(root_dir)
if "" in sys.path:
    sys.path.remove("")

sys.path.insert(0, src_dir)

from wizctl.cli import entry_point

if __name__ == "__main__":
    entry_point()
