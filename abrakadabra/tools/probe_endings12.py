import sys, json, re, glob, math
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
letters = ru["letters"]
idx = {c: i for i, c in enumerate(letters)}
uni = {c: lp for c, lp in ru["unigrams"]}
uni_u = ru["uni_unseen"]
bi = {(a[0], b[0]): lp for a, b, lp in ru["bigrams"]}
tri = {(a[0], b[0], c[0]): lp for a, b, c, lp in ru["trigrams"]}
bi_u, tri_u = ru["bi_unseen"], ru["tri_unseen"]
baseline = ru["baseline"]
words = {w: lp for w, lp in ru["words"]}
end3 = set()
for w in words:
    if len(w) >= 3:
        end3.add(w[-3:])

def score(w):
    cs = [idx[c] for c in w if c in idx]
    if len(cs) != len(w):
        return None
    s = sum(uni.get(letters[i], uni_u) for i in cs)
    for a, b in zip(cs, cs[1:]):
        s += bi.get((letters[a], letters[b]), bi_u)
    for a, b, c in zip(cs, cs[1:], cs[2:]):
        s += tri.get((letters[a], letters[b], letters[c]), tri_u)
    return s - baseline[min(len(w), len(baseline) - 1)]

PUNCT = {",": "б", ".": "ю", ";": "ж"}

def variants(stem, last):
    tok = stem + last
    ok_end = (len(tok) >= 3 and tok[-3:] in end3)
    p = bi.get((stem[-1], last), bi_u)
    t = tri.get((stem[-2], stem[-1], last), tri_u) if len(stem) >= 2 else tri_u
    ss = score(stem) or -99
    ts = score(tok) or -99
    return ok_end, p, t, ss, ts

pos, neg = [], []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([.,;])", text):
        pos.append((m.group(1).lower(), PUNCT[m.group(2)]))
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([бюжБЮЖ])(?![А-Яа-яёЁ])", text):
        if text[m.end():m.end()+1] in ".,;:":
            continue
        neg.append((m.group(1).lower(), m.group(2).lower()))

def rule(stem, last, stem_floor, gap):
    tok = stem + last
    if tok in words:
        return False
    ok_end, p, t, ss, ts = variants(stem, last)
    if not ok_end:
        return True
    if p < -11.0 or t < -17.0:
        return True
    return ss >= stem_floor and ss - ts >= gap

for sf in (-3, -6, -8, -10):
    for gap in (0, 3, 6, 10, 999):
        f = sum(1 for h, l in pos if rule(h, l, sf, gap))
        b = [(h + l) for h, l in neg if rule(h, l, sf, gap)]
        print(f"floor {sf} gap {gap}: fire {f}/{len(pos)} broken {len(b)} {sorted(set(b))[:8]}")
