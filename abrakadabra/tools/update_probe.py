"""Click the update button in the settings window and print the status line."""
import ctypes
import sys
import time

sys.path.insert(0, "tools")
from click_probe import settings_window, children  # noqa: E402
from window_capture import u  # noqa: E402


def main():
    u.SetProcessDPIAware()
    window = settings_window()
    if not window:
        print("no settings window")
        return
    u.SetWindowPos(window, None, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010)
    u.BringWindowToTop(window)
    time.sleep(0.4)
    u.PostMessageW(window, 0x0111, 112, 0)
    for _ in range(12):
        time.sleep(1.0)
        labels = []
        for handle in children(window, "Static"):
            text = ctypes.create_unicode_buffer(500)
            u.GetWindowTextW(handle, text, 500)
            if text.value:
                labels.append(text.value)
        line = labels[-1] if labels else "(empty)"
        print("status:", line)
        if "Обновить" in line or "последняя" in line.lower() or "не удалось" in line.lower():
            break


if __name__ == "__main__":
    main()
