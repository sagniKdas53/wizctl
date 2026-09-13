"""Compact interactive popover widget for the desktop panel."""

import asyncio
from pathlib import Path
import tempfile
import threading
import time
import tkinter as tk
from tkinter import ttk
from typing import Any, Callable, Dict, Optional, Tuple

from PIL import Image
from pywizlight import PilotBuilder, SCENES
from pywizlight.exceptions import WizLightConnectionError, WizLightTimeOutError

from wizctl import __version__
from wizctl.bulb import (
    DEFAULT_BULB_IP,
    apply_saved_state,
    command_toggle,
    get_bulb,
    get_status_info,
    kelvin_to_rgb,
)
from wizctl.parsers import parse_color, validate_ip
from wizctl.state import DEFAULT_PRESET_COLORS, load_state, save_state

# Modern Dark Theme Colors
BG_DARK = "#121214"
CARD_BG = "#1a1a1f"
CARD_BORDER = "#2a2a32"
CARD_HOVER = "#25252e"
TEXT_PRIMARY = "#f4f4f6"
TEXT_SECONDARY = "#a1a1aa"
TEXT_MUTED = "#71717a"
ACCENT_BLUE = "#3b82f6"
ACCENT_GREEN = "#22c55e"
ACCENT_RED = "#ef4444"
ACCENT_AMBER = "#f59e0b"
INPUT_BG = "#22222a"

# Quick Presets
QUICK_KELVIN = [
    ("🕯️ 2200K", 2200),
    ("🛋️ 2700K", 2700),
    ("📖 4000K", 4000),
    ("☀️ 6500K", 6500),
]

QUICK_SCENES = [
    (6, "🛋️ Cozy"),
    (3, "🌅 Sunset"),
    (1, "🌊 Ocean"),
    (14, "🌙 Night"),
]


class AsyncWidgetWorker:
    """Lightweight asynchronous worker loop for widget commands."""

    def __init__(self):
        self._loop = asyncio.new_event_loop()
        self._thread = threading.Thread(target=self._run_loop, daemon=True)
        self._thread.start()

    def _run_loop(self):
        asyncio.set_event_loop(self._loop)
        self._loop.run_forever()

    def submit(
        self,
        coro,
        on_success: Optional[Callable[[Any], None]] = None,
        on_error: Optional[Callable[[Exception], None]] = None,
    ):
        async def _wrapper():
            try:
                res = await coro
                if on_success:
                    on_success(res)
            except Exception as exc:
                if on_error:
                    on_error(exc)

        return asyncio.run_coroutine_threadsafe(_wrapper(), self._loop)

    def stop(self):
        if self._loop.is_running():
            self._loop.call_soon_threadsafe(self._loop.stop)


class WizctlWidget:
    """Compact popover widget designed to open right at the desktop panel."""

    def __init__(self, root: tk.Tk, target_ip: Optional[str] = None):
        self.root = root
        self.root.title("wizctl - Quick Control")
        self.root.configure(bg=BG_DARK)

        # Remove window border for popover look, or keep lightweight dialog style
        self.is_pinned = False
        self._is_closed = False

        # Load state
        self.state = load_state()
        if target_ip:
            self.state["ip"] = target_ip

        self.worker = AsyncWidgetWorker()
        self.is_online = False
        self.is_pinging = False
        self._auto_ping_job = None
        self._last_ping_time: float = 0.0
        self._consecutive_ping_failures: int = 0

        # Set window icon
        icon_path = Path(__file__).parent / "assets" / "icon_32.png"
        if icon_path.is_file():
            try:
                self._app_icon = tk.PhotoImage(file=str(icon_path))
                self.root.iconphoto(True, self._app_icon)
            except Exception:
                pass

        self._build_ui()
        self._position_near_pointer()

        # Key and focus bindings for popover dismissal
        self.root.bind("<Escape>", lambda e: self._on_close())
        self.root.bind("<FocusOut>", self._on_focus_out)
        self.root.protocol("WM_DELETE_WINDOW", self._on_close)

        # Initial ping
        self._init_ping_job = self.root.after(50, self.ping_bulb)

    def _position_near_pointer(self):
        """Intelligently position widget near mouse pointer or panel dock."""
        self.root.update_idletasks()
        width = 330
        height = 490
        screen_w = self.root.winfo_screenwidth()
        screen_h = self.root.winfo_screenheight()

        ptr_x = self.root.winfo_pointerx()
        ptr_y = self.root.winfo_pointery()

        # If pointer is offscreen or 0, fallback to center/bottom
        if ptr_x <= 0 and ptr_y <= 0:
            ptr_x = screen_w // 2
            ptr_y = screen_h - 40

        # Calculate coordinates
        x = max(10, min(screen_w - width - 10, ptr_x - (width // 2)))
        if ptr_y > screen_h // 2:
            # Bottom panel: place widget above panel
            y = max(10, ptr_y - height - 15)
        else:
            # Top panel: place widget below panel
            y = min(screen_h - height - 10, ptr_y + 25)

        self.root.geometry(f"{width}x{height}+{x}+{y}")

    def _on_focus_out(self, event):
        """Auto-dismiss when clicking outside, unless pinned."""
        if self.is_pinned or self._is_closed:
            return
        # If focus transferred outside this widget's window
        self.root.after(150, self._check_focus_loss)

    def _check_focus_loss(self):
        if self.is_pinned or self._is_closed:
            return
        try:
            focused = self.root.focus_get()
            if focused is None:
                self._on_close()
        except Exception:
            pass

    def _toggle_pin(self):
        """Toggle pinned state."""
        self.is_pinned = not self.is_pinned
        self.pin_btn.config(
            text="📌 Pinned" if self.is_pinned else "📌 Pin",
            fg=ACCENT_BLUE if self.is_pinned else TEXT_MUTED,
        )

    def _build_ui(self):
        """Construct compact widget components."""
        container = tk.Frame(self.root, bg=BG_DARK, padx=12, pady=10)
        container.pack(fill=tk.BOTH, expand=True)

        # 1. Header Bar
        header = tk.Frame(container, bg=BG_DARK)
        header.pack(fill=tk.X, pady=(0, 8))

        # Title & Status
        title_box = tk.Frame(header, bg=BG_DARK)
        title_box.pack(side=tk.LEFT, fill=tk.X, expand=True)

        self.title_label = tk.Label(
            title_box,
            text="WiZ Light",
            font=("Helvetica", 11, "bold"),
            bg=BG_DARK,
            fg=TEXT_PRIMARY,
        )
        self.title_label.pack(side=tk.LEFT)

        self.status_dot = tk.Label(
            title_box,
            text="●",
            font=("Helvetica", 10),
            bg=BG_DARK,
            fg=ACCENT_AMBER,
        )
        self.status_dot.pack(side=tk.LEFT, padx=(6, 2))

        self.status_sub = tk.Label(
            title_box,
            text=self.state.get("ip", DEFAULT_BULB_IP),
            font=("Monospace", 8),
            bg=BG_DARK,
            fg=TEXT_MUTED,
        )
        self.status_sub.pack(side=tk.LEFT)

        # Header Buttons (Pin & Close)
        self.pin_btn = tk.Button(
            header,
            text="📌 Pin",
            font=("Helvetica", 8),
            bg=BG_DARK,
            fg=TEXT_MUTED,
            activebackground=CARD_HOVER,
            activeforeground=TEXT_PRIMARY,
            bd=0,
            padx=4,
            pady=2,
            cursor="hand2",
            command=self._toggle_pin,
        )
        self.pin_btn.pack(side=tk.RIGHT, padx=(4, 0))

        close_btn = tk.Button(
            header,
            text="✕",
            font=("Helvetica", 9, "bold"),
            bg=BG_DARK,
            fg=TEXT_MUTED,
            activebackground=ACCENT_RED,
            activeforeground="#ffffff",
            bd=0,
            padx=6,
            pady=1,
            cursor="hand2",
            command=self._on_close,
        )
        close_btn.pack(side=tk.RIGHT)

        # 2. Power Toggle Button
        self.power_btn = tk.Button(
            container,
            text="💡 BULB IS ON",
            font=("Helvetica", 10, "bold"),
            bg=ACCENT_GREEN if self.state.get("power", True) else INPUT_BG,
            fg="#ffffff" if self.state.get("power", True) else TEXT_SECONDARY,
            activebackground="#16a34a",
            activeforeground="#ffffff",
            bd=0,
            pady=8,
            cursor="hand2",
            command=self.toggle_power,
        )
        self.power_btn.pack(fill=tk.X, pady=(0, 8))

        # 3. Brightness Card
        bright_card = tk.Frame(container, bg=CARD_BG, padx=8, pady=6)
        bright_card.pack(fill=tk.X, pady=(0, 8))

        b_header = tk.Frame(bright_card, bg=CARD_BG)
        b_header.pack(fill=tk.X, pady=(0, 2))

        tk.Label(
            b_header,
            text="☀️ Brightness",
            font=("Helvetica", 9, "bold"),
            bg=CARD_BG,
            fg=TEXT_PRIMARY,
        ).pack(side=tk.LEFT)

        init_b = self.state.get("brightness", 255)
        init_pct = int(init_b * 100 / 255)
        self.bright_label = tk.Label(
            b_header,
            text=f"{init_pct}%",
            font=("Monospace", 9, "bold"),
            bg=CARD_BG,
            fg=ACCENT_BLUE,
        )
        self.bright_label.pack(side=tk.RIGHT)

        self.bright_slider = ttk.Scale(
            bright_card,
            from_=1,
            to=255,
            value=init_b,
            command=self._on_brightness_slider,
        )
        self.bright_slider.pack(fill=tk.X, pady=(2, 4))
        self.bright_slider.bind("<ButtonRelease-1>", lambda e: self._on_brightness_release())

        # Quick brightness chips
        b_chips = tk.Frame(bright_card, bg=CARD_BG)
        b_chips.pack(fill=tk.X)
        for pct, val in [(25, 64), (50, 128), (75, 191), (100, 255)]:
            btn = tk.Button(
                b_chips,
                text=f"{pct}%",
                font=("Helvetica", 7, "bold"),
                bg=INPUT_BG,
                fg=TEXT_SECONDARY,
                activebackground=CARD_HOVER,
                activeforeground=TEXT_PRIMARY,
                bd=0,
                padx=4,
                pady=2,
                cursor="hand2",
                command=lambda v=val: self.set_brightness(v),
            )
            btn.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=1)

        # 4. White Temperature Chips
        k_card = tk.Frame(container, bg=CARD_BG, padx=8, pady=6)
        k_card.pack(fill=tk.X, pady=(0, 8))

        tk.Label(
            k_card,
            text="🌡️ White Presets",
            font=("Helvetica", 8, "bold"),
            bg=CARD_BG,
            fg=TEXT_MUTED,
            anchor="w",
        ).pack(fill=tk.X, pady=(0, 4))

        k_row = tk.Frame(k_card, bg=CARD_BG)
        k_row.pack(fill=tk.X)
        for label, kval in QUICK_KELVIN:
            btn = tk.Button(
                k_row,
                text=label,
                font=("Helvetica", 7),
                bg=INPUT_BG,
                fg=TEXT_PRIMARY,
                activebackground=CARD_HOVER,
                bd=0,
                padx=2,
                pady=4,
                cursor="hand2",
                command=lambda k=kval: self.set_kelvin(k),
            )
            btn.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=1)

        # 5. Scene Chips
        scenes_card = tk.Frame(container, bg=CARD_BG, padx=8, pady=6)
        scenes_card.pack(fill=tk.X, pady=(0, 8))

        tk.Label(
            scenes_card,
            text="✨ Quick Scenes",
            font=("Helvetica", 8, "bold"),
            bg=CARD_BG,
            fg=TEXT_MUTED,
            anchor="w",
        ).pack(fill=tk.X, pady=(0, 4))

        s_row = tk.Frame(scenes_card, bg=CARD_BG)
        s_row.pack(fill=tk.X)
        self.scene_buttons: Dict[int, tk.Button] = {}
        for sid, sname in QUICK_SCENES:
            btn = tk.Button(
                s_row,
                text=sname,
                font=("Helvetica", 7),
                bg=INPUT_BG,
                fg=TEXT_PRIMARY,
                activebackground=ACCENT_BLUE,
                activeforeground="#ffffff",
                bd=0,
                padx=2,
                pady=4,
                cursor="hand2",
                command=lambda s=sid, n=sname: self.set_scene(s, n),
            )
            btn.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=1)
            self.scene_buttons[sid] = btn

        # 6. Color Dots
        colors_card = tk.Frame(container, bg=CARD_BG, padx=8, pady=6)
        colors_card.pack(fill=tk.X, pady=(0, 8))

        tk.Label(
            colors_card,
            text="🎨 Quick Colors",
            font=("Helvetica", 8, "bold"),
            bg=CARD_BG,
            fg=TEXT_MUTED,
            anchor="w",
        ).pack(fill=tk.X, pady=(0, 4))

        color_row = tk.Frame(colors_card, bg=CARD_BG)
        color_row.pack(fill=tk.X)
        for hex_code in DEFAULT_PRESET_COLORS[:8]:
            swatch = tk.Button(
                color_row,
                bg=hex_code,
                activebackground=hex_code,
                bd=0,
                cursor="hand2",
                width=2,
                height=1,
                command=lambda c=hex_code: self.set_color_hex(c),
            )
            swatch.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=2)

        # 7. Footer: Open Studio Button
        studio_btn = tk.Button(
            container,
            text="🎛️ Open Full Studio...",
            font=("Helvetica", 9, "bold"),
            bg=INPUT_BG,
            fg=TEXT_SECONDARY,
            activebackground=CARD_HOVER,
            activeforeground=TEXT_PRIMARY,
            bd=1,
            relief=tk.FLAT,
            pady=5,
            cursor="hand2",
            command=self._open_full_gui,
        )
        studio_btn.pack(fill=tk.X)

    # --- Actions & Handlers ---

    def _get_ip(self) -> str:
        return self.state.get("ip", DEFAULT_BULB_IP)

    def ping_bulb(self, force: bool = False):
        """Ping bulb and update widget controls."""
        if getattr(self, "is_pinging", False) or getattr(self, "_is_closed", False):
            return

        now = time.time()
        # Enforce minimum cooldown of 5s unless forced
        if not force and (now - getattr(self, "_last_ping_time", 0.0) < 5.0):
            self._schedule_next_ping(12000)
            return

        self._last_ping_time = now
        ip = self._get_ip()
        self.is_pinging = True
        self.status_dot.config(fg=ACCENT_AMBER)
        self.status_sub.config(text=f"Pinging {ip}...", fg=ACCENT_AMBER)

        t0 = time.time()

        async def _do_ping():
            return await get_status_info(ip)

        def _on_success(info: Dict):
            if getattr(self, "_is_closed", False) or not self.root.winfo_exists():
                return
            elapsed_ms = int((time.time() - t0) * 1000)
            self.root.after(0, lambda: self._apply_status(info, elapsed_ms) if self.root.winfo_exists() else None)

        def _on_error(exc: Exception):
            if getattr(self, "_is_closed", False) or not self.root.winfo_exists():
                return
            self.root.after(0, lambda: self._apply_offline(str(exc)) if self.root.winfo_exists() else None)

        self.worker.submit(_do_ping(), on_success=_on_success, on_error=_on_error)

    def _apply_status(self, info: Dict, elapsed_ms: int):
        """Update widget with live status."""
        if getattr(self, "_is_closed", False) or not self.root.winfo_exists():
            return

        self.is_online = True
        self.is_pinging = False
        self._consecutive_ping_failures = 0
        self.status_dot.config(fg=ACCENT_GREEN)
        rssi = f"{info['rssi']} dBm" if info.get("rssi") is not None else f"{elapsed_ms}ms"
        self.status_sub.config(text=f"{info['ip']} ({rssi})", fg=ACCENT_GREEN)

        power = info.get("power", True)
        self.state["power"] = power
        self._update_power_button(power)

        b = info.get("brightness")
        if b is not None:
            self.state["brightness"] = b
            pct = int(b * 100 / 255)
            self.bright_label.config(text=f"{pct}%")
            self.bright_slider.set(b)

        scene_id = info.get("scene_id")
        if scene_id and scene_id != 0:
            self.state["scene_id"] = scene_id
            for sid, btn in self.scene_buttons.items():
                if sid == scene_id:
                    btn.config(bg=ACCENT_BLUE, fg="#ffffff")
                else:
                    btn.config(bg=INPUT_BG, fg=TEXT_PRIMARY)

        save_state(self.state)
        self._schedule_next_ping(12000)

    def _apply_offline(self, err_msg: str):
        if getattr(self, "_is_closed", False) or not self.root.winfo_exists():
            return
        self.is_online = False
        self.is_pinging = False
        self._consecutive_ping_failures += 1
        self.status_dot.config(fg=ACCENT_RED)
        self.status_sub.config(text="Offline", fg=ACCENT_RED)
        backoffs = [6000, 12000, 20000, 30000]
        idx = min(self._consecutive_ping_failures - 1, len(backoffs) - 1)
        self._schedule_next_ping(backoffs[max(0, idx)])

    def _schedule_next_ping(self, delay_ms: int = 12000):
        if getattr(self, "_is_closed", False) or not self.root.winfo_exists():
            return
        if self._auto_ping_job:
            try:
                self.root.after_cancel(self._auto_ping_job)
            except Exception:
                pass
        self._auto_ping_job = self.root.after(delay_ms, lambda: self.ping_bulb(force=False) if not getattr(self, "_is_closed", False) and self.root.winfo_exists() else None)

    def _update_power_button(self, power: bool):
        if power:
            self.power_btn.config(
                text="💡 BULB IS ON (Click to turn OFF)",
                bg=ACCENT_GREEN,
                fg="#ffffff",
            )
        else:
            self.power_btn.config(
                text="○ BULB IS OFF (Click to turn ON)",
                bg=INPUT_BG,
                fg=TEXT_SECONDARY,
            )

    def toggle_power(self):
        """Toggle bulb power."""
        ip = self._get_ip()
        optimistic = not self.state.get("power", True)
        self.state["power"] = optimistic
        self._update_power_button(optimistic)

        async def _do_toggle():
            async with get_bulb(ip) as bulb:
                if optimistic:
                    await bulb.turn_on()
                else:
                    await bulb.turn_off()

        self.worker.submit(_do_toggle())
        save_state(self.state)
        self._schedule_next_ping(12000)

    def _on_brightness_slider(self, val_str: str):
        val = int(float(val_str))
        pct = int(round(val * 100 / 255))
        self.bright_label.config(text=f"{pct}%")
        self.state["brightness"] = val

    def _on_brightness_release(self):
        val = int(self.bright_slider.get())
        self.set_brightness(val)

    def set_brightness(self, val: int):
        val = max(1, min(255, val))
        self.state["brightness"] = val
        self.bright_slider.set(val)
        pct = int(round(val * 100 / 255))
        self.bright_label.config(text=f"{pct}%")

        ip = self._get_ip()
        async def _do_b():
            async with get_bulb(ip) as bulb:
                await bulb.turn_on(PilotBuilder(brightness=val))
        self.worker.submit(_do_b())
        save_state(self.state)
        self._schedule_next_ping(12000)

    def set_kelvin(self, kval: int):
        self.state["kelvin"] = kval
        self.state["mode"] = "kelvin"
        ip = self._get_ip()
        async def _do_k():
            async with get_bulb(ip) as bulb:
                await bulb.turn_on(PilotBuilder(colortemp=kval))
        self.worker.submit(_do_k())
        save_state(self.state)
        self._schedule_next_ping(12000)

    def set_scene(self, scene_id: int, scene_name: str):
        self.state["scene_id"] = scene_id
        self.state["mode"] = "scene"
        for sid, btn in self.scene_buttons.items():
            if sid == scene_id:
                btn.config(bg=ACCENT_BLUE, fg="#ffffff")
            else:
                btn.config(bg=INPUT_BG, fg=TEXT_PRIMARY)

        ip = self._get_ip()
        async def _do_s():
            async with get_bulb(ip) as bulb:
                await bulb.turn_on(PilotBuilder(scene=scene_id))
        self.worker.submit(_do_s())
        save_state(self.state)
        self._schedule_next_ping(12000)

    def set_color_hex(self, hex_code: str):
        rgb = parse_color(hex_code)
        self.state["rgb"] = list(rgb)
        self.state["hex"] = hex_code
        self.state["mode"] = "color"
        ip = self._get_ip()
        async def _do_c():
            async with get_bulb(ip) as bulb:
                await bulb.turn_on(PilotBuilder(rgb=rgb))
        self.worker.submit(_do_c())
        save_state(self.state)
        self._schedule_next_ping(12000)

    def _open_full_gui(self):
        """Open complete wizctl studio GUI and close widget."""
        from wizctl.gui import run_gui
        target_ip = self._get_ip()
        self._on_close()
        run_gui(target_ip=target_ip)

    def _on_close(self):
        """Clean close."""
        self._is_closed = True
        if hasattr(self, "_init_ping_job") and self._init_ping_job:
            try:
                self.root.after_cancel(self._init_ping_job)
            except Exception:
                pass
            self._init_ping_job = None
        if self._auto_ping_job:
            try:
                self.root.after_cancel(self._auto_ping_job)
            except Exception:
                pass
            self._auto_ping_job = None
        save_state(self.state)
        self.worker.stop()
        try:
            self.root.destroy()
        except Exception:
            pass


def run_widget(target_ip: Optional[str] = None) -> int:
    """Launch the interactive popover widget."""
    root = tk.Tk()
    app = WizctlWidget(root, target_ip=target_ip)
    try:
        root.mainloop()
        return 0
    except KeyboardInterrupt:
        return 130


def handle_panel_click(target_ip: Optional[str] = None) -> int:
    """Handle panel launcher click: double-click toggles bulb power, single-click opens widget."""
    stamp_file = Path(tempfile.gettempdir()) / "wizctl_panel_click_stamp"
    now = time.time()

    is_double_click = False
    if stamp_file.exists():
        try:
            prev_time = float(stamp_file.read_text().strip())
            if now - prev_time <= 0.35:  # 350ms double-click window
                is_double_click = True
        except Exception:
            pass

    if is_double_click:
        try:
            stamp_file.unlink(missing_ok=True)
        except Exception:
            pass
        # Double-click: Toggle bulb power immediately
        state = load_state()
        ip = target_ip or state.get("ip", DEFAULT_BULB_IP)
        asyncio.run(command_toggle(ip))
        return 0
    else:
        # First click: record stamp
        try:
            stamp_file.write_text(str(now))
        except Exception:
            pass

        # Wait 350ms to see if a second click arrives
        time.sleep(0.35)
        if stamp_file.exists():
            try:
                cur_stamp = float(stamp_file.read_text().strip())
                if abs(cur_stamp - now) < 0.001:
                    stamp_file.unlink(missing_ok=True)
                    return run_widget(target_ip=target_ip)
            except Exception:
                pass
        return 0
