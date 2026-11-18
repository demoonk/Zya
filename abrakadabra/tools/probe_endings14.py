import sys, json
sys.stdout.reconfigure(encoding="utf-8")
d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
words = {w: lp for w, lp in ru["words"]}
bi = {(a[0], b[0]): lp for a, b, lp in ru["bigrams"]}
tri = {(a[0], b[0], c[0]): lp for a, b, c, lp in ru["trigrams"]}
end2 = {w[-2:] for w in words if len(w) >= 2}
end3 = {w[-3:] for w in words if len(w) >= 3}
for t in ["чиж", "сиж", "щдб", "веб", "факт", "зумом", "муж", "клуб", "багаж", "груд", "коронавирус", "актуально", "параметр"]:
    print(f"{t}: in_dict={t in words}")
for stem, last in [("чи","ж"),("си","ж"),("щд","б"),("ве","б"),("факт","б"),("зумом","б"),("ру","б"),("голуб","")]:
    tok = stem + last
    print(f"{tok}: stem_word={stem in words} end2={tok[-2:] in end2} end3={tok[-3:] if len(tok)>=3 else '' in end3 and tok[-3:] in end3} pair={bi.get((stem[-1], last))} triple={tri.get((stem[-2], stem[-1], last))}")
print("ж word-endings (2):", sorted({w[-2:] for w in words if w.endswith("ж")})[:20])
print("б word-endings (2):", sorted({w[-2:] for w in words if w.endswith("б")})[:40])
