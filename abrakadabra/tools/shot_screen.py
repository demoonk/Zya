"""Capture the whole screen or a region to PNG with Pillow.

Usage:
    python tools/shot_screen.py out.png
    python tools/shot_screen.py out.png 100 200 900 700
"""
import sys

from PIL import ImageGrab


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "screen.png"
    region = (0, 0)
    if len(sys.argv) > 5:
        region = tuple(int(x) for x in sys.argv[2:6])
    image = ImageGrab.grab(bbox=region if len(region) == 4 else None)
    image.save(out)
    print("saved", out, image.size)


if __name__ == "__main__":
    main()
