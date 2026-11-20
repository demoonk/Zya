"""Show what GDI actually paints for each tray icon variant.

`DrawIconEx` was run against a magenta DIB, so:
  * a pixel still exactly (255,0,255)  -> Windows left it untouched  = transparent
  * anything else                      -> the colour GDI composited there
Each cell is composited over the taskbar colour the theme really uses.
"""
from PIL import Image

variants = [
    ("RU / auto / night", "tray_ru_auto_dark", (25, 21, 18)),
    ("RU / auto / day", "tray_ru_auto_light", (247, 245, 243)),
    ("EN / auto / night", "tray_en_auto_dark", (25, 21, 18)),
    ("EN / auto / day", "tray_en_auto_light", (247, 245, 243)),
    ("RU / off / night", "tray_ru_off_dark", (25, 21, 18)),
    ("RU / off / day", "tray_ru_off_light", (247, 245, 243)),
    ("EN / off / night", "tray_en_off_dark", (25, 21, 18)),
    ("EN / off / day", "tray_en_off_light", (247, 245, 243)),
]
sizes = [16, 24, 32]
zoom = 8
pad = 18

cells = []
for label, name, bg in variants:
    row = []
    for sz in sizes:
        im = Image.open(f"data/check/{name}_draw{sz}.bmp").convert("RGB")
        px = im.load()
        out = Image.new("RGB", (sz, sz), bg)
        ox = out.load()
        for y in range(sz):
            for x in range(sz):
                if px[x, y] != (255, 0, 255):
                    ox[x, y] = px[x, y]
        row.append((f"{sz} px", out.resize((sz * zoom, sz * zoom), Image.NEAREST)))
    cells.append((label, row))

cell = 32 * zoom
W = len(sizes) * (cell + pad) + pad
H = len(cells) * (cell + pad) + pad
board = Image.new("RGB", (W, H), (18, 18, 20))
for r, (label, row) in enumerate(cells):
    for c, (tag, im) in enumerate(row):
        x0 = pad + c * (cell + pad)
        y0 = pad + r * (cell + pad)
        board.paste(im, (x0, y0, x0 + im.width, y0 + im.height))
board.save("data/check/winapi_render_board.png")
print(board.size)
