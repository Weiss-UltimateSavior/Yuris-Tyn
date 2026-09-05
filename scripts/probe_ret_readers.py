# -*- coding: utf-8 -*-
"""P5.2 —— 全语料:@60/@61/$62 读者下标分布(含 $ 前缀)。"""
import struct
import sys
from collections import Counter

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)

readers = {60: Counter(), 61: Counter(), 62: Counter()}


def scan_expr(content, off, ln):
    p = off
    end = off + ln
    last_ref = None
    pending = []
    while p + 3 <= end:
        op = content[p]
        ilen = struct.unpack_from("<H", content, p + 1)[0]
        operand = content[p + 3:p + 3 + ilen]
        p += 3 + ilen
        if op in (0x48, 0x56, 0x76) and ilen == 3 and operand[0] in (0x40, 0x24):
            vid = struct.unpack_from("<H", operand, 1)[0]
            last_ref = (operand[0], vid)
            pending = []
        elif op in (0x42, 0x57, 0x49) and last_ref is not None:
            if op == 0x42 and ilen == 1:
                pending.append(operand[0])
            elif op == 0x57 and ilen == 2:
                pending.append(struct.unpack_from("<h", operand)[0])
            elif op == 0x49 and ilen == 4:
                pending.append(struct.unpack_from("<i", operand)[0])
        elif op == 0x29 and last_ref is not None and pending:
            pre, vid = last_ref
            key = (60 if pre == 0x40 else 62) if vid in (60, 61, 62) else None
            if key:
                readers[key][tuple(pending)] += 1
            last_ref = None
            pending = []
        elif op in (0x2b, 0x2d, 0x2a, 0x2f, 0x3d, 0x21, 0x3e, 0x3c, 0x2c):
            pass
        if p >= end:
            break


for name, ent in sorted(e.items()):
    if not name.endswith(".ybn") or not name.startswith("$ysbin\\yst0"):
        continue
    try:
        b = decrypt(read(d, ent))
    except Exception:
        continue
    magic, ver, g = struct.unpack_from("<4sII", b, 0)
    if magic != b"YSTB":
        continue
    part1 = struct.unpack_from("<%dI" % g, b, 0x20)
    tw = sum((v >> 8) & 0xFF for v in part1)
    content = b[0x20 + 4 * g + tw * 12:]
    ro = 0
    for gi in range(g):
        v = part1[gi]
        cnt = (v >> 8) & 0xFF
        for wi in range(cnt):
            base = 0x20 + 4 * g + ro + wi * 12
            tag, ln, off = struct.unpack_from("<III", b, base)
            if ln:
                scan_expr(content, off, ln)
        ro += cnt * 12

print("@60 idx:", dict(readers[60].most_common(8)))
print("@61 idx:", dict(readers[61].most_common(8)))
print("$62 idx:", dict(readers[62].most_common(8)))
