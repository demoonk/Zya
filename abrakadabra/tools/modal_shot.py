"""Open the hotkey assignment modal and capture the whole settings window."""
import sys
import time

sys.path.insert(0, "tools")
from click_probe import child, children, rect  # noqa: E402
from window_capture import find_settings, capture, u  # noqa: E402


def main(theme):
    u.SetProcessDPIAware()
    windows = find_settings()
    if not windows:
        print("no settings window")
        return
    window = windows[0]
    u.SetWindowPos(window, None, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010)
    u.BringWindowToTop(window)
    time.sleep(0.4)
    list_view = child(window, "SysListView32", 122)
    left, top, right, bottom = rect(list_view)
    screen_w, screen_h = u.GetSystemMetrics(0), u.GetSystemMetrics(1)
    x = (left + right) // 2
    y = top + 60
    u.mouse_event(0x8001, int(x * 65535 / screen_w), int(y * 65535 / screen_h), 0, 0)
    time.sleep(0.2)
    u.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.08)
    u.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(1.0)
    modal = children(window, "ZyaModalWindow")
    print("modal open:", bool(modal))
    if modal:
        ok, size = capture(window, "data/modal_%s.png" % theme)
        print("capture", ok, size)
        u.PostMessageW(modal[0], 0x0111, 160, 0)
        time.sleep(0.4)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "light")
