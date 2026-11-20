import ctypes
import struct
import sys
import time

u = ctypes.windll.user32
ENUM = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)


def settings_window():
    found = []

    def callback(hwnd, _):
        buffer = ctypes.create_unicode_buffer(64)
        u.GetClassNameW(hwnd, buffer, 64)
        if buffer.value == "ZyaSettingsWindow":
            found.append(hwnd)
        return True

    u.EnumWindows(ENUM(callback), 0)
    return found[0] if found else None


def child(parent, cls, cid):
    found = []

    def callback(hwnd, _):
        buffer = ctypes.create_unicode_buffer(64)
        u.GetClassNameW(hwnd, buffer, 64)
        if buffer.value == cls and u.GetWindowLongW(hwnd, -12) == cid:
            found.append(hwnd)
        return True

    u.EnumChildWindows(parent, ENUM(callback), 0)
    return found[0] if found else None


def children(parent, cls):
    found = []

    def callback(hwnd, _):
        buffer = ctypes.create_unicode_buffer(64)
        u.GetClassNameW(hwnd, buffer, 64)
        if buffer.value == cls:
            found.append(hwnd)
        return True

    u.EnumChildWindows(parent, ENUM(callback), 0)
    return found


def rect(hwnd):
    buffer = ctypes.create_string_buffer(16)
    u.GetWindowRect(hwnd, buffer)
    return struct.unpack("<4i", buffer.raw)


def main():
    window = settings_window()
    out = ["settings window: " + str(bool(window))]
    if not window:
        sys.stdout.buffer.write(("\n".join(out)).encode("utf-8"))
        return

    hotkeys = child(window, "SysListView32", 122)
    out.append("hotkey list: " + str(hotkeys))
    out.append("list proc: " + str(u.GetWindowLongPtrW(hotkeys, -1)))
    out.append("list rect: " + str(rect(hotkeys)))

    # A real click on the row: move the cursor, press, release.
    left, top, right, bottom = rect(hotkeys)
    screen_x = u.GetSystemMetrics(0)
    screen_y = u.GetSystemMetrics(1)
    u.mouse_event(0x8001, int((left + 200) * 65535 / screen_x), int((top + 40) * 65535 / screen_y), 0, 0)
    time.sleep(0.4)
    u.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.2)
    u.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(1.5)

    modals = children(window, "ZyaModalWindow")
    out.append("modal count: " + str(len(modals)))
    if modals:
        out.append("modal rect: " + str(rect(modals[0])))
        from PIL import ImageGrab

        a, b, c, d = rect(modals[0])
        ImageGrab.grab(bbox=(a - 8, b - 8, c + 8, d + 8)).save("data/modal_click.png")

    sys.stdout.buffer.write(("\n".join(out)).encode("utf-8"))


if __name__ == "__main__":
    main()
