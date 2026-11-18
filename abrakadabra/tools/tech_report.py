#!/usr/bin/env python3
"""Statistics of technical vocabulary from the 3dnews/ixbt corpus.

Writes abrakadabra/data/tech_stats.txt and prints a short summary:
  * top Latin technical terms by frequency in the 20 collected articles;
  * how many of them the engine can recover from the "wrong layout" form;
  * the mirror (Cyrillic) form that a user actually types for each term.
"""
import html
import json
import os
import re
import sys
from collections import Counter

sys.stdout.reconfigure(encoding="utf-8")

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)  # abrakadabra/
DATA = os.path.join(os.path.dirname(ROOT), "data")
ARTICLES = os.path.join(DATA, "articles")
LEX = os.path.join(ROOT, "data", "tech.json")
OUT = os.path.join(ROOT, "data", "tech_stats.txt")

# RU(1049) -> EN(1049) physical key pairs, same table as src/layout.rs
RU_TO_EN = {
    "а": "f", "б": ",", "в": "d", "г": "g", "д": "l", "е": "t", "ё": "`",
    "ж": ";", "з": "p", "и": "b", "й": "q", "к": "r", "л": "u", "м": "s",
    "н": "i", "о": "o", "п": "z", "р": "h", "с": "c", "т": "n", "у": "e",
    "ф": "v", "х": "x", "ц": "w", "ч": "y", "ш": "u", "ь": "m", "ы": "w",
    "э": "'", "ю": ".", "я": "j",
}
# the two keys that are shared: 'w'->ц, 'x'->ч ... handled above
RU_TO_EN["ш"] = "u"  # note: 'ш' and 'л' share the key in the mirror table used by the engine
# exact mirror used by the engine (QWERTY_TO_RU):
MIRROR = {
    "q": "й", "w": "ц", "e": "у", "r": "к", "t": "е", "y": "н", "u": "л",
    "i": "ш", "o": "щ", "p": "з", "[": "(", "]": ")",
    "a": "ф", "s": "ы", "d": "в", "f": "а", "g": "п", "h": "р", "j": "о",
    "k": "л", "l": "д", ";": "ж", "'": "э",
    "z": "я", "x": "ч", "c": "с", "v": "м", "b": "и", "n": "т", "m": "ь",
    ",": "б", ".": "ю", "/": ".",
}
INV = {v: k for k, v in MIRROR.items()}
INV["ш"] = "i"


def article_text(path):
    raw = html.unescape(open(path, encoding="utf-8").read())
    raw = re.sub(r"<script[\s\S]*?</script>", " ", raw)
    raw = re.sub(r"<style[\s\S]*?</style>", " ", raw)
    raw = re.sub(r"https?://\S+", " ", raw)
    return raw


def main():
    files = [f for f in sorted(os.listdir(ARTICLES)) if f.endswith(".txt")]
    texts = {}
    for f in files:
        texts[f] = article_text(os.path.join(ARTICLES, f))
    full = " ".join(texts.values())

    tokens = re.findall(r"[A-Za-z][A-Za-z0-9'\-]*", full.lower())
    cnt = Counter(tokens)

    lex = json.load(open(LEX, encoding="utf-8"))
    terms = {w: c for w, c in lex["tokens"]}

    lines = []
    lines.append("Технический словарь: статистика по 20 статьям (3dnews.ru, ixbt.com)")
    lines.append("=" * 70)
    lines.append(f"статей: {len(files)}, знаков: {len(full)}, токенов: {len(tokens)}")
    lines.append(f"терминов в tech.json: {len(terms)}")
    lines.append("")
    lines.append("Топ-60 технических токенов в corpus:")
    for w, c in cnt.most_common(60):
        tag = " [lexicon]" if w in terms else ""
        lines.append(f"  {w:<22} {c:>5}{tag}")
    lines.append("")

    lines.append("Термины, которые движок восстанавливает из «не той» раскладки:")
    lines.append("  (в скобках — что реально набирается на RU-раскладке)")
    shown = 0
    for w, c in sorted(terms.items(), key=lambda kv: -kv[1])[:80]:
        wrong = "".join(MIRROR.get(ch, ch) for ch in w)
        if wrong == w:
            continue
        lines.append(f"  {w:<14}  набор на RU-раскладке: {wrong:<14} (в corpus {c} раз)")
        shown += 1
    lines.append(f"  ... всего {shown} терминов с нетривиальным зеркалом")
    lines.append("")

    # how often a Cyrillic mistype of a tech term actually occurs in the corpus
    cyr = re.findall(r"[а-яё0-9]{2,}", full.lower())
    cyr_cnt = Counter(cyr)
    hits = []
    for w in terms:
        wrong = "".join(MIRROR.get(ch, ch) for ch in w)
        if wrong != w and cyr_cnt.get(wrong, 0) > 0:
            hits.append((wrong, w, cyr_cnt[wrong]))
    lines.append("Реальные кириллические написания тех-терминов в статьях:")
    for wrong, right, c in sorted(hits, key=lambda t: -t[2])[:25]:
        lines.append(f"  {wrong:<14} (имеется в виду {right}) x{c}")
    if not hits:
        lines.append("  (в отобранных статьях опечаток такого типа нет — словарь "
                     "собран по корректному написанию)")
    lines.append("")
    lines.append("Сводка: {lex} терминов, {tot} вхождений в corpus".format(
        lex=len(terms), tot=sum(cnt[w] for w in terms if w in cnt)))
    text = "\n".join(lines)
    open(OUT, "w", encoding="utf-8").write(text)
    print(text)


if __name__ == "__main__":
    main()
