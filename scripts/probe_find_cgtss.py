# -*- coding: utf-8 -*-
"""P5.2 —— 全语料定位 es.CGTSS 调用点(临时侦查脚本)。"""
import re
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
pat = re.compile(rb"CGTSS")
for name, ent in sorted(e.items()):
    if not name.endswith(".ybn") or "yst_list" in name or "ysc" in name:
        continue
    try:
        b = decrypt(read(d, ent))
    except Exception:
        continue
    for m in pat.finditer(b):
        # 回溯 3 字节看 op 与窗口上下文:0x4d len_lo len_hi "..."
        print(name, hex(m.start()), b[max(0, m.start() - 8):m.start() + 12].hex())
