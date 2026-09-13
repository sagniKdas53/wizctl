#!/usr/bin/env python3
"""Extract an image palette with the wizctl command-line interface."""

import sys
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPOSITORY_ROOT / "src"))

from wizctl.cli import entry_point


if __name__ == "__main__":
    arguments = sys.argv[1:]
    global_arguments = []
    if "--ip" in arguments:
        index = arguments.index("--ip")
        if index + 1 < len(arguments):
            global_arguments = arguments[index:index + 2]
            del arguments[index:index + 2]
    sys.argv = [sys.argv[0], *global_arguments, "palette", *arguments]
    entry_point()
