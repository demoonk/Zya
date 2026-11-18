import sys, json, re, glob
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
words = {w for w, lp in ru["words"]}

PUNCT = set(".,;")
bad = {}
good = 0
heads_not_dict = 0
heads_dict = 0
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"([А-Яа-яёЁ]{3,})([бюжБЮЖ])(?![А-Яа-яёЁ])", text):
        head, last = m.group(1), m.group(2)
        nxt = text[m.end():m.end() + 1]
        token = head + last
        if nxt in PUNCT:
            if head.lower() in words:
                heads_dict += 1
            else:
                heads_not_dict += 1
            continue
        if token.lower() in words:
            continue
        bad[token] = bad.get(token, 0) + 1
        good += 1
print("punctuation-followed: head in dict", heads_dict, "head not in dict", heads_not_dict)
print("real-looking б/ю/ж words that the rule would break:", good)
for t, c in sorted(bad.items(), key=lambda x: -x[1])[:30]:
    print("  would break:", t, c)
