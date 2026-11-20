"""Render every key-cap icon frame embedded in an executable.

Resources built by windres are laid out as contiguous BITMAPINFOHEADER blocks
(height is doubled because the 1-bit AND mask follows the pixels), so a scan
for those headers is more robust than walking the PE resource directory.

Usage: python tools/dump_pe_icons.py <exe> [out_dir]
"""
import struct
import sys

from PIL import Image


def frames(data: bytes):
    position = 0
    while True:
        position = data.find(b"\x28\x00\x00\x00", position)
        if position < 0:
            return
        width, height = struct.unpack("<ii", data[position + 4 : position + 12])
        planes, bit_count = struct.unpack("<HH", data[position + 12 : position + 16])
        if 0 < width <= 256 and height == 2 * width and bit_count == 32:
            yield position, width
        position += 4


def bitmap(data: bytes, offset: int, width: int) -> Image.Image:
    pixels = data[offset + 40 : offset + 40 + width * width * 4]
    image = Image.new("RGB", (width, width))
    for y in range(width):
        for x in range(width):
            start = (width - 1 - y) * width * 4 + x * 4
            blue, green, red, _ = pixels[start : start + 4]
            image.putpixel((x, y), (red, green, blue))
    return image


def main() -> None:
    exe = sys.argv[1]
    out_dir = sys.argv[2] if len(sys.argv) > 2 else "data/check"
    data = open(exe, "rb").read()
    seen = set()
    for offset, width in frames(data):
        if width in seen:
            continue
        seen.add(width)
        image = bitmap(data, offset, width)
        colors = [image.getpixel((x, y)) for y in range(width) for x in range(width)]
        red = sum(1 for r, g, b in colors if r > 120 and b < 90)
        blue = sum(1 for r, g, b in colors if b > 120 and r < 90)
        print(f"{exe} frame {width}x{width} at {offset}: red={red} blue={blue}")
        image.resize((256, 256), Image.NEAREST).save(f"{out_dir}/exe_icon_{width}.png")


if __name__ == "__main__":
    main()
