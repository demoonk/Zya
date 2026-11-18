import glob, json, re

pairs = {
    'q':'й','w':'ц','e':'у','r':'к','t':'е','y':'н','u':'г','i':'ш','o':'щ','p':'з',
    'a':'ф','s':'ы','d':'в','f':'а','g':'п','h':'р','j':'о','k':'л','l':'д','z':'я',
    'x':'ч','c':'с','v':'м','n':'т','m':'ь',
}
inv = {v:k for k,v in pairs.items()}
tech = {t.lower() for t,_ in json.load(open('data/tech.json', encoding='utf-8'))['tokens']}

def mirror(word):
    out = []
    for ch in word:
        lo = ch.lower()
        if lo in inv:
            m = inv[lo]
            out.append(m.upper() if ch.isupper() else m)
        else:
            out.append(ch)
    return ''.join(out)

hits = {}
for f in glob.glob('../data/articles/*.txt'):
    for tok in re.findall(r"[A-Za-zА-Яа-я0-9]+(?:-[A-Za-zА-Яа-я0-9]+)+", open(f, encoding='utf-8').read()):
        for part in tok.split('-'):
            if len(part) >= 2 and part.isalpha() and all('а' <= c.lower() <= 'я' or c.lower() == 'ё' for c in part):
                mir = mirror(part)
                if mir in tech:
                    hits.setdefault(part.lower(), 0)
                    hits[part.lower()] += 1
print(len(hits), sorted(hits.items(), key=lambda x: -x[1])[:40])
