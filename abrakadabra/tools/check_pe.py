import sys

sys.stdout.reconfigure(encoding="utf-8")
path = sys.argv[1]
data = open(path, "rb").read()

pe = data.find(b"PE\x00\x00")
if pe < 0:
    print("not a PE")
    raise SystemExit(1)

num_sections = int.from_bytes(data[pe + 6 : pe + 8], "little")
size_optional = int.from_bytes(data[pe + 20 : pe + 22], "little")
sect_start = pe + 24 + size_optional

rsrc_raw = None
for i in range(num_sections):
    off = sect_start + i * 40
    name = data[off : off + 8].rstrip(b"\x00")
    raw_off = int.from_bytes(data[off + 16 : off + 20], "little")
    if name.endswith(b".rsrc") or name.endswith(b".rsr"):
        rsrc_raw = raw_off
        break

if rsrc_raw is None:
    print("no .rsrc section -> no resources (no icon)")
    raise SystemExit(1)

def child_dir(off, want):
    n = int.from_bytes(data[rsrc_raw + off + 8 : rsrc_raw + off + 12], "little")
    for i in range(n):
        e = rsrc_raw + off + 16 + i * 8
        rid = int.from_bytes(data[e : e + 4], "little")
        nxt = int.from_bytes(data[e + 4 : e + 8], "little") & 0x7FFFFFFF
        if rid == want:
            return nxt
    return None

top_entries = int.from_bytes(data[rsrc_raw + 8 : rsrc_raw + 12], "little")
types = []
for i in range(top_entries):
    e = rsrc_raw + 16 + i * 8
    types.append(int.from_bytes(data[e : e + 4], "little"))
print("top-level resource types:", types)

grp = child_dir(0, 14)
icons = child_dir(0, 3)
if grp is None or icons is None:
    print("no icon resources")
    raise SystemExit(1)

n = int.from_bytes(data[rsrc_raw + grp + 8 : rsrc_raw + grp + 12], "little")
print("icon entries in group:", n)
sizes = []
for i in range(n):
    e = rsrc_raw + grp + 16 + i * 8
    rid = int.from_bytes(data[e : e + 4], "little")
    nxt = int.from_bytes(data[e + 4 : e + 8], "little") & 0x7FFFFFFF
    size = int.from_bytes(data[e + 8 : e + 12], "little")
    sub = child_dir(icons, rid)
    if sub:
        d = data[rsrc_raw + sub : rsrc_raw + sub + size]
        w, h = d[0], d[1]
        sizes.append((w or 256, h or 256, size))
print("icon images:", sizes)
