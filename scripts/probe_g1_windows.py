# -*- coding: utf-8 -*-
"""P5.2 —— dump s13 g1 GOSUB 窗口(标签名 + STR 实参)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
target = [k for k in e if k.endswith("yst00013.ybn")][0]
b = decrypt(read(d, e[target]))
magic, ver, g = struct.unpack_from("<4sII", b, 0)
part1 = struct.unpack_from("<%dI" % g, b, 0x20)
tw = sum((v >> 8) & 0xFF for v in part1)
content = b[0x20 + 4 * g + tw * 12:]

ro = 0
for gi in range(1):
    ro += ((part1[gi] >> 8) & 0xFF) * 12
for wi in range(2):
    base = 0x20 + 4 * g + ro + wi * 12
    tag, ln, off = struct.unpack_from("<III", b, base)
    seg = content[off:off + ln]
    print("g1 w%d tag=%08x len=%d" % (wi, tag, ln))
    print("  hex:", " ".join("%02x" % x for x in seg))
    if seg[0] == 0x4D:
        print("  str:", seg[3:].decode("cp932", "replace"))
