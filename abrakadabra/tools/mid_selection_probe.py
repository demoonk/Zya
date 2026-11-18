"""Compatibility check of the *replacement* mechanism only.

The layout logic is tested in-process (cargo test / simtest / simall). This
script checks one thing the engine cannot check on its own: that Break on a
fragment selected in the middle of the text replaces exactly that fragment and
leaves the text around it untouched.
"""
import sys
import time
import ctypes
import threading

sys.stdout.reconfigure(encoding="utf-8")
ctypes.windll.shcore.SetProcessDpiAwareness(2)
import tkinter as tk

u = ctypes.windll.user32
root = tk.Tk()
root.title("zя middle selection")
root.geometry("760x200+620+420")
txt = tk.Text(root, font=("Consolas", 17))
txt.pack(fill="both", expand=True)
root.attributes("-topmost", True)


def pump(ms):
    end = time.time() + ms / 1000
    while time.time() < end:
        root.update()
        time.sleep(0.005)


def select_fragment(needle):
    start = txt.search(needle, "1.0", stopindex="end")
    txt.tag_remove("sel", "1.0", "end")
    txt.tag_add("sel", start, f"{start}+{len(needle)}c")
    txt.mark_set("insert", f"{start}+{len(needle)}c")


def run():
    pump(1200)
    txt.focus_force()
    pump(300)
    txt.insert("1.0", "начало ghbdtn конец")
    pump(300)
    before = txt.get("1.0", "end-1c")
    select_fragment("ghbdtn")
    pump(500)
    u.keybd_event(0x13, 0, 0, 0)
    u.keybd_event(0x13, 0, 2, 0)
    pump(2200)
    after = txt.get("1.0", "end-1c")
    print("before:", repr(before), flush=True)
    print("after :", repr(after), flush=True)
    expected = "начало привет конец"
    print("OK" if after == expected else f"FAIL: expected {expected!r}", flush=True)
    root.quit()


threading.Thread(target=run, daemon=True).start()
root.mainloop()
