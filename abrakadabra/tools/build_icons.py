#!/usr/bin/env python3
"""Generate the icons for Abrakadabra.

Inputs:  ../108.png  (the app artwork: a keycap with a black Z and a red Я)
Outputs: data/app.ico        multi-size icon embedded into the executable
         data/tray_<variant>.raw   32x32 bottom-up BGRA (CreateIcon friendly)
         data/tray_mask.raw        32 rows x 4 bytes, 1 bit per pixel

Variants: ru / en  x  on / off  (on = auto-correction active, off = gray).
"""
import io
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
MUTED = (86, 86, 86)


def keycap(size, bg, glyph, glyph_color):
    """A rounded keycap with one big letter, matching the 108.png artwork."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = size // 12
    r = size // 6
    d.rounded_rectangle([pad, pad, size - pad, size - pad], radius=r, fill=bg)
    # subtle inner highlight, like the photographed key
    inset = size // 16
    d.rounded_rectangle(
        [pad + inset, pad + inset, size - pad - inset, size - pad - inset],
        fill=tuple(min(255, c + 8) for c in bg),
        radius=max(1, r - 2),
    )
    font = ImageFont.truetype("arial.ttf", int(size * 0.62))
    w = d.textlength(glyph, font=font)
    d.text(((size - w) / 2, size * 0.10), glyph, font=font, fill=glyph_color)
    return img


def to_raw(img):
    """BGRA, bottom-up, exactly what CreateIcon expects."""
    out = bytearray()
    px = img.load()
    for y in range(img.height - 1, -1, -1):
        for x in range(img.width):
            r, g, b, a = px[x, y]
            out += bytes((b, g, r, a))
    return bytes(out)


def mask_raw(img):
    """AND mask: 1 bit per pixel, 4 bytes per 32-pixel row.
    0 = opaque, 1 = transparent (background shows through)."""
    px = img.load()
    out = bytearray()
    for y in range(img.height):
        row = 0
        for x in range(img.width):
            if px[x, y][3] == 0:
                row |= 1 << (31 - x)
        out += struct.pack("<I", row)
    return bytes(out)


def main():
    os.makedirs(DATA, exist_ok=True)
    if not os.path.exists(SOURCE):
        print("108.png not found, drawing a plain keycap")
        base = None
    else:
        base = Image.open(SOURCE).convert("RGBA")
        # The artwork is a slightly rotated key; trim the transparent margin.
        bbox = base.getbbox()
        base = base.crop(bbox)

    variants = {
        "ru_on": ("Я", RED, WHITE),
        "en_on": ("Z", INK, WHITE),
        "ru_off": ("Я", MUTED, GRAY),
        "en_off": ("Z", MUTED, GRAY),
    }

    mask = None
    for name, (glyph, color, bg) in variants.items():
        img = keycap(32, bg, glyph, color)
        img.save(os.path.join(DATA, f"tray_{name}.png"))
        with open(os.path.join(DATA, f"tray_{name}.raw"), "wb") as fh:
            fh.write(to_raw(img))
        mask = mask_raw(img)

    with open(os.path.join(DATA, "tray_mask.raw"), "wb") as fh:
        fh.write(mask)

    sizes = [256, 128, 64, 32, 16]
    if base is not None:
        frames = [base.resize((s, s), Image.LANCZOS) for s in sizes]
    else:
        frames = [keycap(s, WHITE, "Я", RED) for s in sizes]
    with open(os.path.join(DATA, "app.ico"), "wb") as fh:
        fh.write(ico_bytes(frames))
    print("wrote data/app.ico and 4 tray variants")


def bmp_frame(img):
    """Classic BITMAPINFOHEADER + 32bpp DIB + AND mask (what windres expects)."""
    w, h = img.size
    px = img.load()
    dib = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    body = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = px[x, y]
            body += bytes((b, g, r, a if a else 0))
    stride = (w + 7) // 8
    mask = bytearray()
    for y in range(h):
        row = 0
        for x in range(w):
            if px[x, y][3] == 0:
                row |= 1 << x
        if stride == 4:
            mask += struct.pack("<I", row)
        else:
            mask += row.to_bytes(stride, "little")
    return dib + bytes(body) + bytes(mask)


def ico_bytes(frames):
    """Write a multi-image .ico container."""
    header_size = 6 + 16 * len(frames)
    dir_entries = []
    body = b""
    for img in frames:
        data = bmp_frame(img)
        w, h = img.size
        dir_entries.append(
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
    return struct.pack("<HHH", 0, 1, len(frames)) + b"".join(dir_entries) + body


if __name__ == "__main__":
    main()
