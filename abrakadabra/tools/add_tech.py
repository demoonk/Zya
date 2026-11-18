import json, sys
sys.stdout.reconfigure(encoding="utf-8")
path = "data/tech.json"
d = json.load(open(path, encoding="utf-8"))
toks = d["tokens"]
have = {t.lower() for t, c in toks}
add = ["it", "li", "sim", "fp", "soc", "ai", "io", "hd", "sd", "tv", "ui", "id",
       "bt", "rpm", "fps", "msrp", "usb", "pci", "sod", "ms", "gb", "tb", "mb",
       "cd", "dv", "hdmi", "dp", "ips", "oled", "led", "rgb", "tft", "amoled",
       "nfc", "wi", "pro", "max", "plus", "lite", "neo", "ex", "os", "api", "pc"]
added = []
for t in add:
    if t not in have:
        toks.append([t, 3])
        added.append(t)
json.dump(d, open(path, "w", encoding="utf-8"), ensure_ascii=False)
print("added:", added, "total:", len(toks))
