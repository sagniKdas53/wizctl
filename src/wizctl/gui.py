"""Graphical User Interface for wizctl using Tkinter."""

import asyncio
import colorsys
import io
import math
import os
from pathlib import Path
import threading
import time
import tkinter as tk
from tkinter import filedialog, messagebox, ttk
from typing import Any, Callable, Dict, List, Optional, Tuple
import urllib.parse

from PIL import Image
from pywizlight import PilotBuilder, SCENES, wizlight
from pywizlight.exceptions import WizLightConnectionError, WizLightTimeOutError

try:
    from tkinterdnd2 import DND_FILES, TkinterDnD
    HAS_TKDND = True
except (ImportError, RuntimeError, Exception):
    HAS_TKDND = False

from wizctl import __version__
from wizctl.bulb import DEFAULT_BULB_IP, get_bulb, get_status_info
from wizctl.palette import PaletteColor, PaletteError, extract_palette
from wizctl.parsers import parse_brightness, parse_color, parse_kelvin, validate_ip
from wizctl.state import DEFAULT_PRESET_COLORS, load_state, save_state

# UI Color Palette (Modern Dark Theme)
BG_DARK = "#121214"
CARD_BG = "#1a1a1f"
CARD_BORDER = "#2a2a32"
CARD_HOVER = "#24242c"
TEXT_PRIMARY = "#f4f4f6"
TEXT_SECONDARY = "#a1a1aa"
TEXT_MUTED = "#71717a"
ACCENT_BLUE = "#3b82f6"
ACCENT_GREEN = "#22c55e"
ACCENT_RED = "#ef4444"
ACCENT_AMBER = "#f59e0b"
INPUT_BG = "#22222a"

# Popular WiZ scenes for quick selection
FEATURED_SCENES = [
    (6, "Cozy"),
    (3, "Sunset"),
    (1, "Ocean"),
    (29, "Candlelight"),
    (4, "Forest"),
    (14, "Night light"),
    (2, "Romance"),
    (7, "Party"),
    (5, "Fireplace"),
    (23, "Deep dive"),
    (15, "Spring"),
    (17, "Pulse"),
]


class AsyncBulbWorker:
    """Manages asynchronous bulb commands on a background thread event loop."""

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
        """Submit a coroutine and call success/error callbacks in a thread-safe way."""

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
        """Stop the background event loop."""
        if self._loop.is_running():
            self._loop.call_soon_threadsafe(self._loop.stop)


def pil_to_photo_image(img: Image.Image) -> tk.PhotoImage:
    """Convert a PIL Image to a native tk.PhotoImage using in-memory PNG bytes."""
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return tk.PhotoImage(data=buf.getvalue())


def parse_dropped_paths(data: str, root_tk: Optional[Any] = None) -> List[str]:
    """Cleanly parse raw drag & drop data into a list of valid filesystem paths."""
    if not data:
        return []

    items = []
    if root_tk and hasattr(root_tk, "splitlist"):
        try:
            items = list(root_tk.splitlist(data))
        except Exception:
            items = []

    if not items:
        raw_lines = data.strip().splitlines()
        for line in raw_lines:
            line = line.strip()
            if line:
                items.append(line)

    if not items and data.strip():
        items = [data.strip()]

    results = []
    for item in items:
        cleaned = item.strip().strip("\"'{} \t\r\n")
        # Handle file:// URI
        if cleaned.startswith("file://"):
            parsed = urllib.parse.urlparse(cleaned)
            cleaned = urllib.parse.unquote(parsed.path)
            if os.name == "nt" and cleaned.startswith("/") and len(cleaned) > 2 and cleaned[2] == ":":
                cleaned = cleaned[1:]
        elif cleaned.startswith("//") and not cleaned.startswith("///"):
            cleaned = "/" + cleaned.lstrip("/")

        if cleaned:
            results.append(cleaned)

    return results


class ColorWheelCanvas(tk.Canvas):
    """Interactive circular HSV color wheel widget using Pillow."""

    def __init__(
        self,
        parent,
        size: int = 200,
        on_color_change: Optional[Callable[[Tuple[int, int, int], str], None]] = None,
        on_color_release: Optional[Callable[[Tuple[int, int, int], str], None]] = None,
        bg: str = CARD_BG,
        **kwargs,
    ):
        super().__init__(
            parent,
            width=size,
            height=size,
            bg=bg,
            highlightthickness=0,
            bd=0,
            **kwargs,
        )
        self.size = size
        self.radius = size / 2.0 - 4
        self.center = size / 2.0
        self.on_color_change = on_color_change
        self.on_color_release = on_color_release

        self.current_rgb: Tuple[int, int, int] = (255, 140, 0)
        self.current_hex: str = "#ff8c00"
        self._reticle_radius = 7

        self._wheel_image = self._generate_color_wheel()
        self._photo_image = pil_to_photo_image(self._wheel_image)

        # Draw color wheel image
        self.create_image(self.center, self.center, image=self._photo_image)

        # Reticle items (outer shadow ring, inner white ring, center colored dot)
        self._reticle_shadow = self.create_oval(0, 0, 0, 0, outline="#000000", width=3)
        self._reticle_ring = self.create_oval(0, 0, 0, 0, outline="#ffffff", width=2)
        self._reticle_center = self.create_oval(0, 0, 0, 0, fill=self.current_hex, outline="")

        self.bind("<Button-1>", self._on_mouse_event)
        self.bind("<B1-Motion>", self._on_mouse_event)
        self.bind("<ButtonRelease-1>", self._on_release_event)

        # Set initial position
        self.set_rgb(self.current_rgb, notify=False)

    def _generate_color_wheel(self) -> Image.Image:
        """Render a smooth anti-aliased HSV color wheel."""
        size = self.size
        radius = self.radius
        center = self.center
        raw = bytearray(size * size * 4)

        idx = 0
        for y in range(size):
            dy = y - center
            dy2 = dy * dy
            for x in range(size):
                dx = x - center
                r2 = dx * dx + dy2
                if r2 <= radius * radius:
                    r = math.sqrt(r2)
                    angle = math.atan2(dy, dx)
                    h = (angle / (2 * math.pi)) % 1.0
                    s = r / radius
                    rgb = colorsys.hsv_to_rgb(h, s, 1.0)

                    alpha = 255
                    if r > radius - 1.5:
                        alpha = int(255 * (radius - r) / 1.5)
                        if alpha < 0:
                            alpha = 0

                    raw[idx] = int(rgb[0] * 255)
                    raw[idx + 1] = int(rgb[1] * 255)
                    raw[idx + 2] = int(rgb[2] * 255)
                    raw[idx + 3] = alpha
                idx += 4

        return Image.frombytes("RGBA", (size, size), bytes(raw))

    def _on_mouse_event(self, event):
        dx = event.x - self.center
        dy = event.y - self.center
        r = math.hypot(dx, dy)
        r_clamped = min(r, self.radius)

        angle = math.atan2(dy, dx)
        h = (angle / (2 * math.pi)) % 1.0
        s = r_clamped / self.radius if self.radius > 0 else 0.0

        rgb_float = colorsys.hsv_to_rgb(h, s, 1.0)
        rgb = (int(rgb_float[0] * 255), int(rgb_float[1] * 255), int(rgb_float[2] * 255))
        hex_code = f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"

        # Position marker at clamped radius
        px = self.center + r_clamped * math.cos(angle)
        py = self.center + r_clamped * math.sin(angle)

        self._update_reticle_position(px, py, hex_code)
        self.current_rgb = rgb
        self.current_hex = hex_code

        if self.on_color_change:
            self.on_color_change(rgb, hex_code)

    def _on_release_event(self, event):
        if self.on_color_release:
            self.on_color_release(self.current_rgb, self.current_hex)

    def _update_reticle_position(self, x: float, y: float, hex_code: str):
        rr = self._reticle_radius
        self.coords(self._reticle_shadow, x - rr - 1, y - rr - 1, x + rr + 1, y + rr + 1)
        self.coords(self._reticle_ring, x - rr, y - rr, x + rr, y + rr)
        self.coords(self._reticle_center, x - rr + 2, y - rr + 2, x + rr - 2, y + rr - 2)
        self.itemconfig(self._reticle_center, fill=hex_code)

    def set_rgb(self, rgb: Tuple[int, int, int], notify: bool = False):
        """Update the selected color and move the reticle accordingly."""
        self.current_rgb = rgb
        self.current_hex = f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"

        # Convert RGB to HSV
        h, s, v = colorsys.rgb_to_hsv(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0)
        # Position marker
        angle = h * 2 * math.pi
        r = s * self.radius
        px = self.center + r * math.cos(angle)
        py = self.center + r * math.sin(angle)

        self._update_reticle_position(px, py, self.current_hex)

        if notify and self.on_color_change:
            self.on_color_change(self.current_rgb, self.current_hex)


class WizctlGUI:
    """Main Application GUI for WiZ Bulb Control."""

    def __init__(self, root: tk.Tk, target_ip: Optional[str] = None):
        self.root = root
        self.root.title("WiZ Controller - wizctl")
        self.root.geometry("500x780")
        self.root.minsize(460, 720)
        self.root.configure(bg=BG_DARK)

        # Load persisted state
        self.state = load_state()
        if target_ip:
            self.state["ip"] = target_ip

        self.worker = AsyncBulbWorker()

        # Throttling timers
        self._last_send_time = 0.0
        self._throttle_interval = 0.08  # 80ms throttle
        self._pending_color: Optional[Tuple[int, int, int]] = None
        self._pending_brightness: Optional[int] = None
        self._pending_kelvin: Optional[int] = None

        # Ping & connection state
        self.is_online = False
        self.is_pinging = False
        self._auto_ping_job = None

        # Image Palette State
        self._current_image_path: Optional[str] = None
        self._palette_colors_count: int = 8
        self._palette_thumb_img: Optional[tk.PhotoImage] = None
        self._current_palette: List[PaletteColor] = []

        self._build_ui()
        self._setup_dnd()

        # Setup clipboard paste bindings
        self.root.bind("<Control-v>", self._on_paste_event)
        self.root.bind("<Control-V>", self._on_paste_event)

        # Refresh bulb state when window gains focus
        self.root.bind("<FocusIn>", lambda e: self.ping_bulb() if not self.is_pinging else None)

        # Auto-save state on close
        self.root.protocol("WM_DELETE_WINDOW", self._on_close)

        # Initial ping to detect bulb status immediately
        self.root.after(100, self.ping_bulb)
        self._schedule_auto_ping()

    def _setup_dnd(self):
        """Register OS drag and drop handlers if available."""
        if hasattr(self.root, "drop_target_register"):
            try:
                self.root.drop_target_register(DND_FILES)
                self.root.dnd_bind("<<Drop>>", self._on_drop_event)
                self.root.dnd_bind("<<DropEnter>>", self._on_drag_enter)
                self.root.dnd_bind("<<DropLeave>>", self._on_drag_leave)
            except Exception:
                pass

        if hasattr(self, "drop_card") and hasattr(self.drop_card, "drop_target_register"):
            try:
                self.drop_card.drop_target_register(DND_FILES)
                self.drop_card.dnd_bind("<<Drop>>", self._on_drop_event)
                self.drop_card.dnd_bind("<<DropEnter>>", self._on_drag_enter)
                self.drop_card.dnd_bind("<<DropLeave>>", self._on_drag_leave)
            except Exception:
                pass

    def _on_drag_enter(self, event):
        if hasattr(self, "drop_card"):
            self.drop_card.config(bg=CARD_HOVER, relief=tk.SOLID, bd=2)
        return getattr(event, "action", None)

    def _on_drag_leave(self, event):
        if hasattr(self, "drop_card"):
            self.drop_card.config(bg=INPUT_BG, relief=tk.GROOVE, bd=1)
        return getattr(event, "action", None)

    def _on_drop_event(self, event):
        self._on_drag_leave(event)
        data = getattr(event, "data", "")
        paths = parse_dropped_paths(data, getattr(self.root, "tk", None))
        if not paths:
            self._log_activity("No valid file path detected in drop", is_error=True)
            return

        for p in paths:
            if os.path.isfile(p):
                self.load_image_palette(p)
                return

        self._log_activity(f"Dropped file not found: {paths[0]}", is_error=True)

    def _build_ui(self):
        """Construct all UI components."""
        main_frame = tk.Frame(self.root, bg=BG_DARK, padx=16, pady=12)
        main_frame.pack(fill=tk.BOTH, expand=True)

        # --- 1. Top Header & Connection Card ---
        conn_card = tk.Frame(main_frame, bg=CARD_BG, bd=1, relief=tk.FLAT, padx=12, pady=10)
        conn_card.pack(fill=tk.X, pady=(0, 10))

        top_row = tk.Frame(conn_card, bg=CARD_BG)
        top_row.pack(fill=tk.X)

        tk.Label(
            top_row,
            text="WiZ Bulb IP:",
            font=("Helvetica", 10, "bold"),
            bg=CARD_BG,
            fg=TEXT_PRIMARY,
        ).pack(side=tk.LEFT, padx=(0, 8))

        self.ip_entry = tk.Entry(
            top_row,
            bg=INPUT_BG,
            fg=TEXT_PRIMARY,
            insertbackground=TEXT_PRIMARY,
            bd=1,
            relief=tk.FLAT,
            font=("Monospace", 10),
            width=16,
        )
        self.ip_entry.insert(0, self.state.get("ip", DEFAULT_BULB_IP))
        self.ip_entry.pack(side=tk.LEFT, padx=(0, 8), fill=tk.X, expand=True)
        self.ip_entry.bind("<Return>", lambda e: self.ping_bulb())

        self.ping_btn = tk.Button(
            top_row,
            text="⚡ Ping",
            font=("Helvetica", 9, "bold"),
            bg=ACCENT_BLUE,
            fg="#ffffff",
            activebackground="#2563eb",
            activeforeground="#ffffff",
            bd=0,
            padx=10,
            pady=4,
            cursor="hand2",
            command=self.ping_bulb,
        )
        self.ping_btn.pack(side=tk.RIGHT)

        # Status badge row
        status_row = tk.Frame(conn_card, bg=CARD_BG)
        status_row.pack(fill=tk.X, pady=(8, 0))

        self.status_dot = tk.Label(
            status_row,
            text="●",
            font=("Helvetica", 11),
            bg=CARD_BG,
            fg=ACCENT_AMBER,
        )
        self.status_dot.pack(side=tk.LEFT, padx=(0, 5))

        self.status_label = tk.Label(
            status_row,
            text="Initializing connection...",
            font=("Helvetica", 9),
            bg=CARD_BG,
            fg=TEXT_SECONDARY,
            anchor="w",
        )
        self.status_label.pack(side=tk.LEFT, fill=tk.X, expand=True)

        self.signal_label = tk.Label(
            status_row,
            text="",
            font=("Monospace", 8),
            bg=CARD_BG,
            fg=TEXT_MUTED,
        )
        self.signal_label.pack(side=tk.RIGHT)

        # --- 2. Power Toggle Card ---
        power_card = tk.Frame(main_frame, bg=CARD_BG, bd=1, relief=tk.FLAT, padx=12, pady=10)
        power_card.pack(fill=tk.X, pady=(0, 10))

        self.power_btn = tk.Button(
            power_card,
            text="💡 POWER ON",
            font=("Helvetica", 11, "bold"),
            bg=ACCENT_GREEN if self.state.get("power", True) else "#2e2e38",
            fg="#ffffff" if self.state.get("power", True) else TEXT_SECONDARY,
            activebackground="#16a34a",
            activeforeground="#ffffff",
            bd=0,
            pady=8,
            cursor="hand2",
            command=self.toggle_power,
        )
        self.power_btn.pack(fill=tk.X)

        # --- 3. Brightness Card ---
        bright_card = tk.Frame(main_frame, bg=CARD_BG, bd=1, relief=tk.FLAT, padx=12, pady=10)
        bright_card.pack(fill=tk.X, pady=(0, 10))

        b_header = tk.Frame(bright_card, bg=CARD_BG)
        b_header.pack(fill=tk.X, pady=(0, 4))

        tk.Label(
            b_header,
            text="☀️ Brightness",
            font=("Helvetica", 10, "bold"),
            bg=CARD_BG,
            fg=TEXT_PRIMARY,
        ).pack(side=tk.LEFT)

        init_b = self.state.get("brightness", 255)
        init_pct = int(init_b * 100 / 255)
        self.brightness_label = tk.Label(
            b_header,
            text=f"{init_pct}% ({init_b}/255)",
            font=("Monospace", 9, "bold"),
            bg=CARD_BG,
            fg=ACCENT_BLUE,
        )
        self.brightness_label.pack(side=tk.RIGHT)

        self.bright_slider = ttk.Scale(
            bright_card,
            from_=1,
            to=255,
            value=init_b,
            command=self._on_brightness_slider,
        )
        self.bright_slider.pack(fill=tk.X, pady=(2, 6))
        self.bright_slider.bind("<ButtonRelease-1>", lambda e: self._on_brightness_release())

        # Quick brightness buttons
        quick_b_frame = tk.Frame(bright_card, bg=CARD_BG)
        quick_b_frame.pack(fill=tk.X)
        for pct, val in [(10, 26), (25, 64), (50, 128), (75, 191), (100, 255)]:
            btn = tk.Button(
                quick_b_frame,
                text=f"{pct}%",
                font=("Helvetica", 8),
                bg=INPUT_BG,
                fg=TEXT_SECONDARY,
                activebackground=CARD_HOVER,
                activeforeground=TEXT_PRIMARY,
                bd=0,
                padx=6,
                pady=2,
                cursor="hand2",
                command=lambda v=val: self.set_brightness(v),
            )
            btn.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=2)

        # --- 4. Control Modes Notebook / Tabs ---
        style = ttk.Style()
        style.theme_use("default")
        style.configure(
            "TNotebook",
            background=BG_DARK,
            borderwidth=0,
        )
        style.configure(
            "TNotebook.Tab",
            background=CARD_BG,
            foreground=TEXT_SECONDARY,
            padding=[10, 6],
            font=("Helvetica", 9, "bold"),
            borderwidth=0,
        )
        style.map(
            "TNotebook.Tab",
            background=[("selected", INPUT_BG)],
            foreground=[("selected", TEXT_PRIMARY)],
        )

        self.notebook = ttk.Notebook(main_frame)
        self.notebook.pack(fill=tk.BOTH, expand=True, pady=(0, 8))

        # Tab 1: Color Wheel
        tab_wheel = tk.Frame(self.notebook, bg=CARD_BG, padx=10, pady=10)
        self.notebook.add(tab_wheel, text="🎨 Color Wheel")

        # Color wheel layout
        wheel_container = tk.Frame(tab_wheel, bg=CARD_BG)
        wheel_container.pack(fill=tk.X, pady=(0, 6))

        self.color_wheel = ColorWheelCanvas(
            wheel_container,
            size=190,
            on_color_change=self._on_wheel_color_drag,
            on_color_release=self._on_wheel_color_release,
        )
        self.color_wheel.pack(side=tk.LEFT, padx=(4, 12))

        # Right side info & hex
        right_info = tk.Frame(wheel_container, bg=CARD_BG)
        right_info.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)

        self.color_preview = tk.Frame(
            right_info,
            bg=self.state.get("hex", "#ff8c00"),
            height=34,
            bd=1,
            relief=tk.SOLID,
        )
        self.color_preview.pack(fill=tk.X, pady=(4, 6))

        self.hex_label = tk.Label(
            right_info,
            text="HEX Code:",
            font=("Helvetica", 8, "bold"),
            bg=CARD_BG,
            fg=TEXT_MUTED,
            anchor="w",
        )
        self.hex_label.pack(fill=tk.X)

        hex_entry_row = tk.Frame(right_info, bg=CARD_BG)
        hex_entry_row.pack(fill=tk.X, pady=(2, 6))

        self.hex_entry = tk.Entry(
            hex_entry_row,
            bg=INPUT_BG,
            fg=TEXT_PRIMARY,
            insertbackground=TEXT_PRIMARY,
            bd=1,
            relief=tk.FLAT,
            font=("Monospace", 10, "bold"),
            width=8,
        )
        self.hex_entry.insert(0, self.state.get("hex", "#ff8c00"))
        self.hex_entry.pack(side=tk.LEFT, fill=tk.X, expand=True, padx=(0, 4))
        self.hex_entry.bind("<Return>", lambda e: self._on_hex_submit())

        apply_btn = tk.Button(
            hex_entry_row,
            text="Set",
            font=("Helvetica", 8, "bold"),
            bg=ACCENT_BLUE,
            fg="#ffffff",
            activebackground="#2563eb",
            bd=0,
            padx=8,
            pady=2,
            cursor="hand2",
            command=self._on_hex_submit,
        )
        apply_btn.pack(side=tk.RIGHT)

        self.rgb_info_label = tk.Label(
            right_info,
            text=f"RGB: {self.state.get('rgb', [255,140,0])[0]}, {self.state.get('rgb', [255,140,0])[1]}, {self.state.get('rgb', [255,140,0])[2]}",
            font=("Monospace", 8),
            bg=CARD_BG,
            fg=TEXT_SECONDARY,
            anchor="w",
        )
        self.rgb_info_label.pack(fill=tk.X)

        # Quick preset colors row
        preset_label = tk.Label(
            tab_wheel,
            text="Quick Presets:",
            font=("Helvetica", 8, "bold"),
            bg=CARD_BG,
            fg=TEXT_MUTED,
            anchor="w",
        )
        preset_label.pack(fill=tk.X, pady=(4, 2))

        presets_frame = tk.Frame(tab_wheel, bg=CARD_BG)
        presets_frame.pack(fill=tk.X, pady=(0, 4))

        for color_hex in self.state.get("recent_colors", DEFAULT_PRESET_COLORS)[:10]:
            swatch = tk.Button(
                presets_frame,
                bg=color_hex,
                activebackground=color_hex,
                bd=1,
                relief=tk.FLAT,
                cursor="hand2",
                width=2,
                height=1,
                command=lambda c=color_hex: self.set_color_hex(c),
            )
            swatch.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=2)

        # Tab 2: White Temperature (Kelvin)
        tab_kelvin = tk.Frame(self.notebook, bg=CARD_BG, padx=12, pady=12)
        self.notebook.add(tab_kelvin, text="🌡️ White (Kelvin)")

        k_header = tk.Frame(tab_kelvin, bg=CARD_BG)
        k_header.pack(fill=tk.X, pady=(4, 6))

        tk.Label(
            k_header,
            text="Color Temperature",
            font=("Helvetica", 10, "bold"),
            bg=CARD_BG,
            fg=TEXT_PRIMARY,
        ).pack(side=tk.LEFT)

        init_k = self.state.get("kelvin", 2700)
        self.kelvin_label = tk.Label(
            k_header,
            text=f"{init_k} K",
            font=("Monospace", 10, "bold"),
            bg=CARD_BG,
            fg=ACCENT_AMBER,
        )
        self.kelvin_label.pack(side=tk.RIGHT)

        self.kelvin_slider = ttk.Scale(
            tab_kelvin,
            from_=2200,
            to=6500,
            value=init_k,
            command=self._on_kelvin_slider,
        )
        self.kelvin_slider.pack(fill=tk.X, pady=(4, 10))
        self.kelvin_slider.bind("<ButtonRelease-1>", lambda e: self._on_kelvin_release())

        k_presets_label = tk.Label(
            tab_kelvin,
            text="Temperature Presets:",
            font=("Helvetica", 8, "bold"),
            bg=CARD_BG,
            fg=TEXT_MUTED,
            anchor="w",
        )
        k_presets_label.pack(fill=tk.X, pady=(6, 4))

        k_btn_frame = tk.Frame(tab_kelvin, bg=CARD_BG)
        k_btn_frame.pack(fill=tk.X)

        kelvin_presets = [
            ("🕯️ Candle\n2200K", 2200),
            ("🛋️ Warm\n2700K", 2700),
            ("📖 Neutral\n4000K", 4000),
            ("☀️ Daylight\n6500K", 6500),
        ]
        for label, kval in kelvin_presets:
            btn = tk.Button(
                k_btn_frame,
                text=label,
                font=("Helvetica", 8),
                bg=INPUT_BG,
                fg=TEXT_PRIMARY,
                activebackground=CARD_HOVER,
                bd=0,
                padx=4,
                pady=6,
                cursor="hand2",
                command=lambda k=kval: self.set_kelvin(k),
            )
            btn.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=3)

        # Tab 3: Scenes
        tab_scenes = tk.Frame(self.notebook, bg=CARD_BG, padx=10, pady=10)
        self.notebook.add(tab_scenes, text="✨ Scenes")

        # WiZclick Quick Switch (Wall switch modes 1 & 2)
        wizclick_card = tk.Frame(tab_scenes, bg=INPUT_BG, bd=1, relief=tk.SOLID, padx=8, pady=6)
        wizclick_card.pack(fill=tk.X, pady=(0, 8))

        wc_header = tk.Frame(wizclick_card, bg=INPUT_BG)
        wc_header.pack(fill=tk.X, pady=(0, 4))
        tk.Label(
            wc_header,
            text="⚡ WiZclick (Wall Switch Settings)",
            font=("Helvetica", 8, "bold"),
            bg=INPUT_BG,
            fg=ACCENT_AMBER,
        ).pack(side=tk.LEFT)
        tk.Label(
            wc_header,
            text="Switch 1x or 2x",
            font=("Helvetica", 7),
            bg=INPUT_BG,
            fg=TEXT_MUTED,
        ).pack(side=tk.RIGHT)

        wc_btn_frame = tk.Frame(wizclick_card, bg=INPUT_BG)
        wc_btn_frame.pack(fill=tk.X)

        btn_cozy = tk.Button(
            wc_btn_frame,
            text="🛋️ Mode 1: Cozy (1st Click)",
            font=("Helvetica", 8, "bold"),
            bg=CARD_BG,
            fg=TEXT_PRIMARY,
            activebackground=CARD_HOVER,
            bd=1,
            padx=4,
            pady=4,
            cursor="hand2",
            command=lambda: self.set_scene(6, "Cozy"),
        )
        btn_cozy.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=2)

        btn_night = tk.Button(
            wc_btn_frame,
            text="🌙 Mode 2: Night light (2nd Click)",
            font=("Helvetica", 8, "bold"),
            bg=CARD_BG,
            fg=TEXT_PRIMARY,
            activebackground=CARD_HOVER,
            bd=1,
            padx=4,
            pady=4,
            cursor="hand2",
            command=lambda: self.set_scene(14, "Night light"),
        )
        btn_night.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=2)

        scenes_grid = tk.Frame(tab_scenes, bg=CARD_BG)
        scenes_grid.pack(fill=tk.BOTH, expand=True)

        for i, (sid, sname) in enumerate(FEATURED_SCENES):
            row = i // 3
            col = i % 3
            btn = tk.Button(
                scenes_grid,
                text=sname,
                font=("Helvetica", 9),
                bg=INPUT_BG,
                fg=TEXT_PRIMARY,
                activebackground=ACCENT_BLUE,
                activeforeground="#ffffff",
                bd=0,
                padx=6,
                pady=6,
                cursor="hand2",
                command=lambda s=sid, n=sname: self.set_scene(s, n),
            )
            btn.grid(row=row, column=col, sticky="nsew", padx=3, pady=3)

        for c in range(3):
            scenes_grid.grid_columnconfigure(c, weight=1)

        # Tab 4: Image Palette Picker
        self._build_palette_tab()

        # Restore initial RGB on wheel widget
        if "rgb" in self.state:
            self.color_wheel.set_rgb(tuple(self.state["rgb"]), notify=False)

        # --- 5. Bottom Activity Status Bar ---
        self.activity_bar = tk.Label(
            main_frame,
            text=f"wizctl v{__version__} - Ready",
            font=("Monospace", 8),
            bg=BG_DARK,
            fg=TEXT_MUTED,
            anchor="w",
        )
        self.activity_bar.pack(fill=tk.X, pady=(4, 0))

    def _build_palette_tab(self):
        """Build the interactive Image Palette Picker tab."""
        self.tab_palette = tk.Frame(self.notebook, bg=CARD_BG, padx=10, pady=10)
        self.notebook.add(self.tab_palette, text="🖼️ Palette")

        # Drop Zone / File Selection Card
        self.drop_card = tk.Frame(
            self.tab_palette,
            bg=INPUT_BG,
            bd=1,
            relief=tk.GROOVE,
            padx=10,
            pady=10,
            cursor="hand2",
        )
        self.drop_card.pack(fill=tk.X, pady=(0, 8))
        self.drop_card.bind("<Button-1>", lambda e: self._browse_image())

        drop_header = tk.Frame(self.drop_card, bg=INPUT_BG)
        drop_header.pack(fill=tk.X)

        self.drop_icon_label = tk.Label(
            drop_header,
            text="🖼️ Drag & drop image here or click 'Select Image...'",
            font=("Helvetica", 9, "bold"),
            bg=INPUT_BG,
            fg=TEXT_PRIMARY,
        )
        self.drop_icon_label.pack(side=tk.LEFT, fill=tk.X, expand=True, anchor="w")
        self.drop_icon_label.bind("<Button-1>", lambda e: self._browse_image())

        self.browse_btn = tk.Button(
            drop_header,
            text="📁 Select Image...",
            font=("Helvetica", 8, "bold"),
            bg=ACCENT_BLUE,
            fg="#ffffff",
            activebackground="#2563eb",
            bd=0,
            padx=8,
            pady=3,
            cursor="hand2",
            command=self._browse_image,
        )
        self.browse_btn.pack(side=tk.RIGHT)

        # Image Info and Preview Frame
        self.img_info_frame = tk.Frame(self.tab_palette, bg=CARD_BG)
        self.img_info_frame.pack(fill=tk.X, pady=(0, 6))

        self.thumb_label = tk.Label(self.img_info_frame, bg=CARD_BG)
        self.thumb_label.pack(side=tk.LEFT, padx=(0, 8))

        self.img_details_label = tk.Label(
            self.img_info_frame,
            text="No image loaded. Drag & drop or select an image to extract colors.",
            font=("Helvetica", 8),
            bg=CARD_BG,
            fg=TEXT_MUTED,
            justify=tk.LEFT,
            anchor="w",
        )
        self.img_details_label.pack(side=tk.LEFT, fill=tk.X, expand=True)

        # Swatches Container (Scrollable or Clean Grid)
        self.swatches_title = tk.Label(
            self.tab_palette,
            text="Extracted Color Palette (Click to Apply):",
            font=("Helvetica", 8, "bold"),
            bg=CARD_BG,
            fg=TEXT_MUTED,
            anchor="w",
        )
        self.swatches_title.pack(fill=tk.X, pady=(4, 4))

        self.palette_swatches_frame = tk.Frame(self.tab_palette, bg=CARD_BG)
        self.palette_swatches_frame.pack(fill=tk.BOTH, expand=True)

    def _browse_image(self):
        """Open native file dialog to choose an image file."""
        filetypes = [
            (
                "Image Files",
                "*.jpg *.jpeg *.png *.webp *.bmp *.gif *.tiff *.JPG *.PNG *.JPEG *.WEBP",
            ),
            ("All Files", "*.*"),
        ]
        chosen = filedialog.askopenfilename(
            title="Select Image to Extract Color Palette",
            filetypes=filetypes,
        )
        if chosen:
            self.load_image_palette(chosen)

    def _on_paste_event(self, event):
        """Handle clipboard paste of file path."""
        try:
            clipboard = self.root.clipboard_get().strip()
            if clipboard:
                paths = parse_dropped_paths(clipboard, getattr(self.root, "tk", None))
                for p in paths:
                    if os.path.isfile(p):
                        self.load_image_palette(p)
                        return
        except Exception:
            pass

    def load_image_palette(self, image_path: str, colors: Optional[int] = None):
        """Extract dominant colors from an image file and display the palette in GUI."""
        if colors is None:
            colors = self._palette_colors_count

        path = Path(image_path)
        if not path.is_file():
            self._log_activity(f"Image not found: {image_path}", is_error=True)
            return

        try:
            palette = extract_palette(str(path), colors=colors)
            self._current_palette = palette
            self._current_image_path = str(path)

            # Generate Thumbnail
            with Image.open(path) as img:
                w, h = img.size
                thumb = img.copy()
                thumb.thumbnail((64, 64))
                self._thumb_photo = pil_to_photo_image(thumb)
                self.thumb_label.config(image=self._thumb_photo)

            self.img_details_label.config(
                text=f"📄 {path.name}\nResolution: {w} × {h} px\nColors extracted: {len(palette)}",
                fg=TEXT_PRIMARY,
            )

            # Render swatches
            self._render_palette_swatches(palette)

            # Switch notebook to Palette tab
            self.notebook.select(self.tab_palette)
            self._log_activity(f"✓ Extracted {len(palette)} colors from {path.name}")

        except (PaletteError, Exception) as exc:
            self._log_activity(f"Error reading image palette: {exc}", is_error=True)
            messagebox.showerror("Palette Extraction Error", str(exc))

    def _render_palette_swatches(self, palette: List[PaletteColor]):
        """Render interactive swatch cards for extracted palette colors."""
        for widget in self.palette_swatches_frame.winfo_children():
            widget.destroy()

        cols = 2
        for idx, color in enumerate(palette):
            row = idx // cols
            col = idx % cols

            card = tk.Frame(
                self.palette_swatches_frame,
                bg=INPUT_BG,
                bd=1,
                relief=tk.FLAT,
                padx=8,
                pady=6,
                cursor="hand2",
            )
            card.grid(row=row, column=col, sticky="nsew", padx=3, pady=3)

            # Color preview square
            swatch_box = tk.Frame(
                card,
                bg=color.hex,
                width=28,
                height=28,
                bd=1,
                relief=tk.SOLID,
            )
            swatch_box.pack(side=tk.LEFT, padx=(0, 8))

            # Info labels
            info_frame = tk.Frame(card, bg=INPUT_BG)
            info_frame.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)

            hex_txt = tk.Label(
                info_frame,
                text=color.hex.upper(),
                font=("Monospace", 9, "bold"),
                bg=INPUT_BG,
                fg=TEXT_PRIMARY,
                anchor="w",
            )
            hex_txt.pack(fill=tk.X)

            pct_txt = tk.Label(
                info_frame,
                text=f"{color.percentage:.1f}% dominance",
                font=("Monospace", 8),
                bg=INPUT_BG,
                fg=TEXT_MUTED,
                anchor="w",
            )
            pct_txt.pack(fill=tk.X)

            # Click handler to apply this color
            def _make_handler(c=color):
                return lambda e: self._on_palette_swatch_click(c)

            card.bind("<Button-1>", _make_handler())
            swatch_box.bind("<Button-1>", _make_handler())
            hex_txt.bind("<Button-1>", _make_handler())
            pct_txt.bind("<Button-1>", _make_handler())
            info_frame.bind("<Button-1>", _make_handler())

        for c in range(cols):
            self.palette_swatches_frame.grid_columnconfigure(c, weight=1)

    def _on_palette_swatch_click(self, color: PaletteColor):
        """Handle user clicking a palette color swatch."""
        self.set_color_hex(color.hex)
        self._log_activity(f"✓ Applied image palette color {color.hex} ({color.percentage:.1f}%)")

    def _get_current_ip(self) -> str:
        """Get IP from entry field, validated and trimmed."""
        ip_raw = self.ip_entry.get().strip()
        if not ip_raw:
            return DEFAULT_BULB_IP
        try:
            return validate_ip(ip_raw)
        except ValueError:
            return DEFAULT_BULB_IP

    def _log_activity(self, message: str, is_error: bool = False):
        """Update bottom activity status bar."""
        color = ACCENT_RED if is_error else TEXT_MUTED
        self.activity_bar.config(text=message, fg=color)

    # --- Bulb Communication & Handlers ---

    def ping_bulb(self):
        """Ping the bulb and refresh all state values in the GUI."""
        ip_raw = self.ip_entry.get().strip() or DEFAULT_BULB_IP
        try:
            ip = validate_ip(ip_raw)
        except ValueError as exc:
            self._apply_bulb_offline(str(exc))
            return

        self.state["ip"] = ip
        self.is_pinging = True
        self.status_dot.config(fg=ACCENT_AMBER)
        self.status_label.config(text=f"Pinging {ip}...", fg=ACCENT_AMBER)
        self.ping_btn.config(state=tk.DISABLED)

        t0 = time.time()

        async def _do_ping():
            return await get_status_info(ip)

        def _on_success(info: Dict):
            elapsed_ms = int((time.time() - t0) * 1000)
            self.root.after(0, lambda: self._apply_bulb_status(info, elapsed_ms))

        def _on_error(exc: Exception):
            self.root.after(0, lambda: self._apply_bulb_offline(str(exc)))

        self.worker.submit(_do_ping(), on_success=_on_success, on_error=_on_error)

    def _apply_bulb_status(self, info: Dict, elapsed_ms: int):
        """Callback when status update succeeds."""
        self.is_online = True
        self.is_pinging = False
        self.ping_btn.config(state=tk.NORMAL)

        self.status_dot.config(fg=ACCENT_GREEN)
        mac_str = f" • MAC: {info['mac']}" if info.get("mac") else ""
        rssi_str = f"{info['rssi']} dBm" if info.get("rssi") is not None else ""
        self.status_label.config(
            text=f"Online ({elapsed_ms}ms){mac_str}",
            fg=ACCENT_GREEN,
        )
        self.signal_label.config(text=rssi_str)

        old_power = self.state.get("power")
        old_scene = self.state.get("scene_id")
        old_bright = self.state.get("brightness")
        external_changes = []

        # Update Power State
        power = info.get("power", True)
        if old_power is not None and old_power != power:
            external_changes.append(f"Power: {'ON' if power else 'OFF'}")
        self.state["power"] = power
        self._update_power_button_ui(power)

        # Update Brightness
        b = info.get("brightness")
        if b is not None:
            if old_bright is not None and abs(old_bright - b) > 5 and not getattr(self, "_is_user_dragging", False):
                pct = int(b * 100 / 255)
                external_changes.append(f"Brightness: {pct}%")
            self.state["brightness"] = b
            pct = int(b * 100 / 255)
            self.brightness_label.config(text=f"{pct}% ({b}/255)")
            self.bright_slider.set(b)

        # Update Scene
        scene_id = info.get("scene_id")
        scene_name = info.get("scene")
        if scene_id is not None and scene_id != 0:
            if old_scene is not None and old_scene != scene_id:
                sname = scene_name or SCENES.get(scene_id, f"Scene {scene_id}")
                external_changes.append(f"Scene: {sname}")
            self.state["scene_id"] = scene_id

        # Update Color or Kelvin
        rgb = info.get("rgb")
        if rgb and rgb[0] is not None:
            self.state["rgb"] = list(rgb)
            hex_code = f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"
            self.state["hex"] = hex_code
            self.color_preview.config(bg=hex_code)
            self.hex_entry.delete(0, tk.END)
            self.hex_entry.insert(0, hex_code)
            self.rgb_info_label.config(text=f"RGB: {rgb[0]}, {rgb[1]}, {rgb[2]}")
            self.color_wheel.set_rgb(rgb, notify=False)

        kelvin = info.get("colortemp")
        if kelvin:
            self.state["kelvin"] = kelvin
            self.kelvin_label.config(text=f"{kelvin} K")
            self.kelvin_slider.set(kelvin)

        if external_changes:
            changes_str = ", ".join(external_changes)
            self._log_activity(f"⚡ Live update ({changes_str})")
        else:
            self._log_activity(f"✓ Connected to {info['ip']} ({elapsed_ms}ms)")
        save_state(self.state)

    def _apply_bulb_offline(self, err_msg: str):
        """Callback when ping fails."""
        self.is_online = False
        self.is_pinging = False
        self.ping_btn.config(state=tk.NORMAL)
        self.status_dot.config(fg=ACCENT_RED)
        self.status_label.config(text="Offline (Unreachable)", fg=ACCENT_RED)
        self.signal_label.config(text="")
        self._log_activity(f"Connection failed: {err_msg}", is_error=True)

    def _schedule_auto_ping(self):
        """Background periodic ping to maintain live bulb state."""
        self.ping_bulb()
        self._auto_ping_job = self.root.after(3000, self._schedule_auto_ping)

    def _update_power_button_ui(self, power: bool):
        if power:
            self.power_btn.config(
                text="💡 BULB IS ON (Click to turn OFF)",
                bg=ACCENT_GREEN,
                fg="#ffffff",
            )
        else:
            self.power_btn.config(
                text="○ BULB IS OFF (Click to turn ON)",
                bg="#2e2e38",
                fg=TEXT_SECONDARY,
            )

    def toggle_power(self):
        """Toggle bulb power with live state verification to prevent stale overrides."""
        ip = self._get_current_ip()
        cached_power = self.state.get("power", True)
        optimistic_power = not cached_power
        self.state["power"] = optimistic_power
        self._update_power_button_ui(optimistic_power)

        async def _do_toggle():
            async with get_bulb(ip) as bulb:
                states = await bulb.updateState()
                live_power = None
                if states and states[0] and states[0].get_state() is not None:
                    live_power = bool(states[0].get_state())

                target_power = (not live_power) if live_power is not None else optimistic_power
                if target_power:
                    await bulb.turn_on()
                else:
                    await bulb.turn_off()
                return target_power, live_power

        def _on_success(res):
            if isinstance(res, tuple):
                target_power, live_power = res
            else:
                target_power, live_power = optimistic_power, None

            self.state["power"] = target_power
            self._update_power_button_ui(target_power)
            if live_power is not None and live_power == optimistic_power:
                self._log_activity(
                    f"✓ Synced external state ({'ON' if live_power else 'OFF'}) → toggled {'ON' if target_power else 'OFF'}"
                )
            else:
                self._log_activity(f"✓ Bulb turned {'ON' if target_power else 'OFF'}")
            save_state(self.state)

        def _on_error(exc):
            self.state["power"] = cached_power
            self._update_power_button_ui(cached_power)
            self._log_activity(f"Error toggling power: {exc}", is_error=True)

        self.worker.submit(_do_toggle(), on_success=_on_success, on_error=_on_error)

    # --- Color Controls ---

    def _on_wheel_color_drag(self, rgb: Tuple[int, int, int], hex_code: str):
        """Called live as user drags the color wheel."""
        self.color_preview.config(bg=hex_code)
        self.hex_entry.delete(0, tk.END)
        self.hex_entry.insert(0, hex_code)
        self.rgb_info_label.config(text=f"RGB: {rgb[0]}, {rgb[1]}, {rgb[2]}")

        self.state["rgb"] = list(rgb)
        self.state["hex"] = hex_code

        now = time.time()
        if now - self._last_send_time >= self._throttle_interval:
            self._last_send_time = now
            self._send_color(rgb)
        else:
            self._pending_color = rgb

    def _on_wheel_color_release(self, rgb: Tuple[int, int, int], hex_code: str):
        """Called when user releases mouse on color wheel."""
        self._pending_color = None
        self.state["rgb"] = list(rgb)
        self.state["hex"] = hex_code
        self._send_color(rgb)
        self._record_recent_color(hex_code)
        save_state(self.state)

    def _on_hex_submit(self):
        """Handle custom hex code input submission."""
        val = self.hex_entry.get().strip()
        try:
            rgb = parse_color(val)
            hex_code = f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"
            self.set_color_hex(hex_code)
        except Exception as exc:
            messagebox.showerror("Invalid Color", f"Could not parse color '{val}': {exc}")

    def set_color_hex(self, hex_code: str):
        """Set color by hex string."""
        rgb = parse_color(hex_code)
        self.state["rgb"] = list(rgb)
        self.state["hex"] = hex_code
        self.color_wheel.set_rgb(rgb, notify=False)
        self.color_preview.config(bg=hex_code)
        self.hex_entry.delete(0, tk.END)
        self.hex_entry.insert(0, hex_code)
        self.rgb_info_label.config(text=f"RGB: {rgb[0]}, {rgb[1]}, {rgb[2]}")
        self._send_color(rgb)
        self._record_recent_color(hex_code)
        save_state(self.state)

    def _send_color(self, rgb: Tuple[int, int, int]):
        """Send RGB command to bulb asynchronously."""
        ip = self._get_current_ip()

        async def _do_color():
            async with get_bulb(ip) as bulb:
                states = await bulb.updateState()
                live_power = states[0].get_state() if states and states[0] else None
                await bulb.turn_on(PilotBuilder(rgb=rgb))
                return live_power

        def _on_success(live_power):
            hex_code = f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"
            if live_power is False:
                self.state["power"] = True
                self._update_power_button_ui(True)
                self._log_activity(f"✓ Bulb turned ON & color {hex_code} sent")
            else:
                self._log_activity(f"✓ Color {hex_code} sent")
            save_state(self.state)

        def _on_error(exc):
            self._log_activity(f"Error setting color: {exc}", is_error=True)

        self.worker.submit(_do_color(), on_success=_on_success, on_error=_on_error)

    def _record_recent_color(self, hex_code: str):
        """Add color to recent list."""
        recents = self.state.get("recent_colors", list(DEFAULT_PRESET_COLORS))
        if hex_code in recents:
            recents.remove(hex_code)
        recents.insert(0, hex_code)
        self.state["recent_colors"] = recents[:12]

    # --- Brightness Controls ---

    def _on_brightness_slider(self, val_str: str):
        val = int(float(val_str))
        pct = int(val * 100 / 255)
        self.brightness_label.config(text=f"{pct}% ({val}/255)")
        self.state["brightness"] = val

        now = time.time()
        if now - self._last_send_time >= self._throttle_interval:
            self._last_send_time = now
            self._send_brightness(val)
        else:
            self._pending_brightness = val

    def _on_brightness_release(self):
        val = int(self.bright_slider.get())
        self._pending_brightness = None
        self._send_brightness(val)
        save_state(self.state)

    def set_brightness(self, val: int):
        val = max(1, min(255, val))
        self.state["brightness"] = val
        self.bright_slider.set(val)
        pct = int(val * 100 / 255)
        self.brightness_label.config(text=f"{pct}% ({val}/255)")
        self._send_brightness(val)
        save_state(self.state)

    def _send_brightness(self, brightness: int):
        ip = self._get_current_ip()

        async def _do_brightness():
            async with get_bulb(ip) as bulb:
                states = await bulb.updateState()
                live_power = states[0].get_state() if states and states[0] else None
                await bulb.turn_on(PilotBuilder(brightness=brightness))
                return live_power

        def _on_success(live_power):
            pct = int(brightness * 100 / 255)
            if live_power is False:
                self.state["power"] = True
                self._update_power_button_ui(True)
                self._log_activity(f"✓ Bulb turned ON & brightness set to {pct}% ({brightness}/255)")
            else:
                self._log_activity(f"✓ Brightness set to {pct}% ({brightness}/255)")
            save_state(self.state)

        def _on_error(exc):
            self._log_activity(f"Error setting brightness: {exc}", is_error=True)

        self.worker.submit(_do_brightness(), on_success=_on_success, on_error=_on_error)

    # --- Kelvin Controls ---

    def _on_kelvin_slider(self, val_str: str):
        kval = int(float(val_str))
        self.kelvin_label.config(text=f"{kval} K")
        self.state["kelvin"] = kval

        now = time.time()
        if now - self._last_send_time >= self._throttle_interval:
            self._last_send_time = now
            self._send_kelvin(kval)
        else:
            self._pending_kelvin = kval

    def _on_kelvin_release(self):
        kval = int(self.kelvin_slider.get())
        self._pending_kelvin = None
        self._send_kelvin(kval)
        save_state(self.state)

    def set_kelvin(self, kval: int):
        self.state["kelvin"] = kval
        self.kelvin_slider.set(kval)
        self.kelvin_label.config(text=f"{kval} K")
        self._send_kelvin(kval)
        save_state(self.state)

    def _send_kelvin(self, kval: int):
        ip = self._get_current_ip()

        async def _do_kelvin():
            async with get_bulb(ip) as bulb:
                states = await bulb.updateState()
                live_power = states[0].get_state() if states and states[0] else None
                await bulb.turn_on(PilotBuilder(colortemp=kval))
                return live_power

        def _on_success(live_power):
            if live_power is False:
                self.state["power"] = True
                self._update_power_button_ui(True)
                self._log_activity(f"✓ Bulb turned ON & temperature set to {kval}K")
            else:
                self._log_activity(f"✓ Temperature set to {kval}K")
            save_state(self.state)

        def _on_error(exc):
            self._log_activity(f"Error setting temperature: {exc}", is_error=True)

        self.worker.submit(_do_kelvin(), on_success=_on_success, on_error=_on_error)

    # --- Scene Controls ---

    def set_scene(self, scene_id: int, scene_name: str):
        ip = self._get_current_ip()
        self.state["scene_id"] = scene_id

        async def _do_scene():
            async with get_bulb(ip) as bulb:
                states = await bulb.updateState()
                live_power = states[0].get_state() if states and states[0] else None
                await bulb.turn_on(PilotBuilder(scene=scene_id))
                return live_power

        def _on_success(live_power):
            if live_power is False:
                self.state["power"] = True
                self._update_power_button_ui(True)
                self._log_activity(f"✓ Bulb turned ON & scene activated: {scene_name} (#{scene_id})")
            else:
                self._log_activity(f"✓ Scene activated: {scene_name} (#{scene_id})")
            save_state(self.state)

        def _on_error(exc):
            self._log_activity(f"Error activating scene: {exc}", is_error=True)

        self.worker.submit(_do_scene(), on_success=_on_success, on_error=_on_error)

    def _on_close(self):
        """Cleanup and persist state upon closing window."""
        if self._auto_ping_job:
            self.root.after_cancel(self._auto_ping_job)
        self.state["ip"] = self._get_current_ip()
        save_state(self.state)
        self.worker.stop()
        self.root.destroy()


def run_gui(target_ip: Optional[str] = None) -> int:
    """Launch the Tkinter GUI application."""
    if HAS_TKDND:
        try:
            root = TkinterDnD.Tk()
        except Exception:
            root = tk.Tk()
    else:
        root = tk.Tk()

    app = WizctlGUI(root, target_ip=target_ip)
    try:
        root.mainloop()
        return 0
    except KeyboardInterrupt:
        return 130
