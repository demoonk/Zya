import sys, json, re, glob
sys.stdout.reconfigure(encoding="utf-8")

d = json.load(open("data/lang.json", encoding="utf-8"))
ru = {w for w, lp in d["ru"]["words"]}
en = {w for w, lp in d["en"]["words"]}
MIRROR = {"q":"й","w":"ц","e":"у","r":"к","t":"е","y":"н","u":"г","i":"ш","o":"щ","p":"з",
          "a":"ф","s":"ы","d":"в","f":"а","g":"п","h":"р","j":"о","k":"л","l":"д","z":"я",
          "x":"ч","c":"с","v":"м","b":"и","n":"т","m":"ь"}
MIRROR.update({k.upper(): v.upper() for k, v in MIRROR.items()})

standalone = {}
for f in glob.glob("../data/articles/*.txt"):
    text = open(f, encoding="utf-8", errors="replace").read()
    for m in re.finditer(r"(?<![А-Яа-яёЁA-Za-z0-9])([A-Za-z])(?![А-Яа-яёЁA-Za-z0-9])", text):
        ch = m.group(1)
        ctx = text[max(0, m.start()-25):m.end()+25]
        standalone[ch] = standalone.get(ch, 0) + 1
        if standalone[ch] <= 2:
            print(ch, "->", MIRROR.get(ch.lower(), "?"), "|", ctx.replace("\n", " "))
print("total standalone latin letters:", sum(standalone.values()))
print(sorted(standalone.items(), key=lambda x: -x[1]))
