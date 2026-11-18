import sys, json, re, glob
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
letters = ru["letters"]
n = len(letters)
idx = {c: i for i, c in enumerate(letters)}
bi = {}
for a, b, lp in ru["bigrams"]:
    bi[(a[0], b[0])] = lp
tri = {}
for a, b, c, lp in ru["trigrams"]:
    tri[(a[0], b[0], c[0])] = lp
words = {w for w, lp in ru["words"]}

PUNCT = {",": "б", ".": "ю", ";": "ж"}
rows = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{3,})([.,;])", text):
        head, p = m.group(1), m.group(2)
        last = PUNCT[p]
        pair = bi.get((head[-1], last), ru["bi_unseen"])
        triple = tri.get((head[-2], head[-1], last), ru["tri_unseen"]) if len(head) >= 2 else None
        rows.append((head, p, pair, triple, head in words, head + last in words))

print("occurrences:", len(rows))
in_dict = sum(1 for r in rows if r[4])
token_dict = sum(1 for r in rows if r[5])
print("head in ru dict:", in_dict, " token in ru dict:", token_dict)
buckets = {}
for head, p, pair, triple, hd, td in rows:
    key = (pair >= -11.0, (triple is None or triple >= -17.0))
    buckets[key] = buckets.get(key, 0) + 1
print("pair_ok, triple_ok -> count:", buckets)
worst = sorted({(r[0], r[1], r[2], r[3]) for r in rows}, key=lambda x: -x[2])[:25]
for w in worst:
    print("REJECT-ish:", w)
