import re
import glob
import sys
sys.stdout.reconfigure(encoding="utf-8")

pat = re.compile(r"(?<![A-Za-z0-9])[bB](?![A-Za-z0-9])")
prev_pat = re.compile(r"([A-Za-z0-9]+)$")
stats = {}
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in pat.finditer(text):
        before = text[:m.start()].strip()
        p = prev_pat.search(before)
        prev = p.group(0) if p else ""
        key = "empty" if not prev else ("single" if len(prev) == 1 else "word")
        stats[key] = stats.get(key, 0) + 1
        print(f"{key:6} prev={prev!r} tok={m.group()!r}  ...{before[-30:]!r}")
print(stats)
