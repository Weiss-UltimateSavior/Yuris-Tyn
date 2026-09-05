# -*- coding: utf-8 -*-
"""P5.2 —— dump 指定脚本含指定串的组(含 M-串载荷)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt, yscm_names

ypf = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf"
script_id = int(sys.argv[1]) if len(sys.argv) > 1 else 22
needle = (sys.argv[2] if len(sys.argv) > 2 else "CGTSS").encode()

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


def decode_expr(content, off, ln):
    out = []
    p = off
    end = off + ln
    while p < end:
        op = content[p]
        ilen = struct.unpack_from("<H", content, p + 1)[0]
        operand = content[p + 3:p + 3 + ilen]
        p += 3 + ilen
        if op == 0x4d:
            s = operand.decode("cp932", "replace")
            out.append(f"STR{s!r}")
        elif op in (0x48, 0x56, 0x76) and ilen == 3:
            pre = chr(operand[0])
            vid = struct.unpack_from("<H", operand, 1)[0]
            kind = {0x48: "var", 0x56: "ref", 0x76: "varidx"}[op]
            out.append(f"{kind}({pre}{vid})")
        elif op == 0x42 and ilen == 1:
            out.append(f"push({operand[0]})")
        elif op == 0x57 and ilen == 2:
            out.append(f"push({struct.unpack_from('<h', operand)[0]})")
        elif op == 0x49 and ilen == 4:
            out.append(f"push({struct.unpack_from('<i', operand)[0]})")
        elif op == 0x4c and ilen == 8:
            out.append(f"push({struct.unpack_from('<q', operand)[0]})")
        elif op == 0x29:
            out.append("aload")
        else:
            out.append(f"op{op:#04x}/{ilen}")
    return " ".join(out)


rec_off = 0
for gi in range(g):
    v = part1[gi]
    cmd = v & 0xFF
    cnt = (v >> 8) & 0xFF
    ro = rec_off
    rec_off += cnt * 12
    texts = []
    hit = False
    for wi in range(cnt):
        base = 0x20 + 4 * g + ro + wi * 12
        tag, ln, off = struct.unpack_from("<III", b, base)
        seg = content[off:off + ln]
        if needle in seg:
            hit = True
            b0 = tag & 0xFF
            texts.append(f"    w{wi}: B0={b0} {decode_expr(content, off, min(ln, 200))}")
    if hit:
        nm = names[cmd] if cmd < len(names) else "?"
        print(f"g{gi}: 0x{cmd:02x} {nm} windows={cnt}")
        for t in texts:
            print(t)
