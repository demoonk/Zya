#!/usr/bin/env python3
"""Regenerate the tray icons in the canonical ICONREF form.

CreateIconFromResourceEx expects a BITMAPINFOHEADER with biHeight = 2 * height,
followed by a bottom-up 32 bpp DIB and a 1 bpp AND mask (1 = opaque).
The previous raw BGRA buffers handed to CreateIcon came out with swapped
channels and a flipped glyph, so we now ship complete DIB blocks instead.

Outputs:  data/tray_<variant>.dib   (40-byte header + pixels + mask)
          data/app.ico              multi-size container for the executable
"""
import os
import struct
import sys

from PIL import Image, ImageDraw, ImageFont

sys.stdout.reconfigure(encoding="utf-8")

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
DATA = os.path.join(ROOT, "data")
SOURCE = os.path.join(os.path.dirname(ROOT), "108.png")

WHITE = (246, 246, 246)
GRAY = (150, 150, 150)
INK = (26, 26, 26)
RED = (199, 36, 42)
BLUE = (42, 90, 199)
MUTED = (86, 86, 86)


def keycap(size, bg, glyph, glyph_color):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = max(1, size // 12)
    r = max(1, size // 6)
    d.rounded_rectangle([pad, pad, size - pad, size - pad], radius=r, fill=bg)
    inset = max(1, size // 16)
    d.rounded_rectangle(
        [pad + inset, pad + inset, size - pad - inset, size - pad - inset],
        fill=tuple(min(255, c + 8) for c in bg),
        radius=max(1, r - 2),
    )
    # "RU"/"EN" in two letters, because a single З/Я is not recognisable at the
    # 16 px the notification area actually gives us.
    font = ImageFont.truetype("arialbd.ttf", int(size * 0.44))
    w = d.textlength(glyph, font=font)
    bbox = d.textbbox((0, 0), glyph, font=font)
    h = bbox[3] - bbox[1]
    d.text(((size - w) / 2, (size - h) / 2 - bbox[1]), glyph, font=font, fill=glyph_color)
    return img


def dib(img):
    """ICONREF: header + bottom-up 32bpp pixels + 1bpp AND mask (1 = opaque).

    The explorer shell reads the XOR bitmap of an icon resource as RGBA, not as
    the BGRA that a plain DIB would be. Writing BGR gave a blue "Я" that came
    out orange.
    """
    w, h = img.size
    px = img.load()
    out = bytearray(struct.pack("<IiiHHIIiiII", 40, w, 2 * h, 1, 32, 0, 0, 0, 0, 0, 0))
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


def bmp_frame(img):
    """A .ico directory entry body (same as dib, but height doubled already)."""
    return dib(img)


def ico_bytes(frames):
    header_size = 6 + 16 * len(frames)
    entries = []
    body = b""
    for img in frames:
        data = bmp_frame(img)
        w, h = img.size
        entries.append(
            struct.pack(
                "<BBBBHHII", w & 0xFF, h & 0xFF, 0, 0, 1, 32, len(data), header_size + len(body)
            )
        )
        body += data
    return struct.pack("<HHH", 0, 1, len(frames)) + b"".join(entries) + body


def main():
    os.makedirs(DATA, exist_ok=True)
    variants = {
        "tray_ru_on": ("RU", RED, WHITE),
        "tray_en_on": ("EN", BLUE, WHITE),
        "tray_ru_off": ("RU", MUTED, GRAY),
        "tray_en_off": ("EN", MUTED, GRAY),
    }
    for name, (glyph, color, bg) in variants.items():
        img = keycap(32, bg, glyph, color)
        img.save(os.path.join(DATA, f"{name}.png"))
        with open(os.path.join(DATA, f"{name}.dib"), "wb") as fh:
            fh.write(dib(img))
        print(f"wrote data/{name}.dib ({len(dib(img))} bytes)")

    base = None
    if os.path.exists(SOURCE):
        base = Image.open(SOURCE).convert("RGBA")
        base = base.crop(base.getbbox())
    sizes = [256, 128, 64, 32, 16]
    frames = [base.resize((s, s), Image.LANCZOS) if base else keycap(s, WHITE, "Я", RED) for s in sizes]
    with open(os.path.join(DATA, "app.ico"), "wb") as fh:
        fh.write(ico_bytes(frames))
    print("wrote data/app.ico")


if __name__ == "__main__":
    main()
