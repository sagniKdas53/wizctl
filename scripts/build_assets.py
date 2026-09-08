"""Script to process and generate all icon resolutions and variants for wizctl."""

from pathlib import Path
from PIL import Image, ImageEnhance

SOURCE_IMG = "/home/sagnik/.gemini/antigravity-ide/brain/f5eb4efd-5b54-4b1f-9823-e8aaab730fc5/bulb_icon_512_transparent.png"
ASSETS_DIR = Path("/home/sagnik/Projects/wizctl/src/wizctl/assets")
ASSETS_DIR.mkdir(parents=True, exist_ok=True)

def main():
    img = Image.open(SOURCE_IMG).convert("RGBA")
    r, g, b, a = img.split()
    rgb = Image.merge("RGB", (r, g, b))
    
    # Save base 512x512 icon
    base_512 = img.resize((512, 512), Image.Resampling.LANCZOS)
    base_512.save(ASSETS_DIR / "icon_512.png", format="PNG")
    base_512.save(ASSETS_DIR / "icon.png", format="PNG")
    
    sizes = [256, 128, 64, 48, 32, 24, 22, 16]
    for s in sizes:
        resized = img.resize((s, s), Image.Resampling.LANCZOS)
        resized.save(ASSETS_DIR / f"icon_{s}.png", format="PNG")
        
    # Save Windows ICO containing multiple resolutions
    base_512.save(
        ASSETS_DIR / "icon.ico",
        format="ICO",
        sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)]
    )
    
    # Panel Icons (for Genmon / Panel tray)
    # 1. Bulb ON (vibrant)
    panel_on = img.resize((32, 32), Image.Resampling.LANCZOS)
    panel_on.save(ASSETS_DIR / "panel_bulb_on.png", format="PNG")
    
    # 2. Bulb OFF (desaturated, unlit frosted dome)
    gray_rgb = rgb.convert("L").convert("RGB")
    dimmed_rgb = ImageEnhance.Brightness(gray_rgb).enhance(0.6)
    panel_off_full = Image.merge("RGBA", (*dimmed_rgb.split(), a))
    panel_off = panel_off_full.resize((32, 32), Image.Resampling.LANCZOS)
    panel_off.save(ASSETS_DIR / "panel_bulb_off.png", format="PNG")
    
    # 3. Bulb OFFLINE (muted with subtle opacity)
    offline_rgb = ImageEnhance.Brightness(gray_rgb).enhance(0.4)
    # slightly reduce alpha for offline/unreachable indicator
    dimmed_a = ImageEnhance.Brightness(a).enhance(0.7)
    panel_offline_full = Image.merge("RGBA", (*offline_rgb.split(), dimmed_a))
    panel_offline = panel_offline_full.resize((32, 32), Image.Resampling.LANCZOS)
    panel_offline.save(ASSETS_DIR / "panel_bulb_offline.png", format="PNG")
    
    print(f"✓ All transparent assets successfully generated in {ASSETS_DIR}")

if __name__ == "__main__":
    main()
