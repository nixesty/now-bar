"""Draws Pebbly's icon: the tray's white pill (src/platform/tray.rs `pill_icon_bgra`: a white
capsule 84 % wide and 44 % tall, centred, anti-aliased) on a transparent square. Writes
pebbly.ico (16-256 px, each size drawn on its own pixel grid) and pebbly-256.png (the settings
window's icon).

Run: python assets/make_icon.py   (needs Pillow)"""
import math
import os

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]


def draw(size: int) -> Image.Image:
    """Same maths as `pill_icon_bgra`: coverage from the signed distance to the capsule."""
    s = float(size)
    w, h = s * 0.84, s * 0.44
    x0, y0 = (s - w) / 2, (s - h) / 2
    r = h / 2
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    px = img.load()
    for y in range(size):
        for x in range(size):
            cx, cy = x + 0.5, y + 0.5
            sx = min(max(cx, x0 + r), x0 + w - r)
            d = math.hypot(cx - sx, cy - (y0 + r)) - r
            coverage = min(max(0.5 - d, 0.0), 1.0)
            if coverage > 0:
                px[x, y] = (255, 255, 255, round(coverage * 255))
    return img


def main():
    images = [draw(n) for n in SIZES]
    images[-1].save(os.path.join(HERE, "pebbly.ico"), sizes=[(n, n) for n in SIZES], append_images=images[:-1])
    images[-1].save(os.path.join(HERE, "pebbly-256.png"))
    print("ok")


if __name__ == "__main__":
    main()
