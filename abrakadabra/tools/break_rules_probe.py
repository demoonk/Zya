import ctypes
import io
import json
import struct
import sys
import time

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
sys.path.insert(0, "tools")
from click_probe import settings_window, child, children, rect  # noqa: E402

u = ctypes.windll.user32
u.SetProcessDPIAware()
CFG = "data/appdata/zя/config.json"


def key(vk, flags):
    return u.SendInput(1, struct.pack("<I4xHHIIQ12x", 1, vk, 0, flags, 0, 0), 40)


def press(vk):
    key(vk, 1)
    time.sleep(0.04)
    key(vk, 3)
    time.sleep(0.04)


def status():
    label = child(settings_window(), "Static", 150)
    text = ctypes.create_unicode_buffer(300)
    u.GetWindowTextW(label, text, 300)
    return text.value


def modal():
    return bool(children(settings_window(), "ZyaModalWindow"))


def hotkeys():
    cfg = json.load(open(CFG, encoding="utf-8"))
    return {k: v for k, v in cfg.items() if k.startswith("hotkey")}


def click(window, list_view, row):
    a, b, c, d = rect(list_view)
    sx, sy = u.GetSystemMetrics(0), u.GetSystemMetrics(1)
    x = (a + c) // 2
    y = b + 40 + 26 * row
    u.mouse_event(0x8001, int(x * 65535 / sx), int(y * 65535 / sy), 0, 0)
    time.sleep(0.2)
    u.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.08)
    u.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.8)


window = settings_window()
list_view = child(window, "SysListView32", 122)

cfg = json.load(open(CFG, encoding="utf-8"))
cfg["hotkey_undo"] = "alt"
cfg["hotkey_toggle"] = "ctrl+alt+p"
cfg["hotkey_last"] = "break"
cfg["hotkey_selection"] = "break"
cfg["hotkey_upper"] = "ctrl+break"
cfg["hotkey_invert"] = "shift+break"
json.dump(cfg, open(CFG, "w", encoding="utf-8"), ensure_ascii=False, indent=2)
if list_view:
    child(window, "SysListView32", 122)

tests = [
    ("row 0 undo + Break", 0, lambda: press(0x13)),
    ("row 1 toggle + Ctrl+C", 1, lambda: (key(0x11, 1), press(0x43), key(0x11, 3))),
    ("row 1 toggle + k", 1, lambda: press(0x4B)),
    ("row 2 last + Break", 2, lambda: press(0x13)),
    ("row 4 upper + Ctrl+Break", 4, lambda: (key(0x11, 1), press(0x13), key(0x11, 3))),
    ("row 5 invert + Shift+Break", 5, lambda: (key(0x10, 1), press(0x13), key(0x10, 3))),
]

for name, row, action in tests:
    click(window, list_view, row)
    if not modal():
        print(f"{name}: modal not open, status={status()}")
        continue
    action()
    time.sleep(0.6)
    print(f"{name}: status={status()} modal={modal()} hotkeys={hotkeys()}")
    if modal():
        press(0x1B)
        time.sleep(0.4)
