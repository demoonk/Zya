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
end_lp = {k: math.log(v / total) for k, v in end.items()}
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

def pair(h, l): return bi.get((h[-1], l), bi_u)
def triple(h, l):
    if len(h) < 2: return tri_u
    return tri.get((h[-2], h[-1], l), tri_u)
def endlp(h, l):
    tok = h + l
    if len(tok) < 3: return 100.0 if tok in words else -100.0
    return end_lp.get(tok[-3:], -100.0)

def report(name, fire_fn):
    f = sum(1 for h, l in pos if fire_fn(h, l))
    b = [(h + l) for h, l in neg if fire_fn(h, l)]
    print(f"{name}: fire {f}/{len(pos)}  broken {len(b)} {sorted(set(b))[:20]}")

report("dict-stem", lambda h, l: h in words)
report("pair<-11", lambda h, l: pair(h, l) < -11)
report("triple<-17", lambda h, l: triple(h, l) < -17)
report("old OR (stem+pair+triple)", lambda h, l: h in words or pair(h, l) < -11 or triple(h, l) < -17)
report("end<-12", lambda h, l: endlp(h, l) < -12)
report("old OR end<-12", lambda h, l: h in words or pair(h, l) < -11 or triple(h, l) < -17 or endlp(h, l) < -12)
report("old OR end<-15", lambda h, l: h in words or pair(h, l) < -11 or triple(h, l) < -17 or endlp(h, l) < -15)
