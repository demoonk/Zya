#!/usr/bin/env python3
"""Pick the most multilingual article from data/articles and build the
article_expected.txt / article_typed.txt test fixtures.

The QWERTY -> RU mirror table is parsed straight from abrakadabra/src/layout.rs
(QWERTY_TO_RU), so the fixture always matches the Rust implementation.
"""
import html
import os
import re
import sys
from collections import Counter

sys.stdout.reconfigure(encoding="utf-8")

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
ARTICLES = os.path.abspath(os.path.join(REPO, "data", "articles"))
LAYOUT_RS = os.path.abspath(os.path.join(HERE, "..", "src", "layout.rs"))
OUT_DIR = os.path.abspath(os.path.join(HERE, "..", "data"))
EXPECTED = os.path.join(OUT_DIR, "article_expected.txt")
TYPED = os.path.join(OUT_DIR, "article_typed.txt")

CYR = re.compile(r"[А-Яа-яёЁ]")
LATIN = re.compile(r"[A-Za-z]")
# word-like token: latin/digit runs (may include - and ') or a cyrillic run
TOKEN = re.compile(r"[A-Za-z0-9]+(?:[-'][A-Za-z0-9]+)*|[А-Яа-яёЁ]+(?:[-'][А-Яа-яёЁ]+)*")

JUNK = re.compile(
    r"(?:реклама|вакансии|опрос|о сайте|рассылка|контакты|новости hardware|"
    r"самое интересное|материалы по теме|источник|постоянный url|теги|авторизация|"
    r"личный кабинет|выйти|было интересно|если вы заметили ошибку|скажите об этом|"
    r"в будущее|в прошлое|18\+|новый год|спецпроект|наши рассылки)",
    re.I,
)


def load_pairs(path: str) -> list[tuple[str, str]]:
    src = open(path, encoding="utf-8").read()
    start = src.index("QWERTY_TO_RU")
    block = src[start : src.index("];", start)]
    pairs = re.findall(r"\(\s*'((?:\\'|[^'])*?)'\s*,\s*'((?:\\'|[^'])*?)'\s*\)", block)
    out = []
    for latin, cyr in pairs:
        la = latin.replace("\\'", "'")[0]
        cy = cyr.replace("\\'", "'")[0]
        out.append((la, cy))
    assert len(out) == 33, f"expected 33 pairs, got {len(out)}"
    return out


PAIRS = load_pairs(LAYOUT_RS)
QWERTY_TO_RU = {}
RU_TO_QWERTY = {}
for latin, cyr in PAIRS:
    QWERTY_TO_RU[latin] = cyr
    QWERTY_TO_RU[cyr] = latin
    RU_TO_QWERTY[cyr] = latin
    if latin.isascii() and latin.isalpha():
        QWERTY_TO_RU[latin.upper()] = cyr.upper()
        QWERTY_TO_RU[cyr.upper()] = latin.upper()
        RU_TO_QWERTY[cyr.upper()] = latin.upper()

CLASSIC = {
    "nvme", "oled", "amoled", "microled", "miniled", "rtx", "gtx", "geforce",
    "ddr", "ddr3", "ddr4", "ddr5", "lpddr4", "lpddr5", "1080p", "1440p", "2160p",
    "720p", "4k", "8k", "ssd", "hdd", "gpu", "cpu", "ram", "hdmi", "usb", "usbc",
    "type-c", "wi-fi", "wifi", "bluetooth", "ips", "lcd", "led", "refresh", "ghz",
    "mhz", "pci", "pcie", "sata", "dpi", "ppi", "hdr", "m2", "nvidia", "intel",
    "amd", "snapdragon", "ryzen", "cuda", "vulkan", "directx", "benchmark",
}

TECH_SET = set()
_tech_json = os.path.join(OUT_DIR, "tech.json")
if os.path.exists(_tech_json):
    try:
        import json
        for tok, _n in json.load(open(_tech_json, encoding="utf-8")).get("tokens", []):
            TECH_SET.add(tok.lower())
    except Exception:
        pass


def to_typed(text: str) -> str:
    out = []
    for ch in text:
        if ch in RU_TO_QWERTY:
            out.append(RU_TO_QWERTY[ch])
        elif CYR.match(ch):
            out.append(ch)  # cyrillic outside the mirror table (should not happen)
        else:
            out.append(ch)
    return "".join(out)


def clean_text(raw: str) -> str:
    if "<" in raw and re.search(r"<[a-zA-Z/][^>]*>", raw):
        try:
            from lxml import etree  # noqa: F401
            tree = etree.HTML(raw)
            raw = " ".join((tree.text or "") for tree in tree.iter() if tree.text)
        except Exception:
            raw = re.sub(r"<[^>]+>", " ", raw)
    raw = html.unescape(raw)
    raw = raw.replace("\u00a0", " ").replace("\u2028", "\n").replace("\t", " ")

    lines = []
    for line in raw.splitlines():
        line = " ".join(line.split()).strip()
        if not line:
            continue
        if len(line) < 90:
            continue  # navigation / titles / bylines / dates
        if JUNK.search(line):
            continue
        if re.match(r"^\d{2}\.\d{2}\.\d{4}", line):
            continue
        if re.search(r"https?://", line):
            continue
        lines.append(line)
    text = " ".join(lines)
    text = re.sub(r"\s+", " ", text).strip()
    return text


def token_scripts(text: str) -> list[str]:
    scripts = []
    for m in TOKEN.finditer(text):
        tok = m.group(0)
        if LATIN.search(tok):
            scripts.append("en")
        elif CYR.search(tok):
            scripts.append("ru")
        else:
            scripts.append("num")
    return scripts


def score_article(text: str) -> dict:
    scripts = token_scripts(text)
    latin = sum(1 for s in scripts if s == "en")
    cyril = sum(1 for s in scripts if s == "ru")
    transitions = sum(
        1 for a, b in zip(scripts, scripts[1:])
        if (a == "ru" and b == "en") or (a == "en" and b == "ru")
    )
    ru_to_en = sum(1 for a, b in zip(scripts, scripts[1:]) if a == "ru" and b == "en")
    en_to_ru = sum(1 for a, b in zip(scripts, scripts[1:]) if a == "en" and b == "ru")
    return {
        "latin": latin,
        "cyrillic": cyril,
        "switches": transitions,
        "ru_to_en": ru_to_en,
        "en_to_ru": en_to_ru,
        "score": latin + transitions,
    }


def sentences(text: str) -> list[str]:
    parts = re.split(r"(?<=[.!?])\s+", text)
    return [p.strip() for p in parts if p.strip()]


def windows(text: str) -> list[str]:
    """Candidate 600-800 char windows aligned to sentence boundaries."""
    sents = sentences(text)
    out = []
    for i in range(len(sents)):
        cur = ""
        for j in range(i, len(sents)):
            cand = (cur + " " + sents[j]).strip() if cur else sents[j]
            if len(cand) > 800:
                break
            if 600 <= len(cand) <= 800:
                out.append(cand)
            cur = cand
    return out


def is_tech(tok: str) -> bool:
    t = tok.lower().strip("-'")
    if not LATIN.search(t) or CYR.search(t):
        return False
    if t in TECH_SET:
        return True
    if any(c.isdigit() for c in t):
        return True
    if len(t) <= 4 and t.isascii():
        return True  # acronyms: nvme, oled, rtx, gpu, ssd, ddr4...
    return len(t) >= 5


def main() -> None:
    print(f"QWERTY_TO_RU pairs from layout.rs: {len(PAIRS)}")
    for k, v in PAIRS:
        print(f"  {k} -> {v}")

    results = []
    for name in sorted(os.listdir(ARTICLES)):
        if not name.endswith((".txt", ".html")):
            continue
        raw = open(os.path.join(ARTICLES, name), encoding="utf-8", errors="replace").read()
        text = clean_text(raw)
        sc = score_article(text)
        sc["name"] = name
        sc["body_chars"] = len(text)
        results.append(sc)

    results.sort(key=lambda r: (-r["score"], -r["latin"]))
    print("\n=== article scores (latin_tokens + script_switches) ===")
    for r in results:
        print(
            f"{r['name']:14s} score={r['score']:4d} latin={r['latin']:4d} "
            f"cyr={r['cyrillic']:4d} ru->en={r['ru_to_en']:4d} en->ru={r['en_to_ru']:4d} "
            f"switches={r['switches']:4d} body={r['body_chars']}"
        )

    best = results[0]
    raw = open(os.path.join(ARTICLES, best["name"]), encoding="utf-8", errors="replace").read()
    body = clean_text(raw)

    cands = []
    for w in windows(body):
        scripts = token_scripts(w)
        latin_tok = sum(1 for s in scripts if s == "en")
        cyr_tok = sum(1 for s in scripts if s == "ru")
        if latin_tok < 3 or cyr_tok < 8:
            continue
        tech = sorted({t for t in TOKEN.findall(w) if is_tech(t)})
        if len(tech) < 3:
            continue
        switches = sum(
            1 for a, b in zip(scripts, scripts[1:])
            if (a == "ru" and b == "en") or (a == "en" and b == "ru")
        )
        classic = sum(1 for t in tech if t.lower() in CLASSIC)
        unmirrored = [c for c in w if CYR.match(c) and c not in RU_TO_QWERTY]
        if unmirrored:
            continue  # the switcher cannot mirror these keys (Б/Ж/Э/Х/Ъ/Ё upper case)
        cands.append((switches + 3 * classic, switches, latin_tok, tech, w, unmirrored))
    cands.sort(key=lambda c: (-c[0], -c[1], len(c[4])))
    print(f"\ncandidate windows in {best['name']}: {len(cands)}")
    for i, (sc, sw, lt, tech, w, unmir) in enumerate(cands[:12]):
        print(f"  #{i} score={sc} switches={sw} latin={lt} len={len(w)} tech={','.join(tech[:10])}")
        if unmir:
            print(f"     unmirrored cyrillic: {''.join(unmir)}")
        print(f"     {w[:90]}...")
    chosen = cands[0][4]

    expected = re.sub(r"\s+", " ", chosen).strip()
    typed = to_typed(expected)

    with open(EXPECTED, "w", encoding="utf-8", newline="\n") as f:
        f.write(expected)
    with open(TYPED, "w", encoding="utf-8", newline="\n") as f:
        f.write(typed)

    exp_scripts = token_scripts(expected)
    lat = sum(1 for s in exp_scripts if s == "en")
    cyr = sum(1 for s in exp_scripts if s == "ru")
    sw = sum(1 for a, b in zip(exp_scripts, exp_scripts[1:]) if a != b and "num" not in (a, b))
    tech_used = sorted({t for t in TOKEN.findall(expected) if is_tech(t)})

    print(f"\n=== chosen article: {best['name']} ===")
    print(f"article score: {best['score']} (latin={best['latin']}, switches={best['switches']})")
    print(f"window length: {len(expected)} chars, latin tokens={lat}, cyrillic tokens={cyr}, switches={sw}")
    print(f"tech terms: {', '.join(tech_used)}")
    print(f"\nexpected ({len(expected)} chars):\n{expected[:200]}")
    print(f"\ntyped ({len(typed)} chars):\n{typed[:200]}")
    print(f"\ncyrillic chars in expected: {sum(1 for c in expected if CYR.match(c))}")
    print(f"latin chars in expected: {sum(1 for c in expected if LATIN.match(c))}")
    print(f"latin chars in typed: {sum(1 for c in typed if LATIN.match(c))}")
    print(f"cyrillic chars in typed: {sum(1 for c in typed if CYR.match(c))}")
    print(f"files: {EXPECTED} ({len(expected)} chars), {TYPED} ({len(typed)} chars)")
    ok = len(typed) == len(expected) and all(
        (QWERTY_TO_RU.get(t, t) == e if CYR.match(e) else t == e)
        for e, t in zip(expected, typed)
    )
    print(f"per-character mirror check: {'OK' if ok else 'FAIL'}")
    print(f"sanity map_word('привет') -> {to_typed('привет')}, map_word('hello') -> {to_typed('hello')}")


if __name__ == "__main__":
    main()
