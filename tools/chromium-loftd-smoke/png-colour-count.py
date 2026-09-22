#!/usr/bin/env python3
"""Count pixels of one exact RGB colour in a PNG. Usage: png-colour-count.py FILE RRGGBB

Used by the --waypipe smoke to assert that the guest's painted pattern really
reached the host compositor's screenshot. A file-existence or file-size check is
not enough: weston writes a plausible, all-black PNG when a screenshot is
refused, so the pixels themselves have to be decoded.
"""
import struct
import sys
import zlib


def decode(path):
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit("not a PNG: %s" % path)
    pos, idat = 8, b""
    width = height = depth = colour = None
    while pos < len(data):
        (length, kind) = struct.unpack(">I4s", data[pos:pos + 8])
        pos += 8
        chunk = data[pos:pos + length]
        pos += length + 4
        if kind == b"IHDR":
            (width, height, depth, colour, _, _, _) = struct.unpack(">IIBBBBB", chunk)
        elif kind == b"IDAT":
            idat += chunk
        elif kind == b"IEND":
            break
    if depth != 8 or colour not in (2, 6):
        raise SystemExit("unsupported PNG (depth=%s colour=%s)" % (depth, colour))
    channels = 3 if colour == 2 else 4
    raw = zlib.decompress(idat)
    stride = width * channels
    rows, prev, p = [], bytearray(stride), 0
    for _ in range(height):
        flt = raw[p]
        p += 1
        line = bytearray(raw[p:p + stride])
        p += stride
        for i in range(stride):
            a = line[i - channels] if i >= channels else 0
            b = prev[i]
            c = prev[i - channels] if i >= channels else 0
            x = line[i]
            if flt == 1:
                x = (x + a) & 255
            elif flt == 2:
                x = (x + b) & 255
            elif flt == 3:
                x = (x + ((a + b) >> 1)) & 255
            elif flt == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                x = (x + pr) & 255
            line[i] = x
        rows.append(bytes(line))
        prev = line
    return channels, rows


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    path, target = sys.argv[1], sys.argv[2].lower()
    want = bytes.fromhex(target)
    channels, rows = decode(path)
    count = 0
    for row in rows:
        for i in range(0, len(row), channels):
            if row[i:i + 3] == want:
                count += 1
    print(count)


main()
