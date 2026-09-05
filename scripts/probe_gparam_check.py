#!/usr/bin/env python3
"""P5.2 —— 校验 GOSUB gparam 解码与实参窗一致性(全语料扫描)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt, yscm_names

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
yscm = read(d, e["%ysbin\\ysc.ybn"])
names = yscm_names(yscm)

bad = 0
checked = 0
samples = []
for name, ent in sorted(e.items()):
    if not name.endswith(".ybn") or not name.startswith("$ysbin\\yst0"):
        continue
    b = decrypt(read(d, ent))
    magic, ver, g = struct.unpack_from("<4sII", b, 0)
    if magic != b"YSTB":
        continue
    part1 = struct.unpack_from("<%dI" % g, b, 0x20)
    total_windows = sum((v >> 8) & 0xFF for v in part1)
    rec_off = 0
    for gi in range(g):
        v = part1[gi]
        cmd = v & 0xFF
        cnt = (v >> 8) & 0xFF
        gparam = (v >> 16) & 0xFFFF
        if cmd != 0x2B:  # GOSUB
            rec_off += cnt * 12
            continue
        checked += 1
        int_c = (gparam & 0xFF) >> 3
        flt_c = (gparam & 7) * 4 + (gparam >> 14)
        str_c = (gparam >> 9) & 0x1F
        max_int = max_flt = max_str = 0
        for wi in range(cnt):
            base = 0x20 + 4 * g + rec_off + wi * 12
            tag, ln, off = struct.unpack_from("<III", b, base)
            b0 = tag & 0xFF
            if 0x01 <= b0 <= 0x0F:
                max_int = max(max_int, b0)
            elif 0x10 <= b0 <= 0x1F:
                max_flt = max(max_flt, b0 - 0x10)
            elif 0x20 <= b0 <= 0x2F:
                max_str = max(max_str, b0 - 0x20)
        if (max_int > int_c or max_flt > flt_c or max_str > str_c):
            bad += 1
            if len(samples) < 12:
                samples.append((name, gi, hex(gparam), int_c, flt_c, str_c,
                                max_int, max_flt, max_str))
        rec_off += cnt * 12

print(f"GOSUB groups checked: {checked}, gparam/arg inconsistent: {bad}")
for s in samples:
    print(f"  {s[0]} g{s[1]} gparam={s[2]} int={s[3]}/{s[6]} flt={s[4]}/{s[7]} str={s[5]}/{s[8]} (declared/max_arg)")
