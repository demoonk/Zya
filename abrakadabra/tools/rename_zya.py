#!/usr/bin/env python3
"""Rename the app from AbraKadabra to `zя`.

The crate/binary name stays ASCII (`zya`): MinGW's ld cannot write an output
file whose name contains non-ASCII characters. The distributable is renamed to
`zя.exe` by tools/build.ps1 after every build. The repository folder and the
data paths are intentionally left ASCII.
"""
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")

ROOT = Path(__file__).resolve().parent.parent
DIR_SENTINEL = "\x00KEEP_DIR\x00"

ORDER = [
    ("abrakadabra/data/", DIR_SENTINEL),
    ("abrakadabra.rc", "zya.rc"),
    ("abrakadabra_icon.obj", "zya_icon.obj"),
    ("ABRA_VERBOSE", "ZYA_VERBOSE"),
    ("ABRA_PEER_UNDO", "ZYA_PEER_UNDO"),
    ("0x4142_5241", "0x5A59_4100"),
    ("AbraKadabraTray", "ZyaTray"),
    ("AbraKadabraPeer", "ZyaPeer"),
    ("AbraKadabraDemo", "ZyaDemo"),
    ("AbraKadabraArticle", "ZyaArticle"),
    ("AbraKadabraProbe", "ZyaProbe"),
    ('name = "abrakadabra"', 'name = "zya"'),
    ("Abrakadabra", "Abrakadra"),  # the outer folder keeps its name
    ("AbraKadabra", "zя"),
    ("abrakadabra", "zя"),
    (DIR_SENTINEL, "abrakadabra/data/"),
]

targets = [ROOT / "Cargo.toml", ROOT / "build.rs", ROOT / "README.md"]
targets += sorted((ROOT / "src").glob("*.rs"))

changed = 0
for p in targets:
    text = p.read_text(encoding="utf-8")
    new = text
    for a, b in ORDER:
        new = new.replace(a, b)
    if new != text:
        p.write_text(new, encoding="utf-8")
        changed += 1
        print(f"rewrote {p.relative_to(ROOT)}")
print(f"files changed: {changed}")
