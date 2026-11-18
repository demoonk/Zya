import sys, json, re, glob
sys.stdout.reconfigure(encoding="utf-8")

cnt = {}
ctx = {}
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"(?<![А-Яа-яёЁ])([A-Za-z])(?![A-Za-z0-9])", text):
        ch = m.group(1)
        left = text[max(0, m.start() - 12):m.start()]
        right = text[m.end():m.end() + 12]
        cnt[ch] = cnt.get(ch, 0) + 1
        ctx.setdefault(ch, []).append((left, right))
print("single letters total:", sum(cnt.values()))
for ch, c in sorted(cnt.items(), key=lambda x: -x[1]):
    print(ch, c, ctx[ch][:2])
