"""The README's pictures: lays readmesheet's renders, a settings shot and a setup shot out on
wallpaper-like panels and writes them to assets/readme/. Everything is at 2x; the README shows
it at half size, so it stays sharp on high-DPI screens.

    target\\quick\\readmesheet.exe <sheets>
    set PEBBLY_SETTINGS_SHOTS=<shots> && target\\quick\\pebbly-settings.exe
    set PEBBLY_SETUP_SHOTS=<setup> && target\\quick\\pebbly-setup.exe
    python tools\\readme-images.py <sheets> <shots>\\page-0.png <setup>\\1-kur.png

Pillow only (no numpy): the wallpapers are worked out small, then scaled up and blurred.
"""
import math
import os
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'assets', 'readme')
FONTS = os.path.join(os.environ.get('WINDIR', r'C:\Windows'), 'Fonts')


def wallpaper(w, h, base, blobs):
    """A dark base with soft colour blobs ((x, y) in 0..1, radius, rgb), One UI-like."""
    sw, sh = max(w // 12, 2), max(h // 12, 2)
    small = Image.new('RGB', (sw, sh))
    px = small.load()
    for y in range(sh):
        for x in range(sw):
            u, v = x / sw, y / sh * h / w
            c = list(base)
            for (bx, by), r, col in blobs:
                by = by * h / w
                k = math.exp(-3.0 * ((u - bx) ** 2 + (v - by) ** 2) / (r * r))
                c = [c[i] * (1 - k) + col[i] * k for i in range(3)]
            px[x, y] = tuple(int(round(min(max(ch, 0), 255))) for ch in c)
    return small.resize((w, h), Image.BICUBIC).filter(ImageFilter.GaussianBlur(10)).convert('RGBA')


def rounded(im, r):
    """`im` with its corners cut round (anti-aliased: the mask is drawn at 4x)."""
    mask = Image.new('L', (im.width * 4, im.height * 4), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, mask.width - 1, mask.height - 1), r * 4, fill=255)
    mask = mask.resize(im.size, Image.LANCZOS)
    out = im.copy()
    out.putalpha(ImageChops.multiply(im.getchannel('A'), mask) if im.mode == 'RGBA' else mask)
    return out


def paste(canvas, im, x, y):
    """Alpha-composites `im` at (x, y), cropping whatever falls off the canvas."""
    x0, y0 = max(x, 0), max(y, 0)
    x1, y1 = min(x + im.width, canvas.width), min(y + im.height, canvas.height)
    if x1 > x0 and y1 > y0:
        canvas.alpha_composite(im.crop((x0 - x, y0 - y, x1 - x, y1 - y)), (x0, y0))


def drop(canvas, im, x, y, blur=26, alpha=0.5, dy=16):
    """`im` with a soft shadow under it, cast from its own shape."""
    pad = blur * 3
    shade = Image.new('L', (im.width + 2 * pad, im.height + 2 * pad), 0)
    shade.paste(im.getchannel('A').point(lambda v: int(v * alpha)), (pad, pad))
    black = Image.new('RGBA', shade.size, (0, 0, 0, 255))
    black.putalpha(shade.filter(ImageFilter.GaussianBlur(blur)))
    paste(canvas, black, x - pad, y - pad + dy)
    paste(canvas, im, x, y)


def window(shot, radius):
    """A window shot with Windows 11's round corners and hairline border."""
    im = rounded(shot.convert('RGBA'), radius)
    edge = Image.new('RGBA', im.size, (0, 0, 0, 0))
    ImageDraw.Draw(edge).rounded_rectangle((0, 0, im.width - 1, im.height - 1), radius, outline=(255, 255, 255, 30), width=1)
    im.alpha_composite(edge)
    return im


# The panels' wallpapers: night blues with a warm glow, after the drawn cover's colours.
DUSK = ((14, 14, 30), [((0.18, 0.25), 0.55, (122, 52, 120)), ((0.85, 0.15), 0.5, (48, 58, 150)), ((0.70, 1.00), 0.55, (24, 86, 110))])
DEEP = ((13, 13, 20), [((0.10, 0.10), 0.6, (58, 34, 92)), ((0.95, 0.90), 0.6, (24, 52, 96))])
WARM = ((15, 12, 22), [((0.85, 0.15), 0.55, (110, 46, 84)), ((0.10, 0.95), 0.6, (40, 40, 110))])
RADIUS = 36


def hero(sheets):
    """The card grown out of the media pill in the taskbar, other live pills beside it."""
    w, h, bar = 1760, 1000, 96
    canvas = wallpaper(w, h, *DUSK)
    # The taskbar: acrylic (the wallpaper blurred under a dark tint) with a hairline on top.
    strip = canvas.crop((0, h - bar, w, h)).filter(ImageFilter.GaussianBlur(30))
    strip.alpha_composite(Image.new('RGBA', strip.size, (30, 30, 34, 214)))
    ImageDraw.Draw(strip).line((0, 0, w, 0), fill=(255, 255, 255, 22), width=2)
    canvas.alpha_composite(strip, (0, h - bar))
    # The tray: the chevron, Wi-Fi and volume glyphs, the clock.
    icons = ImageFont.truetype(os.path.join(FONTS, 'SegoeIcons.ttf'), 32)
    text = ImageFont.truetype(os.path.join(FONTS, 'SegUIVar.ttf'), 24)
    d = ImageDraw.Draw(canvas)
    right = w - 44
    for line, y in (('20:42', h - bar + 22), ('24.10.2026', h - bar + 52)):
        d.text((right, y), line, font=text, fill=(255, 255, 255, 235), anchor='ra')
    x = right - 150
    for glyph in ('\uE767', '\uE701', '\uE70E'):
        d.text((x, h - bar // 2), glyph, font=icons, fill=(255, 255, 255, 235), anchor='rm')
        x -= 58
    card = Image.open(os.path.join(sheets, 'card-media.png'))
    pill = Image.open(os.path.join(sheets, 'pill-media.png'))
    cx = 160 + card.width // 2
    paste(canvas, pill, cx - pill.width // 2, h - bar + (bar - pill.height) // 2)
    drop(canvas, card, 160, h - bar - 24 - card.height, blur=36, alpha=0.55, dy=24)
    names = ['claude', 'match', 'timer', 'buds', 'weather', 'power']
    pills = [Image.open(os.path.join(sheets, f'pill-{n}.png')) for n in names]
    gap = 40
    top = (h - bar - 24 - card.height) + (card.height - (sum(p.height for p in pills) + gap * (len(pills) - 1))) // 2
    for p in pills:
        drop(canvas, p, 1000, top, blur=18, alpha=0.45, dy=10)
        top += p.height + gap
    return rounded(canvas, RADIUS)


def columns(sheets, left, right, wall):
    """Two columns of cards from the top, on a wallpaper panel."""
    cards = [[Image.open(os.path.join(sheets, f'card-{n}.png')) for n in col] for col in (left, right)]
    col_w, gap, margin = max(c.width for col in cards for c in col), 48, 64
    heights = [sum(c.height for c in col) + gap * (len(col) - 1) for col in cards]
    w, h = 2 * col_w + gap + 2 * margin, max(heights) + 2 * margin
    canvas = wallpaper(w, h, *wall)
    for i, col in enumerate(cards):
        x0 = margin + i * (col_w + gap)
        y = margin
        for c in col:
            drop(canvas, c, x0 + (col_w - c.width) // 2, y)
            y += c.height + gap
    return rounded(canvas, RADIUS)


def music(sheets):
    """The player and its lyrics side by side, the pill under them."""
    cards = [Image.open(os.path.join(sheets, f'card-{n}.png')) for n in ('media', 'lyrics')]
    pill = Image.open(os.path.join(sheets, 'pill-media.png'))
    gap, margin = 48, 64
    w = sum(c.width for c in cards) + gap + 2 * margin
    h = max(c.height for c in cards) + gap + pill.height + 2 * margin
    canvas = wallpaper(w, h, *WARM)
    x = margin
    for c in cards:
        drop(canvas, c, x, margin)
        x += c.width + gap
    drop(canvas, pill, (w - pill.width) // 2, h - margin - pill.height, blur=18, alpha=0.45, dy=10)
    return rounded(canvas, RADIUS)


def pills(sheets):
    """Two columns of pills, from their left edges, on the taskbar's colour."""
    names = ['media', 'claude', 'match', 'timer', 'weather', 'rain', 'power', 'buds', 'privacy', 'game', 'shelf', 'usage', 'clock', 'volume']
    ims = [Image.open(os.path.join(sheets, f'pill-{n}.png')) for n in names]
    col_w, row_h, gap, margin = max(p.width for p in ims), ims[0].height, 32, 64
    rows = (len(ims) + 1) // 2
    w, h = 2 * col_w + 64 + 2 * margin, rows * row_h + (rows - 1) * gap + 2 * margin
    canvas = wallpaper(w, h, (24, 24, 28), [((0.5, -0.2), 0.9, (40, 40, 52))])
    for i, p in enumerate(ims):
        col, row = i % 2, i // 2
        x0 = margin + col * (col_w + 64)
        paste(canvas, p, x0, margin + row * (row_h + gap))
    return rounded(canvas, RADIUS)


def framed(shot, wall, margin_x, margin_y, radius):
    """A window shot on a wallpaper panel."""
    win = window(Image.open(shot), radius)
    canvas = wallpaper(win.width + 2 * margin_x, win.height + 2 * margin_y, *wall)
    drop(canvas, win, margin_x, margin_y, blur=40, alpha=0.6, dy=24)
    return rounded(canvas, RADIUS)


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    sheets, settings_shot, setup_shot = sys.argv[1:]
    os.makedirs(OUT, exist_ok=True)
    pictures = {
        'hero': hero(sheets),
        'music': music(sheets),
        'day': columns(sheets, ['brief', 'timer'], ['weather', 'wellbeing'], DEEP),
        'live': columns(sheets, ['match', 'buds'], ['claude', 'shelf'], DUSK),
        'pills': pills(sheets),
        # The settings window and the setup are shot at 125 %: their corners are 8 DIP.
        'settings': framed(settings_shot, DEEP, 96, 80, 10),
        'setup': framed(setup_shot, WARM, 64, 56, 10),
    }
    for name, im in pictures.items():
        path = os.path.normpath(os.path.join(OUT, f'{name}.png'))
        im.save(path, optimize=True)
        print(path, im.size, os.path.getsize(path) // 1024, 'KB')


if __name__ == '__main__':
    main()
