#!/usr/bin/env python3
"""Smoke-test the current binary on X11 against a local fake bulb.

Uses an isolated configuration/runtime directory. Never contacts a real bulb.
Run after building: python3 scripts/verify_x11.py --binary target/release/wizctl
"""

import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time


def wait_for(check, description, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.1)
    raise AssertionError(f"Timed out: {description}")


def windows(title, pid):
    result = subprocess.run(
        ["xdotool", "search", "--all", "--onlyvisible", "--pid", str(pid), "--name", title],
        capture_output=True, text=True, check=False,
    )
    return result.stdout.splitlines()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/release/wizctl")
    parser.add_argument("--output", default="/tmp/wizctl-x11-validation.json")
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    if not os.environ.get("DISPLAY"):
        raise SystemExit("An X11 desktop is required")

    evidence = {}
    children = []
    with tempfile.TemporaryDirectory(prefix="wizctl-x11-") as root:
        config = Path(root) / "config" / "wizctl"
        config.mkdir(parents=True)
        state_path = config / "state.json"
        state_path.write_text(json.dumps({"ip": "192.0.2.11", "power": False}))
        runtime = Path(root) / "runtime"
        runtime.mkdir(mode=0o700)
        env = dict(os.environ, XDG_CONFIG_HOME=str(config.parent),
                   XDG_RUNTIME_DIR=str(runtime))
        env.pop("WIZ_IP", None)
        env.pop("BULB_IP", None)
        stop = threading.Event()
        bulb = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        bulb.bind(("127.0.0.1", 38899))
        bulb.settimeout(0.1)

        def serve():
            pilot = {"state": True, "dimming": 50, "temp": 3500,
                     "sceneId": 0, "rssi": -42, "src": 7,
                     "mac": "000000000000"}
            while not stop.is_set():
                try:
                    data, sender = bulb.recvfrom(4096)
                except socket.timeout:
                    continue
                request = json.loads(data)
                method = request["method"]
                if method == "setPilot":
                    pilot.update(request["params"])
                    result = {"success": True}
                else:
                    result = pilot
                bulb.sendto(json.dumps({"method": method, "result": result}).encode(), sender)

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()

        def launch(command):
            child = subprocess.Popen([binary, "--ip", "127.0.0.1", command], env=env,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            children.append(child)
            return child

        try:
            studio = launch("studio")
            wid = wait_for(lambda: windows("WiZ Controller - wizctl", studio.pid), "studio visible")[-1]
            subprocess.run(["xdotool", "windowactivate", "--sync", wid], check=True)
            subprocess.run(["xdotool", "key", "alt+F4"], check=True)
            studio.wait(timeout=12)
            assert studio.returncode == 0, studio.stderr.read().decode()
            saved = json.loads(state_path.read_text())
            assert saved["ip"] == "192.0.2.11", saved
            evidence["studio_normal_close_preserves_ip"] = saved["ip"]

            widget = launch("widget")
            wid = wait_for(lambda: windows("wizctl - Quick Control", widget.pid), "widget visible")[-1]
            subprocess.run(["xdotool", "windowactivate", "--sync", wid], check=True)

            def properties():
                result = subprocess.run([
                    "xprop", "-id", wid, "_NET_WM_WINDOW_TYPE", "_NET_WM_STATE"
                ], text=True, capture_output=True, check=False)
                if widget.poll() is not None:
                    raise AssertionError(f"Popover exited during mapping: {widget.stderr.read().decode()}")
                output = result.stdout
                expected = ["UTILITY", "SKIP_TASKBAR", "SKIP_PAGER", "ABOVE"]
                return output if all(value in output for value in expected) else None

            evidence["widget_properties"] = wait_for(properties, "popover EWMH properties")
            # Exercise the actual pin hit target, then focus a different window.
            subprocess.run(["xdotool", "mousemove", "--window", wid, "270", "20", "click", "1"], check=True)
            time.sleep(0.2)
            studio = launch("studio")
            sid = wait_for(lambda: windows("WiZ Controller - wizctl", studio.pid), "studio for pin test")[-1]
            subprocess.run(["xdotool", "windowactivate", "--sync", sid], check=True)
            time.sleep(0.8)
            assert widget.poll() is None, "Pinned popover closed on focus loss"
            evidence["pin_keeps_open_on_focus_loss"] = True
            subprocess.run(["xdotool", "windowactivate", "--sync", wid], check=True)
            subprocess.run(["xdotool", "mousemove", "--window", wid, "270", "20", "click", "1"], check=True)
            time.sleep(0.2)
            subprocess.run(["xdotool", "windowactivate", "--sync", sid], check=True)
            widget.wait(timeout=12)
            assert widget.returncode == 0, widget.stderr.read().decode()
            evidence["unpin_restores_focus_loss_dismissal"] = True
            subprocess.run(["xdotool", "key", "alt+F4"], check=True)
            studio.wait(timeout=12)
            time.sleep(0.4)
            widget = launch("widget")
            wid = wait_for(lambda: windows("wizctl - Quick Control", widget.pid), "widget for Escape")[-1]

            subprocess.run(["xdotool", "windowactivate", "--sync", wid], check=True)
            subprocess.run(["xdotool", "key", "Escape"], check=True)
            widget.wait(timeout=12)
            assert widget.returncode == 0, widget.stderr.read().decode()
            evidence["escape_exits"] = True

            time.sleep(0.4)
            widget = launch("widget")
            wait_for(lambda: windows("wizctl - Quick Control", widget.pid), "widget visible again")
            time.sleep(1)
            second = launch("widget")
            second.wait(timeout=12)
            widget.wait(timeout=12)
            evidence["second_launch_toggles_closed"] = second.returncode == 0
            assert evidence["second_launch_toggles_closed"]

            time.sleep(0.4)
            widget = launch("widget")
            wid = wait_for(lambda: windows("wizctl - Quick Control", widget.pid), "widget for focus check")[-1]
            subprocess.run(["xdotool", "windowactivate", "--sync", wid], check=True)
            time.sleep(1)
            studio = launch("studio")
            sid = wait_for(lambda: windows("WiZ Controller - wizctl", studio.pid), "second studio")[-1]
            subprocess.run(["xdotool", "windowactivate", "--sync", sid], check=True)
            widget.wait(timeout=12)
            assert widget.returncode == 0, widget.stderr.read().decode()
            evidence["focus_loss_exits"] = True
            subprocess.run(["xdotool", "key", "alt+F4"], check=True)
            studio.wait(timeout=12)
            layout = subprocess.check_output(["xdpyinfo", "-ext", "XINERAMA"], text=True)
            evidence["monitor_layout"] = "\n".join(
                line for line in layout.splitlines() if line.startswith("XINERAMA") or "head #" in line
            )
        finally:
            for child in children:
                if child.poll() is None:
                    child.terminate()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=5)
            stop.set()
            thread.join(timeout=2)
            bulb.close()

    Path(args.output).write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps(evidence, indent=2))


if __name__ == "__main__":
    main()
