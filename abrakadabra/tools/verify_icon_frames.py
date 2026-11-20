"""Check that the icon frames baked into an executable match data/app.ico exactly."""
import io
import struct
import sys

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")


def frames(path):
    ico = open(path, "rb").read()
    count = struct.unpack("<H", ico[4:6])[0]
    out = []
    for i in range(count):
        entry = ico[6 + i * 16 : 6 + i * 16 + 16]
        width, height, _, _, _, _, size, offset = struct.unpack("<BBBBHHII", entry)
        out.append((width or 256, height or 256, size, ico[offset : offset + size]))
    return out


def main():
    reference = frames("data/app.ico")
    print("data/app.ico frames:", [(w, h, s) for w, h, s, _ in reference])
    for path in sys.argv[1:]:
        data = open(path, "rb").read()
        hits = [data.count(frame) for _, _, _, frame in reference]
        admin = b"requireAdministrator" in data
        print(f"{path}: {len(hits)}/{len(hits)} frames present {hits}, admin={int(admin)}")


if __name__ == "__main__":
    main()
