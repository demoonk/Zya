import sys, json
sys.stdout.reconfigure(encoding="utf-8")
d = json.load(open("data/lang.json", encoding="utf-8"))
ru, en = d["ru"], d["en"]

def words(lang):
    return {w: lp for w, lp in lang["words"]}

RUW, ENW = words(ru), words(en)

def tri(lang, s):
    letters = lang["letters"]
    def idx(c):
        c = c.lower()
        if c == "ё":
            c = "е"
        return letters.index(c) if c in letters else -1
    a, b, c = (idx(x) for x in s)
    if a < 0 or b < 0 or c < 0:
        return None
    key = (a * len(letters) + b) * len(letters) + c
    for t in lang["trigrams"]:
        if t[0] == a and t[1] == b and t[2] == c:
            return t[3]
    return lang["tri_unseen"]

print("words:")
for w in ["в", "на", "не", "ч", "с", "от", "л", "у", "я", "а", "п", "мп", "дш", "при",
          "духе", "обманула", "коронавирус", "коронавируса", "питания", "произошёл",
          "экраном", "люблю", "еще", "ghb", "le", "vg", "na", "d", "c", "x", "yf", "yt",
          "jn", "gj", "boeing", "dram", "arm", "lego", "nasa", "vpn", "rx", "xt", "iii"]:
    print(f"  {w!r:16} ru={RUW.get(w)} en={ENW.get(w)}")

print("\nru trigrams ending with the punctuation key letters:")
for stem, last in [("коронавируса", "ю"), ("питания", "ю"), ("датчик", "б"), ("люблю", ""),
                   ("факт", "б"), ("огнутых", "б"), ("человек", "б"), ("буфер", "б")]:
    if last:
        s = stem[-2:] + last
        print(f"  {stem}+{last}: tri({s}) = {tri(ru, s)}")

print("\nru trigram distribution: min/max of stored")
vals = [t[3] for t in ru["trigrams"]]
print("  stored count", len(vals), "min", min(vals), "max", max(vals))
import statistics
print("  median", statistics.median(vals))
