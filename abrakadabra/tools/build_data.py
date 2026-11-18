#!/usr/bin/env python3
"""
Build the language statistics resource used by abrakadabra.

Input : data/ru_50k.txt, data/en_50k.txt   (hermitdave/FrequencyWords: "word count")
Output: abrakadabra/data/lang.json  (embedded into the binary with include_bytes!)

The model is character level and deliberately small:

    score(word) = sum(unigram logprob) + sum(bigram logprob)
                + sum(trigram logprob) + word_bonus

Bigram/trigram counts are weighted by real corpus frequency, so "impossible"
letter clusters (Russian "щк", English "qz") get a much lower score than
plausible ones without any hand-written rule table.
"""
import json
import math
import os
import sys
from collections import Counter

sys.stdout.reconfigure(encoding="utf-8")

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)  # abrakadabra/
# The frequency corpora live one level above the repo (Abrakadra/data).
DATA = os.path.join(os.path.dirname(ROOT), "data")
OUT = os.path.join(ROOT, "data", "lang.json")

RU_LETTERS = "абвгдеёжзийклмнопрстуфхцчшщъыьэюя"
EN_LETTERS = "abcdefghijklmnopqrstuvwxyz"

TOP_WORDS = 50000
MIN_NGRAM = 1
ALPHA = 0.5  # Laplace smoothing
TECH_WEIGHT = 30  # how strongly technical terms count in the model


def load(path, alphabet):
    words = {}
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            parts = line.split()
            if len(parts) != 2:
                continue
            w, c = parts[0], int(parts[1])
            w = w.lower().replace("ё", "е")
            if not w or not all(ch in alphabet for ch in w) or len(w) > 24:
                continue
            words[w] = words.get(w, 0) + c
    return words


def merge_tech(words, tech_path, weight, alphabet):
    """Technical terms are rare in a general corpus but must be recognised as
    real words of their language, so they are folded into the model with a
    weight that makes their letter clusters (nvme, ddr, rtx) plausible."""
    if not os.path.exists(tech_path):
        return words
    for line in open(tech_path, encoding="utf-8"):
        parts = line.split()
        if len(parts) != 2:
            continue
        w, c = parts[0].lower(), int(parts[1])
        if not w or not all(ch in alphabet for ch in w) or len(w) > 24:
            continue
        words[w] = words.get(w, 0) + c * weight
    return words


def build(alphabet, words, top_words):
    n = len(alphabet)
    uni, bi, tri = Counter(), Counter(), Counter()
    for w, c in words.items():
        for ch in w:
            uni[ch] += c
        for a, b in zip(w, w[1:]):
            bi[a + b] += c
        for a, b, cc in zip(w, w[1:], w[2:]):
            tri[a + b + cc] += c

    total_u = sum(uni.values())
    total_b = sum(bi.values())
    total_t = sum(tri.values())

    unigrams = [[a, math.log((uni[a] + ALPHA) / (total_u + ALPHA * n))]
                for a in alphabet if uni[a] > 0]
    bigrams = [[a, b, math.log((bi[a + b] + ALPHA) / (total_b + ALPHA * n * n))]
               for a in alphabet for b in alphabet if bi[a + b] >= MIN_NGRAM]
    trigrams = [[a, b, c, math.log((tri[a + b + c] + ALPHA) / (total_t + ALPHA * n ** 3))]
                for a in alphabet for b in alphabet for c in alphabet
                if tri[a + b + c] >= MIN_NGRAM]
    trigrams.sort(key=lambda x: -x[3])
    trigrams = trigrams[:25000]

    top = sorted(words.items(), key=lambda kv: -kv[1])[:top_words]
    total_w = sum(words.values())

    # Per-length average n-gram score, used to normalise so that Russian and
    # English words of the same length are comparable around zero.
    sums = {}
    counts = {}
    for w, c in words.items():
        L = len(w)
        s = sum(math.log((uni[ch] + ALPHA) / (total_u + ALPHA * n)) for ch in w)
        s += sum(math.log((bi[a + b] + ALPHA) / (total_b + ALPHA * n * n))
                 for a, b in zip(w, w[1:]))
        s += sum(math.log((tri[a + b + cc] + ALPHA) / (total_t + ALPHA * n ** 3))
                 for a, b, cc in zip(w, w[1:], w[2:]))
        sums[L] = sums.get(L, 0.0) + s * c
        counts[L] = counts.get(L, 0) + c
    baseline = [sums.get(L, 0.0) / counts[L] if counts.get(L) else 0.0 for L in range(0, 25)]

    return {
        "letters": alphabet,
        "uni_unseen": math.log(ALPHA / (total_u + ALPHA * n)),
        "bi_unseen": math.log(ALPHA / (total_b + ALPHA * n * n)),
        "tri_unseen": math.log(ALPHA / (total_t + ALPHA * n ** 3)),
        "baseline": baseline,
        "unigrams": unigrams,
        "bigrams": bigrams,
        "trigrams": trigrams,
        "words": [[w, math.log(c / total_w)] for w, c in top],
    }


def main():
    ru = load(os.path.join(DATA, "ru_50k.txt"), RU_LETTERS)
    en = load(os.path.join(DATA, "en_50k.txt"), EN_LETTERS)
    en = merge_tech(en, os.path.join(DATA, "tech_terms.txt"), TECH_WEIGHT, EN_LETTERS)
    ru = merge_tech(ru, os.path.join(DATA, "tech_ru_terms.txt"), TECH_WEIGHT, RU_LETTERS)
    payload = {"ru": build(RU_LETTERS, ru, TOP_WORDS), "en": build(EN_LETTERS, en, TOP_WORDS)}
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as fh:
        json.dump(payload, fh, ensure_ascii=False, separators=(",", ":"))
    print(f"wrote {OUT} ({os.path.getsize(OUT)} bytes)")
    for lang in ("ru", "en"):
        print(lang, "bigrams:", len(payload[lang]["bigrams"]),
              "trigrams:", len(payload[lang]["trigrams"]),
              "words:", len(payload[lang]["words"]))
    ru_set = {a + b for a, b, _ in payload["ru"]["bigrams"]}
    missing = [a + b for a in RU_LETTERS for b in RU_LETTERS if a + b not in ru_set]
    print(" russian bigrams absent from corpus:", "".join(missing[:120]))
    en_set = {a + b for a, b, _ in payload["en"]["bigrams"]}
    missing_en = [a + b for a in EN_LETTERS for b in EN_LETTERS if a + b not in en_set]
    print(" english bigrams absent from corpus:", "".join(missing_en[:120]))


if __name__ == "__main__":
    main()
