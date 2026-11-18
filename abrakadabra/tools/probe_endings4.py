import sys, json, re, glob, math
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
freq = {w: math.exp(lp) for w, lp in ru["words"]}
total = sum(freq.values())
end = {}
for w, f in freq.items():
    if len(w) >= 3:
        end[w[-3:]] = end.get(w[-3:], 0.0) + f
end_lp = {k: math.log(v / total) for k, v in end.items()}

PUNCT = {",": "б", ".": "ю", ";": "ж"}

def collect():
    pos = []   # real "stem + punctuation" cases in the corpus (should fire)
    neg = []   # genuine Russian words ending in б/ю/ж (must not fire)
    for f in glob.glob("../data/articles/*.txt"):
        text = open(f, encoding="utf-8", errors="replace").read()
        for m in re.finditer(r"([А-Яа-яёЁ]{2,})([.,;])", text):
            pos.append((m.group(1).lower(), PUNCT[m.group(2)]))
        for m in re.finditer(r"([А-Яа-яёЁ]{2,})([бюжБЮЖ])(?![А-Яа-яёЁ])", text):
            if text[m.end():m.end()+1] in ".,;:":
                continue
            neg.append((m.group(1).lower(), m.group(2).lower()))
    return pos, neg

pos, neg = collect()
for th in (-8, -10, -12, -14, -16, -18, -20):
    fire = sum(1 for h, l in pos if end_lp.get(h[-3:] + "", None) is None or True)
    fire = 0
    for h, l in pos:
        tok = h + l
        lp = end_lp.get(tok[-3:], None)
        if lp is None or lp < th:
            fire += 1
    broken = 0
    for h, l in neg:
        tok = h + l
        lp = end_lp.get(tok[-3:], None)
        if lp is None or lp < th:
            broken += 1
    print(f"threshold {th}: fire {fire}/{len(pos)}  broken {broken}")

print("neg total:", len(neg))
print("sample end_lp:", sorted((v, k) for k, v in end_lp.items() if k[-1] in "бюж")[-25:])
