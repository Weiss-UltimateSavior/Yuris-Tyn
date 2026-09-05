# -*- coding: utf-8 -*-
"""P5.2 —— dump 指定脚本指定组(含 gparam 解码与全部窗)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt, yscm_names, decode_expr

ypf = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf"
script_id = int(sys.argv[1]) if len(sys.argv) > 1 else 22
lo = int(sys.argv[2]) if len(sys.argv) > 2 else 74
hi = int(sys.argv[3]) if len(sys.argv) > 3 else 92

d = open(ypf, "rb").read()
entries = parse_ypf(d)
yscm = read(d, entries["%ysbin\\ysc.ybn"])
names = yscm_names(yscm)
target = None
for k, e in entries.items():
    if k.endswith("\\yst%05d.ybn" % script_id):
        target = e
        break
b = decrypt(read(d, target))
magic, ver, g = struct.unpack_from("<4sII", b, 0)
part1 = struct.unpack_from("<%dI" % g, b, 0x20)
total_windows = sum((v >> 8) & 0xFF for v in part1)
content = b[0x20 + 4 * g + total_windows * 12:]

rec_off = 0
for gi in range(g):
    v = part1[gi]
    cmd = v & 0xFF
    cnt = (v >> 8) & 0xFF
    gparam = (v >> 16) & 0xFFFF
    ro = rec_off
    rec_off += cnt * 12
    if not (lo <= gi < hi):
        continue
    nm = names[cmd] if cmd < len(names) else "?"
    if cmd == 0x2B:
        ic = (gparam & 0xFF) >> 3
        fc = (gparam & 7) * 4 + (gparam >> 14)
        sc = (gparam >> 9) & 0x1F
        gpd = f" int={ic} flt={fc} str={sc}"
    else:
        gpd = ""
    print(f"g{gi}: 0x{cmd:02x} {nm} windows={cnt} gparam=0x{gparam:04x}{gpd}")
    for wi in range(cnt):
        base = 0x20 + 4 * g + ro + wi * 12
        tag, ln, off = struct.unpack_from("<III", b, base)
        b0 = tag & 0xFF
        b2 = (tag >> 16) & 0xFF
        b3 = (tag >> 24) & 0xFF
        expr = decode_expr(content, off, min(ln, 200)) if ln else ""
        print(f"    w{wi}: B0={b0} B2={b2} B3={b3} len={ln} {expr}")
