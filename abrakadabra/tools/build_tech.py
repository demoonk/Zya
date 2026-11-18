#!/usr/bin/env python3
"""
Build the technical vocabulary from the collected 3dnews/ixbt articles.

A token counts as technical when it is Latin script and either
  * contains a digit (DDR4, RTX 5060, LPDDR5X, IP68, FP32), or
  * is not a normal English word in the frequency corpus (data/en_50k.txt),
    i.e. it is a brand, standard or model name (Nvidia, NVMe, OLED, CUDA).

Outputs
  data/tech_terms.txt          "word count" lines, merged into the EN model
  abrakadabra/data/tech.json      {"tokens": [["ddr4", 9], ...]} lexicon with digits
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
DATA = os.path.join(os.path.dirname(ROOT), "data")  # Abrakadra/data
ARTICLES = os.path.join(DATA, "articles")
OUT_LEX = os.path.join(ROOT, "data", "tech.json")
OUT_WORDS = os.path.join(DATA, "tech_terms.txt")
OUT_RU_WORDS = os.path.join(DATA, "tech_ru_terms.txt")

TOKEN = re.compile(r"[A-Za-z][A-Za-z0-9'\-]*")
CYR = re.compile(r"[А-Яа-яёЁ]{4,}")
BOILERPLATE = {
    "mdash", "ndash", "mdash", "laquo", "raquo", "ldquo", "rdquo", "fnof",
    "sect", "copy", "nbsp", "hellip", "middot", "deg", "plusmn", "times",
    "dnews", "ixbt", "com", "www", "https", "http", "page", "img", "jpg",
    "jpeg", "png", "gif", "svg", "utm", "source", "medium", "referral",
    "campaign", "shop", "cookie", "cookies", "youtube", "google", "app",
    "store", "rss", "amp", "style", "class", "alt", "title", "src", "href",
    "the", "and", "for", "with", "from", "that", "this", "your", "you",
    "not", "have", "are", "was", "will", "its", "by", "of", "to", "in",
    "on", "at", "as", "or", "an", "a", "is", "it", "be", "we", "us", "he",
    "she", "they", "them", "their", "which", "when", "what", "who", "how",
    "why", "all", "one", "two", "three", "new", "can", "get", "more",
    "also", "such", "about", "after", "into", "but", "just", "like", "very",
    "most", "only", "now", "year", "years", "day", "days", "time", "may",
    "say", "said", "no", "yes", "than", "then", "so", "if", "there", "here",
    "made", "make", "has", "had", "been", "were", "does", "did", "been",
    "min", "sec", "kb", "mb", "gb", "tb", "bit", "bits", "hr", "pm", "am",
    "read", "video", "photo", "photos", "news", "live", "comments", "share",
    "print", "subscribe", "email", "mail", "login", "sign", "register",
    "search", "menu", "home", "top", "best", "rate", "rating", "price",
    "prices", "buy", "sale", "shop", "market", "author", "authors", "date",
    "view", "views", "count", "next", "prev", "previous", "follow", "telegram",
    "vk", "twitter", "facebook", "instagram", "rutube", "zen", "dzen", "max",
    "ru", "en", "de", "ua", "id", "ii", "iii", "iv", "v", "x", "c", "g", "p",
    "ti", "xt", "se", "pro", "max", "ultra", "lite", "air", "mini", "plus",
    "one", "pcs", "pc", "iphone", "ipad", "macbook", "android", "windows",
}


def load_english_words(path):
    words = set()
    if os.path.exists(path):
        for line in open(path, encoding="utf-8"):
            parts = line.split()
            if len(parts) == 2:
                words.add(parts[0].lower())
    return words


def article_text(path):
    raw = open(path, encoding="utf-8").read()
    raw = html.unescape(raw)
    raw = re.sub(r"<script[\s\S]*?</script>", " ", raw)
    raw = re.sub(r"<style[\s\S]*?</style>", " ", raw)
    raw = re.sub(r"https?://\S+", " ", raw)  # no urls: they are not prose
    return raw


SEED = {
    "ddr3", "ddr4", "ddr5", "ddr", "lpddr", "lpddr4", "lpddr5", "nvme", "ssd",
    "hdd", "sata", "pcie", "pci", "cpu", "gpu", "ram", "rom", "bios", "uefi",
    "hdmi", "displayport", "usb", "usbc", "typec", "wifi", "wi-fi", "wi-fi6",
    "bluetooth", "ethernet", "lan", "wan", "soc", "npu", "tpu", "dsp", "isp",
    "oled", "amoled", "ips", "lcd", "led", "qled", "miniled", "microled",
    "4k", "8k", "1080p", "1440p", "2160p", "720p", "hdr", "dpi", "fps",
    "ghz", "mhz", "tdp", "watt", "watts", "mah", "kwh", "wh", "nit", "nits",
    "bit", "bits", "byte", "bytes", "gb", "tb", "mb", "kb", "tb", "ssd",
    "cores", "ядро", "transistor", "nm", "finfet", "euv", "lithography",
    "driver", "drivers", " firmware", "firmware", "cache", "buffer", "benchmark",
    "fps", "latency", "bandwidth", "refresh", "hz", "gamut", "srgb", "dci",
    "p3", "deltae", "contrast", "luma", "chroma", "codec", "h264", "h265",
    "hevc", "av1", "dolby", "aptx", "aac", "mp3", "flac", "jpeg", "png", "svg",
    "android", "ios", "windows", "linux", "macos", "chrome", "firefox",
    "iphone", "ipad", "macbook", "imac", "airpods", "watch", "pixel", "galaxy",
    "redmi", "poco", "oneplus", "realme", "vivo", "oppo", "asus", "acer",
    "lenovo", "dell", "hp", "msi", "gigabyte", "asrock", "corsair", "kingston",
    "sandisk", "seagate", "toshiba", "crucial", "logitech", "razer", "steelseries",
    "intel", "amd", "nvidia", "qualcomm", "mediatek", "snapdragon", "tensor",
    "apple", "samsung", "xiaomi", "huawei", "tesla", "spacex", "microsoft",
    "google", "yandex", "telegram", "youtube", "netflix", "spotify",
    "rtx", "gtx", "radeon", "geforce", "cuda", "directx", "vulkan", "opengl",
    "m2", "m3", "a14", "a15", "a17", "i5", "i7", "i9", "ryzen", "threadripper",
    "xbox", "playstation", "ps5", "ps4", "switch", "nintendo", "steam",
    "vr", "ar", "mr", " drone", "drone", "robot", "robotics", "ai", "ml",
    "llm", "neural", "network", "cloud", "server", "datacenter", "gpu",
}


def main():
    english = load_english_words(os.path.join(DATA, "en_50k.txt"))
    russian = load_english_words(os.path.join(DATA, "ru_50k.txt"))
    counter = Counter()
    cyr = Counter()
    for s in SEED:
        s = s.strip().lower()
        if s and s.isascii():
            counter[s] += 2  # seed: always known, even if unseen in the sample
    for name in sorted(os.listdir(ARTICLES)):
        if not name.endswith(".txt"):
            continue
        text = article_text(os.path.join(ARTICLES, name))
        for m in TOKEN.finditer(text):
            tok = m.group(0).lower().strip("-'")
            if len(tok) < 2 or tok in BOILERPLATE:
                continue
            has_digit = any(ch.isdigit() for ch in tok)
            letters = "".join(ch for ch in tok if ch.isalpha())
            if not has_digit and (len(letters) < 3 or letters in english or tok in english):
                continue
            counter[tok] += 1
        for m in CYR.finditer(text):
            w = m.group(0).lower().replace("ё", "е")
            if w not in russian:
                cyr[w] += 1

    lex = [(w, c) for w, c in counter.most_common() if c >= 2]
    os.makedirs(os.path.dirname(OUT_LEX), exist_ok=True)
    with open(OUT_LEX, "w", encoding="utf-8") as fh:
        json.dump({"tokens": lex}, fh, ensure_ascii=False, separators=(",", ":"))

    # letter-only forms feed the language model itself
    merged = Counter()
    for w, c in lex:
        letters = "".join(ch for ch in w if ch.isalpha())
        if len(letters) >= 2:
            merged[letters] += c
    with open(OUT_WORDS, "w", encoding="utf-8") as fh:
        for w, c in sorted(merged.items(), key=lambda kv: -kv[1]):
            fh.write(f"{w} {c}\n")

    # Russian technical vocabulary that the general corpus does not know
    with open(OUT_RU_WORDS, "w", encoding="utf-8") as fh:
        for w, c in sorted((w, c) for w, c in cyr.most_common() if c >= 2):
            fh.write(f"{w} {c}\n")

    print(f"lexicon tokens: {len(lex)}  ({sum(counter.values())} occurrences)")
    print("letter words:", len(merged))
    print("new russian technical words:", sum(1 for w, c in cyr.items() if c >= 3))
    for w, c in lex[:50]:
        print(f"  {w}\t{c}")


if __name__ == "__main__":
    main()
