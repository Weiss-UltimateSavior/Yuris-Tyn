# -*- coding: utf-8 -*-
"""P5.2 —— dump s13 g3 GOSUB 的标签名并查 YSLB。"""
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
for gi in range(3):
    ro += ((part1[gi] >> 8) & 0xFF) * 12
base = 0x20 + 4 * g + ro
tag, ln, off = struct.unpack_from("<III", b, base)
seg = content[off:off + ln]
print("g3 w0 tag=%08x len=%d" % (tag, ln))
print("hex:", " ".join("%02x" % x for x in seg))
print("raw:", seg[3:].decode("cp932", "replace"))

# 查 YSLB(YSLB 每条: len name hash4 pc4 sid2 (+1 怪癖))
ysl_key = [k for k in e if k.endswith("ysl.ybn")][0]
ysl = read(d, e[ysl_key])
cnt = struct.unpack_from("<I", ysl, 8)[0]
p = 0x10 + 256 * 4
needle = seg[4:ln - 1]  # 去 0x4d+len(2B)/首界定符 与尾界定符
hits = []
for _ in range(cnt):
    l = ysl[p]; p += 1
    nm = ysl[p:p + l]; p += l
    if p + 10 > len(ysl):
        break
    pc, sid = struct.unpack_from("<IH", ysl, p + 4)
    p += 11
    if nm == needle:
        hits.append((nm.decode("cp932"), pc, sid))
print("YSLB hits:", hits)
