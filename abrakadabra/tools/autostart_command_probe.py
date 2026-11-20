import ctypes
import io
import json
import subprocess
import sys
import time

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
sys.path.insert(0, "tools")
from click_probe import settings_window, child, rect  # noqa: E402

u = ctypes.windll.user32
u.SetProcessDPIAware()
WM_COMMAND = 0x0111

window = settings_window()
print("window rect", rect(window))


def status():
    label = child(window, "Static", 150)
    buf = ctypes.create_unicode_buffer(400)
    u.GetWindowTextW(label, buf, 400)
    return buf.value


def run_value():
    p = subprocess.run(
        ["reg", "query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "zя.exe"],
        capture_output=True,
    )
    return p.stdout.decode("utf-8", "replace").strip()


def task_value():
    p = subprocess.run(["schtasks", "/query", "/tn", "ZyaAutoStart"], capture_output=True)
    out = p.stdout.decode("utf-8", "replace") + p.stderr.decode("utf-8", "replace")
    return "задание ZyaAutoStart существует" if "ZyaAutoStart" in out else "задания нет"


def config():
    cfg = json.load(open("C:/Users/demOOn/AppData/Roaming/zя/config.json", encoding="utf-8"))
    return cfg.get("autostart"), cfg.get("admin_autostart")


print("начало: status={} config={} run={} task={}".format(status(), config(), run_value(), task_value()))

for role in (120, 121, 120, 121):
    u.PostMessageW(window, WM_COMMAND, role, 0)
    time.sleep(1.0)
    print(
        "role {}: status={} config={} run={} task={}".format(
            role, status(), config(), run_value(), task_value()
        )
    )
