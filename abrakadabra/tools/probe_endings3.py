import sys, json, re, glob
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
words = {w for w, lp in ru["words"]}
bi = {(a[0], b[0]): lp for a, b, lp in ru["bigrams"]}
tri = {(a[0], b[0], c[0]): lp for a, b, c, lp in ru["trigrams"]}
bi_unseen = ru["bi_unseen"]; tri_unseen = ru["tri_unseen"]
end3 = set()
end2 = set()
for w in words:
    if len(w) >= 3: end3.add(w[-3:])
    if len(w) >= 2: end2.add(w[-2:])

PUNCT = {",": "б", ".": "ю", ";": "ж"}

def pair(h, l): return bi.get((h[-1], l), bi_unseen)
def triple(h, l):
    if len(h) < 2: return tri_unseen
    return tri.get((h[-2], h[-1], l), tri_unseen)

fire = 0
miss = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([.,;])", text):
        head, p = m.group(1), m.group(2)
        l = PUNCT[p]
        tok = (head + l).lower()
        ends_ok = (len(tok) >= 3 and tok[-3:] in end3) or (len(tok) == 2 and tok in words)
        pair_ok = pair(head.lower(), l) >= -11.0
        triple_ok = triple(head.lower(), l) >= -17.0
        # fire if the ending is impossible by BOTH measures
        if not (ends_ok and pair_ok and triple_ok):
            fire += 1
        else:
            miss.append(tok)
print("union fire:", fire, "miss:", len(miss), sorted(set(miss))[:30])

risk = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([бюжБЮЖ])(?![А-Яа-яёЁ])", text):
        head, last = m.group(1), m.group(2)
        if text[m.end():m.end()+1] in ".,;:":
            continue
        tok = (head + last).lower()
        ends_ok = (len(tok) >= 3 and tok[-3:] in end3) or (len(tok) == 2 and tok in words)
        pair_ok = pair(head.lower(), last.lower()) >= -11.0
        triple_ok = triple(head.lower(), last.lower()) >= -17.0
        if ends_ok and pair_ok and triple_ok:
            continue
        risk.append(tok)
print("real words broken:", len(risk), sorted(set(risk))[:30])
