import ctypes
import io
import subprocess
import sys

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

installed = r"C:\Users\demOOn\AppData\Roaming\zя\zя.exe"
value = f'"{installed}" run headless'

advapi = ctypes.windll.advapi32
key = ctypes.c_void_p(0)
if advapi.RegOpenKeyExW(
    ctypes.c_void_p(0x80000001),
    r"Software\Microsoft\Windows\CurrentVersion\Run",
    0,
    2,
    ctypes.byref(key),
) == 0:
    data = value.encode("utf-16-le")
    print("set result", advapi.RegSetValueExW(key, "zя.exe", 0, 1, data, len(data)))
    advapi.RegCloseKey(key)

proc = subprocess.run(
    ["reg", "query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "zя.exe"],
    capture_output=True,
)
print(proc.stdout.decode("utf-8", "replace"))
