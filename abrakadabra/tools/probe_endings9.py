import sys, json, re, glob, math
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
words = {w for w, lp in ru["words"]}
bi = {(a[0], b[0]): lp for a, b, lp in ru["bigrams"]}
tri = {(a[0], b[0], c[0]): lp for a, b, c, lp in ru["trigrams"]}
bi_u, tri_u = ru["bi_unseen"], ru["tri_unseen"]
freq = {w: math.exp(lp) for w, lp in ru["words"]}
total = sum(freq.values())
end = {}
for w, f in freq.items():
    if len(w) >= 3:
        end[w[-3:]] = end.get(w[-3:], 0.0) + f

PUNCT = {",": "б", ".": "ю", ";": "ж"}

def fires(stem, last):
    tok = stem + last
    if tok in words:
        return False
    if len(tok) < 3:
        return False
    ok_end = tok[-3:] in end
    p = bi.get((stem[-1], last), bi_u)
    t = tri.get((stem[-2], stem[-1], last), tri_u) if len(stem) >= 2 else tri_u
    return (stem in words) or (not ok_end) or p < -11.0 or t < -17.0

pos, neg = [], []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([.,;])", text):
        pos.append((m.group(1).lower(), PUNCT[m.group(2)]))
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([бюжБЮЖ])(?![А-Яа-яёЁ])", text):
        if text[m.end():m.end()+1] in ".,;:":
            continue
        neg.append((m.group(1).lower(), m.group(2).lower()))

ok = sum(1 for h, l in pos if fires(h, l))
bad = [h + l for h, l in neg if fires(h, l)]
print("fire", ok, "/", len(pos), "  false positives:", len(bad), sorted(set(bad))[:30])
