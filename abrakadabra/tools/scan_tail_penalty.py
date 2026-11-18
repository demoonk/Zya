import json
import sys
sys.stdout.reconfigure(encoding="utf-8")

raw = json.load(open("data/lang.json", encoding="utf-8"))
ru = raw["ru"]
letters = ru["letters"]
idx = {c: i for i, c in enumerate(letters)}
uni = [ru["uni_unseen"]] * len(letters)
for ch, lp in ru["unigrams"]:
    uni[idx[ch[0]]] = lp
bi = [ru["bi_unseen"]] * (len(letters) * len(letters))
for a, b, lp in ru["bigrams"]:
    bi[idx[a[0]] * len(letters) + idx[b[0]]] = lp
n = len(letters)
tri = {}
for a, b, c, lp in ru["trigrams"]:
    tri[(idx[a[0]] * n + idx[b[0]]) * n + idx[c[0]]] = lp
tri_unseen = ru["tri_unseen"]

print("bi_unseen", ru["bi_unseen"], "tri_unseen", ru["tri_unseen"])
print("uni б", uni[idx['б']], "uni ю", uni[idx['ю']], "uni ж", uni[idx['ж']])


def increment(word):
    cs = [idx[c] for c in word]
    s = uni[cs[-1]]
    if len(cs) >= 2:
        s += bi[cs[-2] * n + cs[-1]]
    if len(cs) >= 3:
        s += tri.get((cs[-3] * n + cs[-2]) * n + cs[-1], tri_unseen)
    return s


for w in ["компанию", "батарею", "клуб", "чиж", "багаж", "люблю", "огнутыхб", "огнутых",
          "фактб", "факт", "сказкаж", "сказка", "проектж", "проект", "версию", "машину",
          "сиж", "щдувж", "щдув", "сегодняю", "тулю", "груб", "чижб"]:
    print(f"{w}: tail increment {increment(w):.2f}")
