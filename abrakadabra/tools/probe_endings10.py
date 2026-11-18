import sys, json, re, glob, math
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
words = {w: lp for w, lp in ru["words"]}
bi = {(a[0], b[0]): lp for a, b, lp in ru["bigrams"]}
tri = {(a[0], b[0], c[0]): lp for a, b, c, lp in ru["trigrams"]}
bi_u, tri_u = ru["bi_unseen"], ru["tri_unseen"]

end_words = {}
for w in words:
    if len(w) >= 3:
        end_words.setdefault(w[-3:], []).append(w)

PUNCT = {",": "б", ".": "ю", ";": "ж"}
miss = {}
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([.,;])", text):
        head, p = m.group(1).lower(), m.group(2)
        l = PUNCT[p]
        tok = head + l
        if tok in words:
            continue
        ok_end = tok[-3:] in end_words
        pv = bi.get((head[-1], l), bi_u)
        tv = tri.get((head[-2], head[-1], l), tri_u) if len(head) >= 2 else tri_u
        if ok_end and pv >= -11.0 and tv >= -17.0:
            miss.setdefault(tok, [0, len(end_words[tok[-3:]]), pv, tv])
            miss[tok][0] += 1
print("misses:", sum(v[0] for v in miss.values()))
for tok, (c, n, pv, tv) in sorted(miss.items(), key=lambda x: -x[1][0])[:40]:
    print(f"  {tok} x{c}  endings_with_tail={n} pair={pv:.1f} triple={tv:.1f} e.g. {end_words[tok[-3:]][:4]}")
