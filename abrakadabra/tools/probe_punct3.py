import sys, json, re, glob
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
words = {w for w, lp in ru["words"]}
bi = {(a[0], b[0]): lp for a, b, lp in ru["bigrams"]}
tri = {(a[0], b[0], c[0]): lp for a, b, c, lp in ru["trigrams"]}
PUNCT = {",": "б", ".": "ю", ";": "ж"}

def lp(a, b):
    return bi.get((a, b), ru["bi_unseen"])

def tlp(a, b, c):
    return tri.get((a, b, c), ru["tri_unseen"])

fire = 0
miss = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{3,})([.,;])", text):
        head, p = m.group(1), m.group(2)
        last = PUNCT[p]
        h = head.lower()
        ok = h in words or lp(h[-1], last) < -11.0 or tlp(h[-2], h[-1], last) < -17.0
        if ok:
            fire += 1
        else:
            miss.append((head, p, lp(h[-1], last), tlp(h[-2], h[-1], last)))
print("would fire:", fire, "of 1565")
print("still missed:", len(miss))
for x in miss[:20]:
    print("  ", x)

risk = 0
risklist = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{3,})([бюжБЮЖ])(?![А-Яа-яёЁ])", text):
        head, last = m.group(1), m.group(2)
        nxt = text[m.end():m.end() + 1]
        if nxt in ".,;":
            continue
        h = head.lower()
        tok = (head + last).lower()
        if tok in words:
            continue
        if h in words or lp(h[-1], last) < -11.0 or tlp(h[-2], h[-1], last) < -17.0:
            risk += 1
            risklist.append(tok)
print("false positives on real words:", risk)
print(sorted(set(risklist))[:40])
