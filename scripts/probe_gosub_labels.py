# -*- coding: utf-8 -*-
"""P5.2 —— dump s126 指定组的 GOSUB 标签与实参(字节级)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
target = [k for k in e if k.endswith("yst00126.ybn")][0]
b = decrypt(read(d, e[target]))
magic, ver, g = struct.unpack_from("<4sII", b, 0)
part1 = struct.unpack_from("<%dI" % g, b, 0x20)
tw = sum((v >> 8) & 0xFF for v in part1)
content = b[0x20 + 4 * g + tw * 12:]

ro = 0
want = {145, 146, 147, 148, 730, 731, 732}
for gi in range(g):
    v = part1[gi]
    cmd = v & 0xFF
    cnt = (v >> 8) & 0xFF
    if gi in want and cmd == 0x2B:
        print(f"g{gi}: GOSUB windows={cnt}")
        for wi in range(cnt):
            base = 0x20 + 4 * g + ro + wi * 12
            tag, ln, off = struct.unpack_from("<III", b, base)
            seg = content[off:off + ln]
            b0 = tag & 0xFF
            if seg[0] == 0x4D:
                print(f"  w{wi} B0={b0}: {seg[3:].decode('cp932', 'replace')}")
            else:
                print(f"  w{wi} B0={b0}: {' '.join('%02x' % x for x in seg[:16])}")
    ro += cnt * 12
