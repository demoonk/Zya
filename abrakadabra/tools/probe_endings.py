import sys, json, re, glob
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru_words = [w for w, lp in d["ru"]["words"]]
end2 = set()
end3 = set()
for w in ru_words:
    if len(w) >= 2:
        end2.add(w[-2:])
    if len(w) >= 3:
        end3.add(w[-3:])
print("ru word-final bigrams:", len(end2), "trigrams:", len(end3))
print("bigrams ending in б/ю/ж:", sorted(x for x in end2 if x[-1] in "бюж"))
print("trigrams ending in б:", sorted(x for x in end3 if x[-1] == "б")[:40])
print("trigrams ending in ж:", sorted(x for x in end3 if x[-1] == "ж")[:40])

PUNCT = {",": "б", ".": "ю", ";": "ж"}
fire = 0
miss = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([.,;])", text):
        head, p = m.group(1), m.group(2)
        tok = (head + PUNCT[p]).lower()
        if len(tok) >= 3 and tok[-3:] in end3:
            miss.append(tok)
        else:
            fire += 1
print("would fire:", fire, "miss:", len(miss), sorted(set(miss))[:25])

risk = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{2,})([бюжБЮЖ])(?![А-Яа-яёЁ])", text):
        head, last = m.group(1), m.group(2)
        nxt = text[m.end():m.end()+1]
        if nxt in ".,;:":
            continue
        tok = (head + last).lower()
        if len(tok) >= 3 and tok[-3:] in end3:
            continue
        risk.append((head, last))
print("real words that would break:", len(risk), sorted(set(risk))[:30])
