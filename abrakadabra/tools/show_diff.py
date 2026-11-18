import sys
sys.stdout.reconfigure(encoding="utf-8")
exp = open("data/article_expected.txt", encoding="utf-8").read().rstrip("\n")
got = open("data/article_typed.txt", encoding="utf-8").read().rstrip("\n")
pos = [116, 154, 331, 417, 447, 627, 628, 632, 670, 671, 672, 673, 697, 776, 777]
for p in sorted(set(pos)):
    a = max(0, p - 25)
    print(f"{p:4d}  ...{exp[a:p]}[{exp[p]}]{exp[p+1:p+15]}...")
