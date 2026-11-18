#!/usr/bin/env python3
"""Find collisions: a real Russian word whose mirror is a tech term
(i.e. the tech lexicon could cause a false rewrite of correct Russian text)."""
import json
import os
import re
import sys
from collections import Counter

sys.stdout.reconfigure(encoding="utf-8")

MIRROR = {
    "q": "й", "w": "ц", "e": "у", "r": "к", "t": "е", "y": "н", "u": "г",
    "i": "ш", "o": "щ", "p": "з",
    "a": "ф", "s": "ы", "d": "в", "f": "а", "g": "п", "h": "р", "j": "о",
    "k": "л", "l": "д",
    "z": "я", "x": "ч", "c": "с", "v": "м", "b": "и", "n": "т", "m": "ь",
    ",": "б", ".": "ю", ";": "ж", "'": "э", "`": "ё", "[": "х", "]": "ъ",
}
INV = {v: k for k, v in MIRROR.items()}

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)  # abrakadabra/
DATA = os.path.join(os.path.dirname(ROOT), "data")

ru = set()
en = set()
for path, lang in ((os.path.join(DATA, "ru_50k.txt"), ru), (os.path.join(DATA, "en_50k.txt"), en)):
    for line in open(path, encoding="utf-8"):
        w = line.split()[0]
        lang.add(w)

lex = json.load(open(os.path.join(ROOT, "data", "tech.json"), encoding="utf-8"))
tech = set(w for w, _ in lex["tokens"])

print("tech terms:", len(tech))
collisions = []
for w in ru:
    mir = "".join(INV.get(ch, "") for ch in w)
    if len(mir) >= 2 and all(ch.isalnum() for ch in mir) and mir in tech:
        collisions.append((w, mir))
print("RU words whose mirror is a tech term:", len(collisions))
for w, m in sorted(collisions):
    print(f"  {w} -> {m}")

# tech terms that are also ordinary Russian words (should never trigger)
print("\ntech terms that are also RU words:",
      sorted(w for w in tech if w in ru))
