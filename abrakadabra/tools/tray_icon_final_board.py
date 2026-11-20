"""Final presentation: the four tray icons over a transparency checkerboard."""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUT = Path("data/tray_icons")
cell, pad = 256, 30
label_font = ImageFont.truetype("C:\\Windows\\Fonts\\segoeui.ttf", 20)

groups = [
    ("night", [("ru_dark", "RU · night"), ("en_dark", "EN · night")]),
    ("day", [("ru_light", "RU · day"), ("en_light", "EN · day")]),
    ("off", [("ru_off_dark", "RU · auto off"), ("en_off_dark", "EN · auto off")]),
]

w = 2 * (cell + pad) + pad
h = cell + 2 * pad + 34
sheet = Image.new("RGB", (w, 3 * (h + pad) + pad), (255, 255, 255))
offset_y = pad
for _, items in groups:
    board = Image.new("RGBA", (w, h), (255, 255, 255, 255))
    draw = ImageDraw.Draw(board)
    for y in range(0, h, 12):
        for x in range(0, w, 12):
            if ((x // 12) + (y // 12)) % 2:
                draw.rectangle((x, y, x + 11, y + 11), fill=(214, 214, 214, 255))
    for i, (name, caption) in enumerate(items):
        icon = Image.open(OUT / f"{name}.png").resize((cell, cell), Image.LANCZOS)
        board.alpha_composite(icon, (pad + i * (cell + pad), pad))
        ImageDraw.Draw(board).text((pad + i * (cell + pad), pad + cell + 6), caption, fill=(60, 60, 60, 255), font=label_font)
    sheet.paste(board.convert("RGB"), (0, offset_y))
    offset_y += h + pad

sheet.save(OUT / "final_board.png")
print(OUT / "final_board.png", sheet.size)
