# -*- coding: utf-8 -*-
"""P7.1 —— YSCM 指定命令的参数名表(CG/CGACT/CGINFO)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
yscm = read(d, e["%ysbin\\ysc.ybn"])
count = struct.unpack_from("<I", yscm, 8)[0]
p = 0x10
want = set(sys.argv[1:]) or {"CG", "CGACT", "CGINFO"}
for i in range(count):
    end = yscm.index(b"\0", p)
    name = yscm[p:end].decode("ascii")
    p = end + 1
    pc = yscm[p]
    p += 1
    params = []
    for _ in range(pc):
        end = yscm.index(b"\0", p)
        params.append(yscm[p:end].decode("ascii"))
        p = end + 1
        p += 2
    if name in want:
        print("%s (%d params):" % (name, pc))
        for i2, pn in enumerate(params):
            print("  slot %2d: %s" % (i2, pn))
