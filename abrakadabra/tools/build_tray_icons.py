#!/usr/bin/env python3
"""Regenerate the tray icons for ZЯ.

The tray icon is deliberately tiny, so the glyph is drawn as large as possible
in the 32x32 notification-area icon. The background is transparent. On states
use a solid white glyph; off states use a hollow white glyph.

Outputs:
  data/tray_ru_on.dib / data/tray_ru_off.dib
  data/tray_en_on.dib / data/tray_en_off.dib
  data/app.ico

CreateIconFromResourceEx expects an ICONREF block:
  BITMAPINFOHEADER (biHeight = 2 * icon height) + bottom-up 32bpp RGBA + 1bpp AND mask.
"""
import os
import struct

from PIL import Image, ImageDraw, ImageFont, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
DATA = os.path.join(ROOT, "data")
SOURCE = os.path.join(os.path.dirname(ROOT), "108.png")

WHITE = (246, 246, 246)
INK = (26, 26, 26)
RED = (199, 36, 42)


def get_font():
    for name in ("arialbd.ttf", "arial.ttf", "segoeui.ttf"):
        try:
            return ImageFont.truetype(name, 96)
        except OSError:
            continue
    return ImageFont.load_default()


def draw_text(text, filled):
    """Render the layout code at its natural aspect ratio, then center it in 32x32.

    The previous version stretched the cropped glyph to the whole square, which
    squashed `RU`/`EN` horizontally. Fitting by width and keeping the proportion
    makes the letters read as normal letters.
    """
    target = 32
    font = get_font()
    canvas = Image.new("RGBA", (300, 300), (0, 0, 0, 0))
    d = ImageDraw.Draw(canvas)

    stroke_width = 2
    text_pos = (30, 30)

    if filled:
        d.text(
            text_pos,
            text,
            font=font,
            fill=(255, 255, 255, 255),
            stroke_width=stroke_width,
            stroke_fill=(0, 0, 0, 255),
        )
    else:
        d.text(
            text_pos,
            text,
            font=font,
            fill=(0, 0, 0, 0),
            stroke_width=stroke_width,
            stroke_fill=(255, 255, 255, 255),
        )

    bbox = d.textbbox(text_pos, text, font=font, stroke_width=stroke_width)
    glyph = canvas.crop(bbox)

    w, h = glyph.size
    if w <= 0 or h <= 0:
        return glyph.resize((target, target), Image.LANCZOS)

    # Keep the glyph's own proportion: it touches the left and right edges of
    # the icon, but it is never stretched.
    margin = 0
    available = target - 2 * margin
    scale = min(available / w, available / h)
    nw = max(1, round(w * scale))
    nh = max(1, round(h * scale))
    scaled = glyph.resize((nw, nh), Image.LANCZOS)

    out = Image.new("RGBA", (target, target), (0, 0, 0, 0))
    out.paste(scaled, ((target - nw) // 2, (target - nh) // 2))
    return out


def dib(img):
    w, h = img.size
    px = img.load()
    out = bytearray(struct.pack("<IiiHHIIiiII", 40, w, 2 * h, 1, 32, 0, 0, 0, 0, 0, 0))

    # Bottom-up XOR bitmap. Windows icon resources read this as RGBA, not BGRA.
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = px[x, y]
            out += bytes((r, g, b, a))

    stride = (w + 7) // 8
    for y in range(h - 1, -1, -1):
        row = [0] * stride
        for x in range(w):
            if px[x, y][3] != 0:
                row[x // 8] |= 1 << (7 - (x % 8))
        out += bytes(row)

    return bytes(out)


def ico_bytes(frames):
    header_size = 6 + 16 * len(frames)
    entries = []
    body = b""

    for img in frames:
        data = dib(img)
        w, h = img.size
        entries.append(
            struct.pack(
                "<BBBBHHII",
                w & 0xFF,
                h & 0xFF,
                0,
                0,
                1,
                32,
                len(data),
                header_size + len(body),
            )
        )
        body += data

    return struct.pack("<HHH", 0, 1, len(frames)) + b"".join(entries) + body


def main():
    os.makedirs(DATA, exist_ok=True)

    variants = {
        "tray_ru_on": ("RU", True),
        "tray_ru_off": ("RU", False),
        "tray_en_on": ("EN", True),
        "tray_en_off": ("EN", False),
    }

    for name, (text, filled) in variants.items():
        img = draw_text(text, filled)
        img.save(os.path.join(DATA, f"{name}.png"))
        data = dib(img)
        with open(os.path.join(DATA, f"{name}.dib"), "wb") as fh:
            fh.write(data)
        print(f"wrote data/{name}.png and data/{name}.dib ({len(data)} bytes)")

    base = None
    if os.path.exists(SOURCE):
        base = Image.open(SOURCE).convert("RGBA")
        base = base.crop(base.getbbox())

    sizes = [256, 128, 64, 32, 16]
    frames = []
    for s in sizes:
        if base:
            frames.append(base.resize((s, s), Image.LANCZOS))
        else:
            frames.append(draw_text("RU", True).resize((s, s), Image.LANCZOS))

    with open(os.path.join(DATA, "app.ico"), "wb") as fh:
        fh.write(ico_bytes(frames))
    print("wrote data/app.ico")


if __name__ == "__main__":
    main()
