# -*- coding: utf-8 -*-
"""P5.2 —— 缺口命令名映射(临时侦查脚本)。"""
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, yscm_names

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
key = [k for k in e if k.endswith("ysc.ybn")][0]
yscm = read(d, e[key])
names = yscm_names(yscm)
for c in [0x1b, 0x39, 0x3c, 0x59, 0x36, 0x15, 0x6b, 0x19, 0x45, 0x1a, 0x5d, 0x0e, 0x14, 0x1c, 0x31, 0x68]:
    print(hex(c), names[c] if c < len(names) else "?")
