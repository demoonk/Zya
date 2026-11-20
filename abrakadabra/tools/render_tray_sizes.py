"""Render the raw tray `.icon` dumps as they appear in the taskbar (24 px and 16 px)."""
import io
import sys

from PIL import Image, ImageDraw

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

ICON = 32


def read_icon(path):
    data = open(path, "rb").read()
    pixels = []
    for y in range(ICON):
        row = []
        for x in range(ICON):
            off = 40 + (ICON - 1 - y) * 128 + x * 4
            b, g, r, a = data[off], data[off + 1], data[off + 2], data[off + 3]
            row.append((r, g, b, a))
        pixels.append(row)
    return pixels


def composite(pixels, size, bg):
    out = Image.new("RGB", (size, size), bg)
    draw = ImageDraw.Draw(out)
    for y in range(size):
        for x in range(size):
            # Nearest-neighbour downsample, then alpha blend onto the taskbar colour.
            r, g, b, a = pixels[(y * ICON) // size][(x * ICON) // size]
            t = a / 255.0
            out.putpixel(
                (x, y), tuple(int(c * t + bg[i] * (1 - t)) for i, c in enumerate((r, g, b)))
            )
    return out


def main():
    names = [
        ("tray_ru_on_dark", (25, 21, 18)),
        ("tray_ru_on_light", (247, 245, 243)),
        ("tray_en_on_dark", (25, 21, 18)),
        ("tray_en_on_light", (247, 245, 243)),
        ("tray_ru_off_dark", (25, 21, 18)),
    ]
    sizes = [32, 24, 16]
    rows = []
    for name, bg in names:
        pixels = read_icon(f"data/check/{name}.icon")
        cells = [composite(pixels, s, bg) for s in sizes]
        pad = 12
        width = sum(c.width for c in cells) + pad * (len(cells) + 1)
        height = max(c.height for c in cells) + 2 * pad
        row = Image.new("RGB", (width, height), (32, 32, 32))
        x = pad
        for c in cells:
            row.paste(c, (x, pad))
            x += c.width + pad
        rows.append((name, row))

    width = max(r.width for _, r in rows)
    gap = 16
    sheet = Image.new("RGB", (width, sum(r.height for _, r in rows) + gap * (len(rows) + 1)), (20, 20, 20))
    y = gap
    for _, r in rows:
        sheet.paste(r, (gap, y))
        y += r.height + gap
    sheet.save("data/check/tray_rendered_sizes.png")
    print("saved data/check/tray_rendered_sizes.png", sheet.size)


if __name__ == "__main__":
    main()
