import re
import glob
import sys
sys.stdout.reconfigure(encoding="utf-8")

pat = re.compile(r"\d[бюжБЮЖ][^\s]|(?<![\s\d])[бюжБЮЖ]\d")
hits = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in pat.finditer(text):
        a = max(0, m.start() - 25)
        b = min(len(text), m.end() + 25)
        hits.append((f, m.group(), text[a:b].replace("\n", " ")))
print(len(hits), "digit-бюж-digit hits")
for f, tok, ctx in hits[:40]:
    print(f"{tok}  {ctx}")
