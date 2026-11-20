"""Capture the zя tray icon from the live taskbar and save it at 1x and 10x."""
import sys
from pathlib import Path

import uiautomation as auto
from PIL import Image, ImageGrab

sys.stdout.reconfigure(encoding="utf-8")
OUT = Path("data/check")
OUT.mkdir(exist_ok=True)

target = None
for c in auto.GetRootControl().GetChildren():
    if c.Name != "Taskbar":
        continue
    stack = [(c, 0)]
    while stack:
        ctrl, d = stack.pop()
        if d > 8:
            continue
        for ch in ctrl.GetChildren():
            if "zя" in (ch.Name or ""):
                target = ch
            stack.append((ch, d + 1))
    break

if target is None:
    print("tray button not found — is the app running?")
    sys.exit(0)

r = target.BoundingRectangle
print("icon rect:", r)
im = ImageGrab.grab(bbox=(r.left, r.top, r.right, r.bottom)).convert("RGB")
im.save(OUT / "tray_icon_native.png")
im.resize((im.width * 10, im.height * 10), Image.NEAREST).save(OUT / "tray_icon_native_10x.png")
print("saved", OUT / "tray_icon_native.png", im.size)
