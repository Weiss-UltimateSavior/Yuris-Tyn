# -*- coding: utf-8 -*-
"""P5 —— YSLB 查询命令行:打印标签 → (script, pc)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
ysl = read(d, e[[k for k in e if k.endswith("ysl.ybn")][0]])
cnt = struct.unpack_from("<I", ysl, 8)[0]
p = 12 + 256 * 4
table = {}
for _ in range(cnt):
    l = ysl[p]; p += 1
    nm = ysl[p:p + l]; p += l
    pc, sid = struct.unpack_from("<IH", ysl, p + 4)
    p += 12
    table.setdefault(nm.decode("cp932", "replace"), []).append((sid, pc))

for name in sys.argv[1:]:
    print(name, "->", table.get(name, "NOT FOUND"))
