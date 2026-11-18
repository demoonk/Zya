#!/usr/bin/env python3
"""Fetch 10 recent tech articles from 3dnews.ru and ixbt.com into data/articles/."""
import json
import os
import re
import sys
import urllib.request
from urllib.parse import urljoin

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "..", "data")
OUT = os.path.join(DATA, "articles")
UA = ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
      "(KHTML, like Gecko) Chrome/120.0 Safari/537.36")

sys.stdout.reconfigure(encoding="utf-8")


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept-Language": "ru,en;q=0.8"})
    return urllib.request.urlopen(req, timeout=30).read().decode("utf-8", "replace")


def clean(html):
    html = re.sub(r"<script[\s\S]*?</script>", " ", html)
    html = re.sub(r"<style[\s\S]*?</style>", " ", html)
    html = re.sub(r"<!--[\s\S]*?-->", " ", html)
    # keep paragraph-ish structure
    html = re.sub(r"</(p|li|tr|h[1-6]|div)>", "\n", html)
    text = re.sub(r"<[^>]+>", " ", html)
    text = text.replace("&nbsp;", " ").replace("&amp;", "&").replace("&quot;", '"')
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r"\n\s*\n+", "\n", text)
    return text.strip()


def links_from(html, base, patterns):
    found = []
    seen = set()
    for m in re.finditer(r'href="([^"]+)"', html):
        u = urljoin(base, m.group(1))
        if any(re.search(p, u) for p in patterns) and u not in seen:
            seen.add(u)
            found.append(u)
    return found


def main():
    os.makedirs(OUT, exist_ok=True)
    plans = {
        "3dnews": [
            ("https://3dnews.ru/news/", [r"3dnews\.ru/\d{6,7}"]),
            ("https://3dnews.ru/", [r"3dnews\.ru/\d{6,7}"]),
        ],
        "ixbt": [
            ("https://www.ixbt.com/news/", [r"ixbt\.com/news/\d{4}-\d{2}-\d{2}/[a-z0-9-]+\.html"]),
            ("https://www.ixbt.com/", [r"ixbt\.com/(news|live)/[a-z0-9/-]+\.html"]),
        ],
    }
    index = {}
    for site, sources in plans.items():
        urls = []
        for base, pats in sources:
            try:
                html = get(base)
            except Exception as exc:
                print(f"{site}: {base} failed: {exc}")
                continue
            for u in links_from(html, base, pats):
                if u not in urls:
                    urls.append(u)
        index[site] = urls[:10]
        print(f"{site}: found {len(urls)} candidate article urls")

    meta = []
    for site, urls in index.items():
        for i, u in enumerate(urls, 1):
            try:
                html = get(u)
                title = re.search(r"<title>(.*?)</title>", html)
                title = title.group(1).strip() if title else u
                body = clean(html)
                if len(body) < 500:
                    print(f"{site} {u}: too short ({len(body)})")
                    continue
                path = os.path.join(OUT, f"{site}_{i}.txt")
                with open(path, "w", encoding="utf-8") as fh:
                    fh.write(title + "\n\n" + body)
                meta.append({"site": site, "n": i, "url": u, "title": title, "chars": len(body)})
                print(f"{site}_{i}: {len(body)} chars  {title[:70]}")
            except Exception as exc:
                print(f"{site} {u}: failed {exc}")
    with open(os.path.join(OUT, "index.json"), "w", encoding="utf-8") as fh:
        json.dump(meta, fh, ensure_ascii=False, indent=2)
    print(f"saved {len(meta)} articles")


if __name__ == "__main__":
    main()
