# -*- coding: utf-8 -*-
"""P7.1 —— dump s9 指定偏移的窗口原始字节(FILE 槽表达式解码)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
b = decrypt(read(d, [v for k, v in e.items() if k.endswith("yst00009.ybn")][0]))
magic, ver, g = struct.unpack_from("<4sII", b, 0)
part1 = struct.unpack_from("<%dI" % g, b, 0x20)
tw = sum((v >> 8) & 0xFF for v in part1)
content = b[0x20 + 4 * g + tw * 12:]

# (off, len) 对:文件槽表达式 + 名字表达式
for off, ln in [(28598, 5), (33368, 5), (27417, 16), (33321, 16), (27036, 21)]:
    seg = content[off:off + ln]
    print("off=%-6d len=%-3d %s  | %r" % (off, ln, seg.hex(" "), seg))
