"""Reference implementation of the tray glyph rasterizer (mirrors tray.rs)."""
import io
import sys

from PIL import Image

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

ICON = 32
SS = 4
W, H, GAP, S = 13, 24, 3, 4


def rects_letter(ch, ox, oy):
    """Union of axis-aligned rectangles / polygons for one letter."""
    if ch == "R":
        return [
            [(ox, oy), (ox + S, oy), (ox + S, oy + H), (ox, oy + H)],
            [(ox, oy), (ox + W, oy), (ox + W, oy + S), (ox, oy + S)],
            [(ox + W - S, oy), (ox + W, oy), (ox + W, oy + H // 2), (ox + W - S, oy + H // 2)],
            [(ox + S, oy + H // 2 - S), (ox + W, oy + H // 2 - S), (ox + W, oy + H // 2), (ox + S, oy + H // 2)],
            [(ox + S, oy + H // 2), (ox + W - S, oy + H // 2), (ox + W, oy + H), (ox + W - S, oy + H)],
        ]
    if ch == "U":
        return [
            [(ox, oy), (ox + S, oy), (ox + S, oy + H), (ox, oy + H)],
            [(ox + W - S, oy), (ox + W, oy), (ox + W, oy + H), (ox + W - S, oy + H)],
            [(ox, oy + H - S), (ox + W, oy + H - S), (ox + W, oy + H), (ox, oy + H)],
        ]
    if ch == "E":
        return [
            [(ox, oy), (ox + S, oy), (ox + S, oy + H), (ox, oy + H)],
            [(ox, oy), (ox + W, oy), (ox + W, oy + S), (ox, oy + S)],
            [(ox, oy + H // 2 - S // 2), (ox + W - 1, oy + H // 2 - S // 2),
             (ox + W - 1, oy + H // 2 + S // 2), (ox, oy + H // 2 + S // 2)],
            [(ox, oy + H - S), (ox + W, oy + H - S), (ox + W, oy + H), (ox, oy + H)],
        ]
    if ch == "N":
        return [
            [(ox, oy), (ox + S, oy), (ox + S, oy + H), (ox, oy + H)],
            [(ox + W - S, oy), (ox + W, oy), (ox + W, oy + H), (ox + W - S, oy + H)],
            [(ox, oy), (ox + S + 1, oy), (ox + W, oy + H - S - 2),
             (ox + W, oy + H), (ox + W - S - 1, oy + H), (ox, oy + S + 2)],
        ]
    return []


def inside(poly, px, py):
    sign = 0
    n = len(poly)
    for i in range(n):
        x1, y1 = poly[i]
        x2, y2 = poly[(i + 1) % n]
        cross = (x2 - x1) * (py - y1) - (y2 - y1) * (px - x1)
        s = 1 if cross > 1e-9 else (-1 if cross < -1e-9 else 0)
        if s:
            if sign and sign != s:
                return False
            sign = s
    return sign != 0


def glyph(label, slash=False):
    total = 2 * W + GAP
    x0 = (ICON - total) / 2.0
    y0 = (ICON - H) / 2.0
    polys = []
    for i, ch in enumerate(label):
        polys.extend(rects_letter(ch, x0 + i * (W + GAP), y0))
    if slash:
        polys.append([(3.0, 29.0), (5.5, 29.0), (29.0, 5.5), (29.0, 3.0), (26.5, 3.0), (3.0, 26.5)])
    out = []
    for y in range(ICON):
        row = []
        for x in range(ICON):
            hit = 0
            for sy in range(SS):
                for sx in range(SS):
                    px = x + (sx + 0.5) / SS
                    py = y + (sy + 0.5) / SS
                    if any(inside(p, px, py) for p in polys):
                        hit += 1
            row.append(hit * 255 // (SS * SS))
        out.append(row)
    return out


def show(g):
    for y in range(ICON):
        print("".join("#" if g[y][x] >= 128 else ("+" if g[y][x] > 0 else ".") for x in range(ICON)))


def to_image(g, ink, bg):
    im = Image.new("RGB", (ICON, ICON), bg)
    px = im.load()
    for y in range(ICON):
        for x in range(ICON):
            c = g[y][x] / 255.0
            px[x, y] = tuple(int(bg[i] + (ink[i] - bg[i]) * c) for i in range(3))
    return im


cells = []
for label in ("RU", "EN"):
    g = glyph(label)
    show(g)
    for scale in (32, 24, 18, 16):
        cells.append(to_image(g, (255, 255, 255), (18, 21, 25)).resize((scale, scale), Image.LANCZOS))
        cells.append(to_image(g, (0, 0, 0), (243, 245, 247)).resize((scale, scale), Image.LANCZOS))

gw = max(c.width for c in cells)
rows = [cells[i:i + 2] for i in range(0, len(cells), 2)]
sheet = Image.new("RGB", (gw + 20, sum(r[0].height + 6 for r in rows) + 10), (70, 70, 70))
y = 5
for r in rows:
    x = 5
    for c in r:
        sheet.paste(c, (x, y))
        x += c.width + 12
    y += r[0].height + 6
sheet = sheet.resize((sheet.width * 5, sheet.height * 5), Image.NEAREST)
sheet.save("data/check/tray_glyph_sheet.png")
print("saved")
