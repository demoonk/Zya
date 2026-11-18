import sys, json, re, glob
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
words = {w for w, lp in ru["words"]}
end3 = {w[-3:] for w in words if len(w) >= 3}

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
pos, neg = [], []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([.,;])", text):
        pos.append((m.group(1).lower(), PUNCT[m.group(2)]))
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([бюжБЮЖ])(?![А-Яа-яёЁ])", text):
        if text[m.end():m.end()+1] in ".,;:":
            continue
        neg.append((m.group(1).lower(), m.group(2).lower()))

for extra in ["чиж", "сиж", "щдб", "клуб", "багаж", "груд", "муж", "веб"]:
    neg.append((extra[:-1], extra[-1]))

def delta(stem, last):
    a, b = score(stem), score(stem + last)
    if a is None or b is None:
        return None
    return a - b

print("POS deltas min/median/max:")
ds = sorted(delta(h, l) for h, l in pos)
print("  n", len(ds), "min", ds[0], "median", ds[len(ds)//2], "max", ds[-1])
print("NEG deltas:")
dn = sorted((delta(h, l), h + l) for h, l in neg)
for v, w in dn[:25]:
    print("  ", w, round(v, 1))
print("  ...")
for v, w in dn[-10:]:
    print("  ", w, round(v, 1))
for th in (0, 2, 4, 6, 8, 10):
    f = sum(1 for h, l in pos if (delta(h, l) or -99) >= th)
    b = [h + l for h, l in neg if (delta(h, l) or -99) >= th]
    print(f"th {th}: fire {f}/{len(pos)} broken {len(b)} {sorted(set(b))[:10]}")
