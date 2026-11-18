import json
import glob
import re
import sys
sys.stdout.reconfigure(encoding="utf-8")

raw = json.load(open("data/lang.json", encoding="utf-8"))
ru = raw["ru"]
letters = ru["letters"]
n = len(letters)
idx = {c: i for i, c in enumerate(letters)}
bi = [ru["bi_unseen"]] * (n * n)
for a, b, lp in ru["bigrams"]:
    bi[idx[a[0]] * n + idx[b[0]]] = lp
words = set()
for line in open("../data/ru_50k.txt", encoding="utf-8"):
    line = line.strip().lower()
    if line:
        words.add(line)

rows = []
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for tok in re.findall(r"[А-Яа-яёЁ]{4,}", text):
        t = tok.lower()
        if t[-1] in "бюж" and t not in words:
            rows.append((bi[idx[t[-2]] * n + idx[t[-1]]], tok))
rows.sort()
print(f"{len(rows)} tokens ending in б/ю/ж (len>=4, not in dict)")
seen = {}
for lp, tok in rows:
    seen.setdefault((lp, tok[-2:]), 0)
    seen[(lp, tok[-2:])] += 1
for lp, pair in sorted(seen)[:60]:
    print(f"bigram {pair}: {lp:.2f}")
