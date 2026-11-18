import sys, json, re, glob, math
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
words = {w for w, lp in ru["words"]}
end = {}
freq = {w: math.exp(lp) for w, lp in ru["words"]}
total = sum(freq.values())
for w, f in freq.items():
    if len(w) >= 3:
        end[w[-3:]] = end.get(w[-3:], 0.0) + f

PUNCT = {",": "б", ".": "ю", ";": "ж"}
neg = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([бюжБЮЖ])(?![А-Яа-яёЁ])", text):
        if text[m.end():m.end()+1] in ".,;:":
            continue
        neg.append((m.group(1), m.group(2)))

bad = []
for head, last in neg:
    tok = (head + last).lower()
    if tok in words:
        continue
    tail = tok[-3:] if len(tok) >= 3 else tok
    if tail not in end:
        bad.append((head, last, tok, end.get(tok[-2:], 0)))
print("real words NOT in dict and NOT covered by endings:", len(bad))
for x in sorted(set(bad)):
    print(" ", x)
