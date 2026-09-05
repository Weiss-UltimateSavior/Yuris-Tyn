# -*- coding: utf-8 -*-
"""P7.1 —— 语料 CG(0x01) 命令槽位使用分布。"""
import collections
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
combo = collections.Counter()
for k in sorted(e.keys()):
    if ".ybn" not in k or "yst" not in k:
        continue
    try:
        sid = int(k.split("yst")[-1].split(".")[0])
    except ValueError:
        continue
    b = decrypt(read(d, e[k]))
    magic, ver, g = struct.unpack_from("<4sII", b, 0)
    part1 = struct.unpack_from("<%dI" % g, b, 0x20)
    tw = sum((v >> 8) & 0xFF for v in part1)
    ro = 0
    for gi in range(g):
        v = part1[gi]
        cmd = v & 0xFF
        cnt = (v >> 8) & 0xFF
        gro = ro
        ro += cnt * 12
        if cmd != 0x01:
            continue
        slots = tuple()
        for wi in range(cnt):
            base = 0x20 + 4 * g + gro + wi * 12
            tag, ln, off = struct.unpack_from("<III", b, base)
            if ln > 0:
                slots += ((tag & 0xFF),)
        combo[slots] += 1
for slots, n in sorted(combo.items(), key=lambda kv: -kv[1]):
    print("[%s] x%d" % (",".join(str(s) for s in slots), n))
