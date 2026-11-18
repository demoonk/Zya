import re
import glob
import sys
sys.stdout.reconfigure(encoding="utf-8")

pat = re.compile(r"(?<![A-Za-z0-9])[bB](?![A-Za-z0-9])")
hits = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in pat.finditer(text):
        a = max(0, m.start() - 40)
        b = min(len(text), m.end() + 40)
        hits.append((f, m.group(), text[a:b].replace("\n", " ")))
print(len(hits), "hits")
for f, tok, ctx in hits:
    print(f"{tok}  {ctx}")
