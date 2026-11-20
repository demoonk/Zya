"""Open a hotkey modal and capture the whole settings window per theme."""
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
    time.sleep(0.5)
    list_view = child(window, "SysListView32", 122)
    left, top, right, bottom = rect(list_view)
    screen_w, screen_h = u.GetSystemMetrics(0), u.GetSystemMetrics(1)
    u.mouse_event(0x8001, int((left + right) // 2 * 65535 / screen_w),
                  int((top + 60) * 65535 / screen_h), 0, 0)
    time.sleep(0.25)
    u.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.1)
    u.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(1.2)
    modal = children(window, "ZyaModalWindow")
    print("modal", bool(modal), rect(modal[0]) if modal else None)
    ok, size = capture(window, "data/check/modal_position_%s.png" % theme)
    print("captured", ok, size)
    if modal:
        u.PostMessageW(modal[0], 0x0111, 160, 0)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "dark")
