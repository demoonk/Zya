"""Locate the zя tray icon by diffing two taskbar screenshots.

Usage:
    python tools/tray_diff.py base.png new.png out_crop.png
"""
import sys

from PIL import Image


def main():
    base = Image.open(sys.argv[1]).convert("RGB")
    new = Image.open(sys.argv[2]).convert("RGB")
    out = sys.argv[3] if len(sys.argv) > 3 else "tray_diff.png"

    diff = []
    for y in range(base.height):
        for x in range(base.width):
            if base.getpixel((x, y)) != new.getpixel((x, y)):
                diff.append((x, y))
    if not diff:
        print("no difference: the tray icon did not change")
        return
    xs = [x for x, _ in diff]
    ys = [y for _, y in diff]
    print("changed region", min(xs), min(ys), max(xs), max(ys), "pixels", len(diff))
    crop = new.crop((max(0, min(xs) - 6), max(0, min(ys) - 6), min(new.width, max(xs) + 7), min(new.height, max(ys) + 7)))
    crop = crop.resize((crop.width * 6, crop.height * 6), Image.NEAREST)
    crop.save(out)
    print("saved", out, crop.size)


if __name__ == "__main__":
    main()
