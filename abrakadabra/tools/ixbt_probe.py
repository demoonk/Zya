import re
import sys
import urllib.request
from collections import Counter

sys.stdout.reconfigure(encoding="utf-8")
UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36"

for page in ("https://www.ixbt.com/news/", "https://www.ixbt.com/live/"):
    try:
        h = urllib.request.urlopen(urllib.request.Request(page, headers={"User-Agent": UA}), timeout=30).read().decode("utf-8", "replace")
    except Exception as exc:
        print(page, "failed", exc)
        continue
    urls = [m.group(1) for m in re.finditer(r'href="([^"]+)"', h)]
    seg = Counter(u.split("/")[1] if len(u.split("/")) > 1 else "" for u in urls)
    print(page, len(h), seg.most_common(12))
    print("sample:", [u for u in urls if re.search(r"\d{4,6}", u)][:12])
