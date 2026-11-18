#!/usr/bin/env python3
"""Generate a 32x32 tray icon as raw BGRA (bottom-up) without any third-party
library. The glyph is a hand-drawn 'R'/'E' split: blue half = Russian side,
red half = English side, with a white <-> arrow in the middle.

Outputs:
  abrakadabra/data/tray.raw   32*32*4 bytes, bottom-up BGRA
  abrakadabra/data/tray.mask  32 rows * 4 bytes (128), all 0xFF = fully opaque
"""
import os

SIZE = 32
OUT = os.path.join(os.path.dirname(__file__), "..", "abrakadabra", "data")

BLUE = (59, 130, 246)
RED = (239, 68, 68)
WHITE = (255, 255, 255)
DARK = (24, 28, 38)


def paint():
    px = [[DARK for _ in range(SIZE)] for _ in range(SIZE)]  # px[y][x], y = top row

    for y in range(SIZE):
        for x in range(SIZE):
            # rounded corners
            if x < 3 and y < 3 and (3 - x) ** 2 + (3 - y) ** 2 > 10:
                continue
            if x >= SIZE - 3 and y < 3 and (x - (SIZE - 4)) ** 2 + (3 - y) ** 2 > 10:
                continue
            if x < 3 and y >= SIZE - 3 and (3 - x) ** 2 + (y - (SIZE - 4)) ** 2 > 10:
                continue
            if x >= SIZE - 3 and y >= SIZE - 3 and (x - (SIZE - 4)) ** 2 + (y - (SIZE - 4)) ** 2 > 10:
                continue
            px[y][x] = BLUE if x < SIZE // 2 else RED

    # "Я" on the left, "A" on the right, drawn as 5x7 blocks
    ya = {
        (0, 1), (1, 1), (2, 1), (3, 1), (4, 1),
        (0, 0), (1, 2), (2, 3), (3, 4), (4, 4),
        (0, 4), (1, 5), (2, 6),
    }
    aa = {
        (0, 2), (1, 1), (2, 0), (3, 1), (4, 2),
        (1, 3), (2, 3), (3, 3),
        (0, 4), (1, 4), (2, 4), (3, 4), (4, 4),
        (0, 6), (4, 6),
    }
    for (gy, gx) in ya:
        px[9 + gy][3 + gx] = WHITE
    for (gy, gx) in aa:
        px[9 + gy][19 + gx] = WHITE

    # arrow in the middle
    for y in range(14, 18):
        for x in range(13, 19):
            px[y][x] = WHITE
    for x in (12, 19):
        px[15][x] = WHITE

    return px


def main():
    px = paint()
    raw = bytearray()
    for y in range(SIZE - 1, -1, -1):  # bottom-up for CreateIcon
        for x in range(SIZE):
            r, g, b = px[y][x]
            raw.extend((b, g, r, 255))
    mask = bytes([0xFF] * (SIZE * SIZE // 8))

    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, "tray.raw"), "wb") as fh:
        fh.write(bytes(raw))
    with open(os.path.join(OUT, "tray.mask"), "wb") as fh:
        fh.write(mask)
    print(f"wrote {OUT}/tray.raw ({len(raw)} bytes) and tray.mask ({len(mask)} bytes)")


if __name__ == "__main__":
    main()
