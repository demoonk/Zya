import ctypes
import sys
from pathlib import Path

import uiautomation as auto
from PIL import Image

sys.stdout.reconfigure(encoding="utf-8")
P = Path(__file__).resolve().parent.parent / "data"

user32 = ctypes.windll.user32
gdi = ctypes.windll.gdi32

user32.GetDC.restype = ctypes.c_void_p
user32.ReleaseDC.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
gdi.CreateCompatibleDC.restype = ctypes.c_void_p
gdi.CreateCompatibleDC.argtypes = [ctypes.c_void_p]
gdi.CreateCompatibleBitmap.restype = ctypes.c_void_p
gdi.CreateCompatibleBitmap.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
gdi.SelectObject.restype = ctypes.c_void_p
gdi.SelectObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
gdi.BitBlt.argtypes = [ctypes.c_void_p] + [ctypes.c_int] * 4 + [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
gdi.DeleteObject.argtypes = [ctypes.c_void_p]
gdi.DeleteDC.argtypes = [ctypes.c_void_p]


class BIH(ctypes.Structure):
    _fields_ = [("biSize", ctypes.c_uint32), ("biWidth", ctypes.c_int32),
                ("biHeight", ctypes.c_int32), ("biPlanes", ctypes.c_uint16),
                ("biBitCount", ctypes.c_uint16), ("biCompression", ctypes.c_uint32),
                ("biSizeImage", ctypes.c_uint32), ("biXPelsPerMeter", ctypes.c_int32),
                ("biYPelsPerMeter", ctypes.c_int32), ("biClrUsed", ctypes.c_uint32),
                ("biClrImportant", ctypes.c_uint32)]


gdi.GetDIBits.restype = ctypes.c_int
gdi.GetDIBits.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_int,
                          ctypes.c_char_p, ctypes.c_void_p, ctypes.c_uint]


def shot():
    w = user32.GetSystemMetrics(0)
    h = user32.GetSystemMetrics(1)
    hdc = user32.GetDC(None)
    mem = gdi.CreateCompatibleDC(hdc)
    bmp = gdi.CreateCompatibleBitmap(hdc, w, h)
    gdi.SelectObject(mem, bmp)
    gdi.BitBlt(mem, 0, 0, w, h, hdc, 0, 0, 0x00CC00CC)
    bih = BIH()
    bih.biSize = 40
    bih.biWidth = w
    bih.biHeight = -h
    bih.biPlanes = 1
    bih.biBitCount = 32
    buf = ctypes.create_string_buffer(w * h * 4)
    gdi.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bih), 0)
    gdi.DeleteObject(bmp)
    gdi.DeleteDC(mem)
    user32.ReleaseDC(None, hdc)
    return Image.frombytes("RGBA", (w, h), bytes(buf)).convert("RGB")


img = shot()
print("screen:", img.size)

target = None
for c in auto.GetRootControl().GetChildren():
    if c.Name == "Taskbar":
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
    print("tray button not found")
    sys.exit(0)

r = target.BoundingRectangle
print("icon rect:", r)
crop = img.crop((r.left - 4, r.top - 4, r.right + 4, r.bottom + 4))
crop.resize((crop.width * 6, crop.height * 6), Image.NEAREST).save(P / "tray_icon_now.png")
band = img.crop((max(0, r.left - 300), r.top, min(img.width, r.right + 200), r.bottom))
band.resize((band.width * 2, band.height * 2), Image.LANCZOS).save(P / "tray_band.png")
print("saved tray_icon_now.png", crop.size, "tray_band.png", band.size)
