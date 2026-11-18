#!/usr/bin/env python3
"""Look at every Latin token that occurs in the collected articles."""
import os
import re
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
ARTICLES = os.path.join(HERE, "..", "data", "articles")

TOKEN = re.compile(r"[A-Za-z][A-Za-z0-9'\-]*")
c = Counter()
for name in sorted(os.listdir(ARTICLES)):
    if not name.endswith(".txt"):
        continue
    for m in TOKEN.finditer(open(os.path.join(ARTICLES, name), encoding="utf-8").read()):
        c[m.group(0).lower().strip("-'")] += 1

with_digit = [(w, n) for w, n in c.items() if any(ch.isdigit() for ch in w)]
print("tokens with digits:", len(with_digit))
for w, n in sorted(with_digit, key=lambda x: -x[1])[:60]:
    print(f"  {w}\t{n}")
print("\npure latin tokens:", len(c))
for w, n in sorted(c.items(), key=lambda x: -x[1])[:60]:
    print(f"  {w}\t{n}")
