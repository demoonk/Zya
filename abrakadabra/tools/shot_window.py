"""Screenshot the ZЯ settings window by its window class name.

Usage: python tools/shot_window.py out.png
"""
import ctypes
import sys
import time

from PIL import Image

u = ctypes.windll.user32
g = ctypes.windll.gdi32

u.SetProcessDPIAware()

WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)


class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


def windows_of_class(name):
    found = []

    def callback(hwnd, _):
        if not u.IsWindowVisible(hwnd):
            return True
        buffer = ctypes.create_unicode_buffer(128)
        u.GetClassNameW(hwnd, buffer, 128)
        if buffer.value == name:
            rect = RECT()
            u.GetWindowRect(hwnd, ctypes.byref(rect))
            found.append((hwnd, rect.left, rect.top,
                          rect.right - rect.left, rect.bottom - rect.top))
        return True

    u.EnumWindows(WNDENUMPROC(callback), 0)
    return found


def capture(x, y, w, h, out):
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
    raw = Image.frombytes("RGBA", (w, h), buf)
    red, green, blue, _ = raw.split()
    Image.merge("RGB", (blue, green, red)).save(out)


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "window.png"
    tab = int(sys.argv[2]) if len(sys.argv) > 2 else None
    for hwnd, x, y, w, h in windows_of_class("ZyaSettingsWindow"):
        if tab is not None:
            u.SendMessageW(hwnd, 0x0111, tab, 0)
            time.sleep(0.4)
        print(f"settings window at {x},{y} size {w}x{h}")
        capture(x, y, w, h, out)
        return
    print("no settings window is open")


if __name__ == "__main__":
    main()
