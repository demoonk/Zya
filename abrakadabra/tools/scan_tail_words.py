import re
import glob
import sys
sys.stdout.reconfigure(encoding="utf-8")

# Tokens in the corpus that end in б/ю/ж and are NOT in the ru dictionary:
# these would be false positives for the punctuation rule.
words = set()
for line in open("../data/ru_50k.txt", encoding="utf-8"):
    line = line.strip().lower()
    if line:
        words.add(line)

bad = {}
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for tok in re.findall(r"[А-Яа-яёЁ]{3,}", text):
        if tok[-1] in "бюж" and tok.lower() not in words:
            bad[tok.lower()] = bad.get(tok.lower(), 0) + 1
print(len(bad), "distinct non-dictionary words ending in б/ю/ж")
for w, c in sorted(bad.items(), key=lambda x: -x[1])[:60]:
    print(f"{w}  x{c}")
