"""Capture the ZЯ settings window into a PNG without depending on what is
stacked on top of it on the desktop."""
import ctypes
import io
import sys
import time

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
sys.path.insert(0, "tools")
from click_probe import settings_window, rect  # noqa: E402

from PIL import Image  # noqa: E402

u = ctypes.windll.user32
g = ctypes.windll.gdi32
u.SetProcessDPIAware()

WINDOW_CLASS = "ZyaSettingsWindow"
GDI = {
    "CreateCompatibleDC": g.CreateCompatibleDC,
    "CreateCompatibleBitmap": g.CreateCompatibleBitmap,
    "SelectObject": g.SelectObject,
    "DeleteObject": g.DeleteObject,
    "DeleteDC": g.DeleteDC,
    "GetBitmapBits": g.GetBitmapBits,
}


def find_settings():
    buffer = ctypes.create_unicode_buffer(100)
    found = []

    def callback(hwnd, _):
        u.GetClassNameW(hwnd, buffer, 100)
        if buffer.value == WINDOW_CLASS:
            found.append(hwnd)
        return True

    CB = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
    u.EnumWindows(CB(callback), 0)
    return found


class DIBSECTION(ctypes.Structure):
    _fields_ = [
        ("bmType", ctypes.c_long),
        ("bmWidth", ctypes.c_long),
        ("bmHeight", ctypes.c_long),
        ("bmWidthBytes", ctypes.c_long),
        ("bmPlanes", ctypes.c_byte),
        ("bmBitsPer", ctypes.c_byte),
        ("bmBits", ctypes.c_void_p),
        ("dsPh", ctypes.c_long),
        ("dsV", ctypes.c_long),
        ("dsDx", ctypes.c_long),
        ("dsDy", ctypes.c_long),
        ("dsXs", ctypes.c_long),
        ("dsYs", ctypes.c_long),
        ("ciColors", ctypes.c_long),
        ("objName", ctypes.c_byte * 32),
    ]


def capture(hwnd, path):
    left, top, right, bottom = rect(hwnd)
    width, height = right - left, bottom - top
    dc = u.GetWindowDC(hwnd)
    mem = GDI["CreateCompatibleDC"](dc)
    bitmap = GDI["CreateCompatibleBitmap"](dc, width, height)
    GDI["SelectObject"](mem, bitmap)
    ok = u.PrintWindow(hwnd, mem, 2)
    info = DIBSECTION()
    ctypes.windll.gdi32.GetObjectW(bitmap, ctypes.sizeof(info), ctypes.byref(info))
    stride = info.bmWidthBytes
    height = abs(info.bmHeight)
    bpp_bytes = stride // info.bmWidth
    data = ctypes.create_string_buffer(stride * height)
    GDI["GetBitmapBits"](bitmap, stride * height, ctypes.byref(data))
    # GetBitmapBits already returns the DIB top-down for a positive-height bitmap.
    rows = [data[y * stride : y * stride + info.bmWidth * bpp_bytes] for y in range(height)]
    pixels = b"".join(rows)
    if bpp_bytes == 4:
        image = Image.frombytes("RGBA", (info.bmWidth, height), pixels).convert("RGB")
    else:
        image = Image.frombytes("RGB", (info.bmWidth, height), pixels)
    image.save(path)
    GDI["DeleteObject"](bitmap)
    GDI["DeleteDC"](mem)
    u.ReleaseDC(hwnd, dc)
    return ok, image.size


def main():
    windows = find_settings()
    if not windows:
        print("no settings window")
        return
    hwnd = windows[0]
    for tab, name in ((100, "settings"), (101, "ignore"), (102, "log")):
        u.PostMessageW(hwnd, 0x0111, tab, 0)
        time.sleep(1.0)
        ok, size = capture(hwnd, "data/tab_%s.png" % name)
        print(name, "printwindow", ok, size)


if __name__ == "__main__":
    main()
