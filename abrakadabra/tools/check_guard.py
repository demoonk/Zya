import json
import os
import sys

sys.stdout.reconfigure(encoding="utf-8")

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)  # abrakadabra/
DATA = os.path.join(os.path.dirname(ROOT), "data")  # Abrakadra/data

PAIRS = [
    ("q", "й"), ("w", "ц"), ("e", "у"), ("r", "к"), ("t", "е"), ("y", "н"),
    ("u", "г"), ("i", "ш"), ("o", "щ"), ("p", "з"), ("a", "ф"), ("s", "ы"),
    ("d", "в"), ("f", "а"), ("g", "п"), ("h", "р"), ("j", "о"), ("k", "л"),
    ("l", "д"), ("z", "я"), ("x", "ч"), ("c", "с"), ("v", "м"), ("b", "и"),
    ("n", "т"), ("m", "ь"), (",", "б"), (".", "ю"), (";", "ж"), ("'", "э"),
    ("`", "ё"), ("[", "х"), ("]", "ъ"),
]
M = {}
for a, b in PAIRS:
    M[a] = b
    M[b] = a


def mirror(t):
    return "".join(M.get(ch, ch) for ch in t)


ru = set(l.split()[0] for l in open(os.path.join(DATA, "ru_50k.txt"), encoding="utf-8") if len(l.split()) == 2)
en = set(l.split()[0] for l in open(os.path.join(DATA, "en_50k.txt"), encoding="utf-8") if len(l.split()) == 2)
terms = [t[0] for t in json.load(open(os.path.join(ROOT, "data", "tech.json"), encoding="utf-8"))["tokens"]]

blocked = []
for t in terms:
    typed = mirror(t)
    letters = "".join(ch for ch in typed if ch.isalpha())
    # the engine only blocks runs of two or more letters
    if len(letters) >= 2 and letters in ru:
        blocked.append((t, typed, letters))
print("blocked by 'current letters are a real word' guard:", len(blocked))
for t, typed, l in blocked:
    print("  ", t, "<-", typed, "(letters:", l, ")")
print("terms whose mirror is also a tech term:", sum(1 for t in terms if mirror(t) in set(terms)))
print("en-word overlap:", sum(1 for t in terms if "".join(c for c in t if c.isalpha()) in en))
