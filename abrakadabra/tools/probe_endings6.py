import sys, json, math
sys.stdout.reconfigure(encoding="utf-8")
d = json.load(open("data/lang.json", encoding="utf-8"))
ru = d["ru"]
freq = {w: math.exp(lp) for w, lp in ru["words"]}
total = sum(freq.values())
end = {}
for w, f in freq.items():
    if len(w) >= 3:
        end[w[-3:]] = end.get(w[-3:], 0.0) + f
for th in (-11, -12, -13):
    keep = [k for k, v in end.items() if math.log(v / total) >= th]
    print(th, "endings kept:", len(keep))
    print("  with б/ю/ж:", sorted(k for k in keep if k[-1] in "бюж"))
