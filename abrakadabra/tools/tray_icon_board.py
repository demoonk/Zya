"""Tray icon pictures: RU/EN, night (dark theme) and day (light theme).

Reads the icon dumps written by `cargo test --release tray` with
ZYA_TRAY_PREVIEW=1. A dump is exactly the resource the app passes to
CreateIconFromResourceEx: 40-byte BITMAPINFOHEADER + 32x32 BGRA (bottom-up)
+ 128-byte AND mask.

Writes into data/tray_icons/:
  ru_dark.png ru_light.png en_dark.png en_light.png     32x32 RGBA, transparent
  *_256.png                                            the same icons x8
  board.png                                            night + day panels
  sizes.png                                            32 / 24 / 16 px taskbar sizes
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ICON = 32
SRC = Path("data/check")
OUT = Path("data/tray_icons")
OUT.mkdir(exist_ok=True)

VARIANTS = [
    ("ru_dark", "tray_ru_auto_dark.icon", "RU · ночь"),
    ("ru_light", "tray_ru_auto_light.icon", "RU · день"),
    ("en_dark", "tray_en_auto_dark.icon", "EN · ночь"),
    ("en_light", "tray_en_auto_light.icon", "EN · день"),
    ("ru_off_dark", "tray_ru_off_dark.icon", "RU · авто выключено"),
    ("ru_off_light", "tray_ru_off_light.icon", "RU · авто выключено"),
    ("en_off_dark", "tray_en_off_dark.icon", "EN · авто выключено"),
    ("en_off_light", "tray_en_off_light.icon", "EN · авто выключено"),
]

NIGHT = (25, 21, 18)
DAY = (247, 245, 243)


def read_icon(path: Path) -> Image.Image:
    data = path.read_bytes()
    xor = data[40 : 40 + ICON * ICON * 4]
    rgba = bytearray(ICON * ICON * 4)
    for row in range(ICON):
        src = (ICON - 1 - row) * ICON * 4
        for col in range(ICON):
            b, g, r, a = xor[src + col * 4 : src + col * 4 + 4]
            o = (row * ICON + col) * 4
            rgba[o : o + 4] = bytes((r, g, b, a))
    return Image.frombytes("RGBA", (ICON, ICON), bytes(rgba))


def font(size: int) -> ImageFont.FreeTypeFont:
    for candidate in ("C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\arial.ttf"):
        try:
            return ImageFont.truetype(candidate, size)
        except OSError:
            continue
    return ImageFont.load_default()


def panel(items, cell, pad, background, title, title_colour):
    cols = len(items)
    w = cols * (cell + pad) + pad
    h = cell + 2 * pad + 40
    sheet = Image.new("RGBA", (w, h), background + (255,))
    draw = ImageDraw.Draw(sheet)
    draw.text((pad, 8), title, fill=title_colour + (255,), font=font(22))
    for i, icon in enumerate(items):
        sheet.alpha_composite(
            icon.resize((cell, cell), Image.LANCZOS), (pad + i * (cell + pad), pad + 30)
        )
    return sheet


def checker(w, h, step=12):
    img = Image.new("RGBA", (w, h), (255, 255, 255, 255))
    draw = ImageDraw.Draw(img)
    for y in range(0, h, step):
        for x in range(0, w, step):
            if ((x // step) + (y // step)) % 2:
                draw.rectangle((x, y, x + step - 1, y + step - 1), fill=(214, 214, 214, 255))
    return img


def main():
    icons = {}
    for name, src, caption in VARIANTS:
        icon = read_icon(SRC / src)
        icon.save(OUT / f"{name}.png")
        icon.resize((256, 256), Image.LANCZOS).save(OUT / f"{name}_256.png")
        icons[name] = icon
        box = [
            min(x for y in range(ICON) for x in range(ICON) if icon.getpixel((x, y))[3] > 0),
            min(y for y in range(ICON) for x in range(ICON) if icon.getpixel((x, y))[3] > 0),
            max(x for y in range(ICON) for x in range(ICON) if icon.getpixel((x, y))[3] > 0),
            max(y for y in range(ICON) for x in range(ICON) if icon.getpixel((x, y))[3] > 0),
        ]
        print(
            f"{name}: box {box[2]-box[0]+1}x{box[3]-box[1]+1} in {ICON}x{ICON}, "
            f"{sum(1 for y in range(ICON) for x in range(ICON) if icon.getpixel((x,y))[3] > 0)} ink px"
        )

    night = panel([icons["ru_dark"], icons["en_dark"]], 200, 26, NIGHT, "Ночные (тёмная тема)", (230, 230, 230))
    day = panel([icons["ru_light"], icons["en_light"]], 200, 26, DAY, "Дневные (светлая тема)", (60, 60, 60))
    off = panel([icons["ru_off_dark"], icons["en_off_dark"]], 200, 26, NIGHT, "Автоисправление выключено", (230, 230, 230))

    gap = 20
    w = max(night.width, day.width, off.width)
    h = night.height + day.height + off.height + 2 * gap
    board = Image.new("RGB", (w, h), (255, 255, 255))
    board.paste(night.convert("RGB"), (0, 0))
    board.paste(day.convert("RGB"), (0, night.height + gap))
    board.paste(off.convert("RGB"), (0, night.height + gap + day.height + gap))
    board.save(OUT / "board.png")

    # Checkerboard sheet: proves the background really is transparent.
    cell, pad = 120, 18
    cols = 8
    sheet = checker(cols * (cell + pad) + pad, cell + 2 * pad)
    for i, (name, _, _) in enumerate(VARIANTS):
        sheet.alpha_composite(icons[name].resize((cell, cell), Image.LANCZOS), (pad + i * (cell + pad), pad))
    sheet.convert("RGB").save(OUT / "transparent_sheet.png")

    # Sizes as the taskbar draws them.
    sizes = (32, 24, 16)
    cell = 34
    pad = 14
    w = 4 * (cell + pad) + pad
    h = len(sizes) * (cell + pad) + pad
    strip = Image.new("RGB", (w, h), (255, 255, 255))
    for i, name in enumerate(("ru_dark", "ru_light", "en_dark", "en_light")):
        for j, size in enumerate(sizes):
            bg = NIGHT if j == 0 else DAY
            cell_img = Image.new("RGBA", (size, size), bg + (255,))
            cell_img.alpha_composite(icons[name].resize((size, size), Image.LANCZOS))
            x = pad + i * (cell + pad) + (cell - size) // 2
            y = pad + j * (cell + pad) + (cell - size) // 2
            strip.paste(cell_img.convert("RGB"), (x, y))
    strip.save(OUT / "sizes.png")

    print("wrote:", ", ".join(p.name for p in sorted(OUT.glob("*.png"))))


if __name__ == "__main__":
    main()
