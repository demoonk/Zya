"""Screenshot a screen region and save it as PNG (uses the bundled Pillow).

Usage: python tools/shot.py out.png [x y w h]
No arguments -> whole screen.
"""
import sys
import ctypes

from PIL import Image

u = ctypes.windll.user32
g = ctypes.windll.gdi32


def capture(x, y, w, h):
    hdc = u.GetDC(0)
    mem = g.CreateCompatibleDC(hdc)
    bmp = g.CreateCompatibleBitmap(hdc, w, h)
    g.SelectObject(mem, bmp)
    g.BitBlt(mem, 0, 0, w, h, hdc, x, y, 13369376)

    class BI(ctypes.Structure):
        _fields_ = [
            ("biSize", ctypes.c_uint),
            ("biWidth", ctypes.c_int),
            ("biHeight", ctypes.c_int),
            ("biPlanes", ctypes.c_ushort),
            ("biBitCount", ctypes.c_ushort),
            ("biCompression", ctypes.c_uint),
            ("biSizeImage", ctypes.c_uint),
            ("biXPelsPerMeter", ctypes.c_int),
            ("biYPelsPerMeter", ctypes.c_int),
            ("biClrUsed", ctypes.c_uint),
            ("biClrImportant", ctypes.c_uint),
        ]

    bi = BI()
    bi.biSize = 40
    bi.biWidth = w
    bi.biHeight = -h
    bi.biPlanes = 1
    bi.biBitCount = 32
    buf = ctypes.create_string_buffer(w * h * 4)
    g.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bi), 0)
    g.DeleteObject(bmp)
    g.DeleteDC(mem)
    u.ReleaseDC(0, hdc)
    return Image.frombytes("RGBA", (w, h), buf).convert("RGB")


def main():
    sw, sh = u.GetSystemMetrics(0), u.GetSystemMetrics(1)
    out = sys.argv[1] if len(sys.argv) > 1 else "shot.png"
    if len(sys.argv) >= 5:
        x, y, w, h = (int(v) for v in sys.argv[2:6])
    else:
        x, y, w, h = 0, 0, sw, sh
    capture(x, y, w, h).save(out)
    print(f"{out} {w}x{h} from {x},{y} (screen {sw}x{sh})")


if __name__ == "__main__":
    main()
