import sys
sys.stdout.reconfigure(encoding="utf-8")
typed = open("data/article_typed.txt", encoding="utf-8").read()
exp = open("data/article_expected.txt", encoding="utf-8").read()
print("typed len", len(typed), "exp len", len(exp))
for p in [116, 154, 331, 417, 447, 697, 627, 670]:
    a = max(0, p - 30)
    print(f"{p:4d} exp: {exp[a:p+10]!r}")
    print(f"{p:4d} typ: {typed[a:p+10]!r}")
    print()
