#!/usr/bin/env python3
"""Regenerate the tray icons for ZЯ (bug: white glyphs on a transparent
background are invisible on a light Windows taskbar).

Every icon is a dark rounded-rect "plate" (dark slate #2B3A55) with the layout
code in solid white on top:

  tray_ru_on.dib   "RU" on the plate
  tray_en_on.dib   "EN" on the plate
  tray_ru_off.dib  "RU" + a red diagonal slash  -> auto-correction off
  tray_en_off.dib  "EN" + a red diagonal slash  -> auto-correction off

A hollow/outlined glyph was tried for the off state but it disappears at the
16 px the notification area actually renders, so the off state is marked by the
red slash instead: it survives the downscale and is unmistakable.

Because the plate covers the glyph area, the icon is legible on both a light
taskbar (dark plate stands out) and a dark one (the light edge stroke and the
white letters stand out).

The drawing is done at 8x the target size and then resampled down to 32x32, so
the letters stay crisp when Windows scales the tray icon to 16x16.

Outputs:
  data/tray_<lang>_<state>.dib          ICONREF consumed by CreateIconFromResourceEx
  data/tray_<lang>_<state>_preview.png  visual check (32 and 16 px, light + dark)
  data/app.ico                          Explorer/taskbar icon for the exe

CreateIconFromResourceEx expects an ICONREF block:
  BITMAPINFOHEADER (biSize=40, biWidth=32, biHeight=2*32=64, biBitCount=32)
  + bottom-up 32bpp BGRA XOR bitmap (32*32*4 bytes)
  + 1bpp AND mask, 32*4 = 128 bytes, bit 1 = opaque, bit 0 = transparent.
"""
import os
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
DATA = ROOT / "data"

SIZE = 32          # target icon size
SS = 8             # supersampling factor for antialiased drawing
PLATE = (43, 58, 85)      # #2B3A55 dark slate
EDGE = (96, 118, 153)     # #607699 light stroke: makes the plate visible on dark taskbars
WHITE = (247, 248, 250)
RED = (226, 76, 76)       # #E24C4C slash for the off state
RADIUS = 7               # rounded-corner radius in target pixels
SLASH = 2                # slash width in target pixels


def get_font(size: int) -> ImageFont.FreeTypeFont:
    for name in ("arialbd.ttf", "arial.ttf", "segoeui.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def glyph(text: str, canvas_size: int, font_size: int) -> Image.Image:
    """Render `text` as an RGBA glyph cropped to its ink box."""
    tmp = Image.new("RGBA", (canvas_size * 2, canvas_size * 2), (0, 0, 0, 0))
    d = ImageDraw.Draw(tmp)
    d.text((10, 10), text, font=get_font(font_size), fill=WHITE + (255,))
    return tmp.crop(d.textbbox((10, 10), text, font=get_font(font_size)))


def draw_icon(text: str, on: bool) -> Image.Image:
    n = SIZE * SS
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    last = n - 1
    radius = RADIUS * SS

    # The plate: a rounded rect that fills the whole icon, so the icon is opaque
    # everywhere except the rounded corners.
    d.rounded_rectangle([0, 0, last, last], radius=radius, fill=PLATE + (255,))
    # 1px light edge so the plate is still readable on a dark taskbar.
    d.rounded_rectangle(
        [SS // 2, SS // 2, last - SS // 2, last - SS // 2],
        radius=radius - SS // 2,
        outline=EDGE + (255,),
        width=SS,
    )

    if not on:
        # Red diagonal slash: unmistakable "auto-correction off" mark.
        d.line(
            ((2 * SS, last - 2 * SS), (last - 2 * SS, 2 * SS)),
            fill=RED + (255,),
            width=SLASH * SS,
        )

    # Fit the letters into a 22 x 14 box (target pixels), keeping proportion.
    box_w, box_h = 22 * SS, 14 * SS
    g = glyph(text, canvas_size=n, font_size=14 * SS)
    gw, gh = g.size
    scale = min(box_w / gw, box_h / gh)
    w, h = max(1, round(gw * scale)), max(1, round(gh * scale))
    g = g.resize((w, h), Image.LANCZOS)
    img.paste(g, ((n - w) // 2, (n - h) // 2), g)

    return img.resize((SIZE, SIZE), Image.LANCZOS)


def dib(img: Image.Image) -> bytes:
    w, h = img.size
    px = img.load()
    out = bytearray(
        struct.pack("<IiiHHIIiiII", 40, w, 2 * h, 1, 32, 0, w * h * 4, 0, 0, 0, 0)
    )

    # Bottom-up 32bpp BGRA XOR bitmap (a DIB stores bytes as B, G, R, A).
    for y in range(h - 1, -1, -1):
        row = bytearray()
        for x in range(w):
            r, g, b, a = px[x, y]
            row += bytes((b, g, r, a))
        out += row

    # 1bpp AND mask, bottom-up, MSB-first: 1 = opaque, 0 = transparent.
    stride = (w + 7) // 8
    for y in range(h - 1, -1, -1):
        bits = bytearray(stride)
        for x in range(w):
            if px[x, y][3] != 0:
                bits[x // 8] |= 1 << (7 - (x % 8))
        out += bytes(bits)

    return bytes(out)


def preview(img: Image.Image, path: Path) -> None:
    """Magnified check: 32 px and 16 px on a light taskbar and on a dark one."""
    small = img.resize((16, 16), Image.LANCZOS)
    cells = [
        ("32 px, light taskbar", img.resize((128, 128), Image.LANCZOS), (255, 255, 255, 255)),
        ("16 px, light taskbar", small.resize((128, 128), Image.NEAREST), (255, 255, 255, 255)),
        ("32 px, dark taskbar", img.resize((128, 128), Image.LANCZOS), (24, 24, 24, 255)),
        ("16 px, dark taskbar", small.resize((128, 128), Image.NEAREST), (24, 24, 24, 255)),
    ]
    sheet = Image.new("RGBA", (512, 152), (224, 224, 224, 255))
    draw = ImageDraw.Draw(sheet)
    for i, (label, tile_img, bg) in enumerate(cells):
        tile = Image.new("RGBA", (128, 128), bg)
        tile.paste(tile_img, (0, 0), tile_img)
        sheet.paste(tile, (i * 128, 0))
        draw.text((i * 128 + 6, 132), label, fill=(30, 30, 30, 255))
    sheet.save(path)


def ico_bytes(frames) -> bytes:
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


def main() -> None:
    DATA.mkdir(parents=True, exist_ok=True)

    variants = {
        "tray_ru_on": ("RU", True),
        "tray_en_on": ("EN", True),
        "tray_ru_off": ("RU", False),
        "tray_en_off": ("EN", False),
    }

    frames = []
    icons = {}
    for name, (text, on) in variants.items():
        img = draw_icon(text, on)
        icons[name] = img
        data = dib(img)
        assert len(data) == 40 + 32 * 32 * 4 + 32 * 4, len(data)
        (DATA / f"{name}.dib").write_bytes(data)
        img.save(DATA / f"{name}.png")
        preview(img, DATA / f"{name}_preview.png")
        print(f"wrote data/{name}.dib ({len(data)} bytes) + previews")

    combined = Image.new("RGBA", (512, 152 * 4 + 8), (224, 224, 224, 255))
    for i, name in enumerate(("tray_ru_on", "tray_en_on", "tray_ru_off", "tray_en_off")):
        combined.paste(Image.open(DATA / f"{name}_preview.png"), (0, i * 152))
    combined.save(DATA / "tray_all_preview.png")
    print("wrote data/tray_all_preview.png")

    for size in (256, 128, 64, 32, 16):
        frames.append(draw_icon("RU", True).resize((size, size), Image.LANCZOS))
    (DATA / "app.ico").write_bytes(ico_bytes(frames))
    print("wrote data/app.ico")


if __name__ == "__main__":
    main()
