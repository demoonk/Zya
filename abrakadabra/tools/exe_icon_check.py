"""Walk the PE resource tree of an executable and report icon + manifest entries.

Usage: python tools/exe_icon_check.py <exe> [out-prefix]
"""
import struct
import sys


def resource_root(data):
    pe = struct.unpack("<I", data[60:64])[0]
    count = struct.unpack("<H", data[pe + 20:pe + 22])[0]
    for index in range(count):
        entry = pe + 24 + index * 40
        if data[entry:entry + 5] == b".rsrc":
            return struct.unpack("<I", data[entry + 20:entry + 24])[0]
    return None


def directory(data, offset):
    # IMAGE_RESOURCE_DIRECTORY: 16-byte header, then entries of 8 bytes.
    entries = struct.unpack("<I", data[offset + 8:offset + 12])[0]
    return [
        struct.unpack("<II", data[offset + 16 + i * 8:offset + 24 + i * 8])
        for i in range(entries)
    ]


def main():
    path = sys.argv[1]
    prefix = sys.argv[2] if len(sys.argv) > 2 else "data/check/res"
    data = open(path, "rb").read()
    root = resource_root(data)
    if root is None:
        print(f"{path}: no .rsrc section")
        return

    for kind, child in directory(data, root):
        if not child & 0x80000000:
            continue
        sub = directory(data, root + (child & 0x7FFFFFFF))
        for name, offset in sub:
            start = root + offset + 8
            if kind == 3:  # RT_ICON
                width, height = struct.unpack("<II", data[start + 4:start + 12])
                pixel_size = struct.unpack("<I", data[start + 18:start + 22])[0]
                body = data[start:start + 22 + pixel_size + 40]
                out = f"{prefix}_{name}_{width}x{height}.ico"
                with open(out, "wb") as handle:
                    handle.write(b"\x00\x00\x01\x00\x01\x00")
                    handle.write(struct.pack("<HHIB", width, height, 32, len(body)))
                    handle.write(struct.pack("<I", 22))
                    handle.write(body)
                print(f"{path}: icon {name} {width}x{height} -> {out}")
            elif kind == 10:  # RT_MANIFEST
                text = data[start:start + 200].decode("utf-8", "replace")
                print(f"{path}: manifest {name}: {text[:140]}")


if __name__ == "__main__":
    main()
