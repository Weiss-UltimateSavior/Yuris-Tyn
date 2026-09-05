# -*- coding: utf-8 -*-
"""P5 —— 全语料扫描指定变量的引用窗(全命令;内联验证版逻辑)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
TARGET = int(sys.argv[1])
COUNT = 0

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
    content = b[0x20 + 4 * g + tw * 12:]
    ro = 0
    for gi in range(g):
        v = part1[gi]
        cmd = v & 0xFF
        cnt = (v >> 8) & 0xFF
        gro = ro
        ro += cnt * 12
        for wi in range(cnt):
            base = 0x20 + 4 * g + gro + wi * 12
            tag, ln, off = struct.unpack_from("<III", b, base)
            b0 = tag & 0xFF
            raw = content[off:off + ln]
            p = 0
            while p + 6 <= len(raw):
                if raw[p] in (0x48, 0x56, 0x76) and raw[p + 3] in (0x40, 0x24):
                    vid = struct.unpack_from("<H", raw, p + 4)[0]
                    if vid == TARGET:
                        print("s%d g%d cmd=%02x w%d B0=%d len=%d raw=%s"
                              % (sid, gi, cmd, wi, b0, ln, raw[:24].hex(" ")))
                        COUNT += 1
                p += 1
print("total refs:", COUNT)
