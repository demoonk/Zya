"""Capture only the hotkey modal window so its own painting can be inspected."""
import sys
import time

sys.path.insert(0, "tools")
from click_probe import child, children, rect  # noqa: E402
from window_capture import find_settings, capture, u  # noqa: E402


def main(theme):
    u.SetProcessDPIAware()
    window = find_settings()[0]
    u.SetWindowPos(window, None, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010)
    u.BringWindowToTop(window)
    time.sleep(0.4)
    list_view = child(window, "SysListView32", 122)
    left, top, right, bottom = rect(list_view)
    screen_w, screen_h = u.GetSystemMetrics(0), u.GetSystemMetrics(1)
    u.mouse_event(0x8001, int((left + right) // 2 * 65535 / screen_w),
                  int((top + 60) * 65535 / screen_h), 0, 0)
    time.sleep(0.2)
    u.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.08)
    u.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(1.0)
    modal = children(window, "ZyaModalWindow")
    if not modal:
        print("no modal")
        return
    ok, size = capture(modal[0], "data/modal_only_%s.png" % theme)
    print("modal", ok, size, rect(modal[0]))
    for child_window in children(modal[0], ""):
        buffer = ctypes_text(child_window)
        print("modal child", rect(child_window), buffer)
        break
    u.PostMessageW(modal[0], 0x0111, 160, 0)


def ctypes_text(hwnd):
    import ctypes
    buffer = ctypes.create_unicode_buffer(200)
    u.GetWindowTextW(hwnd, buffer, 200)
    return buffer.value


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "light")
