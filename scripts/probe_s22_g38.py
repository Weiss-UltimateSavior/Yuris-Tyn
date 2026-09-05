# -*- coding: utf-8 -*-
"""P5 —— s22 g38 IF(@60[1]) 分歧侦查:解码 GOSUB 标签串 + 窗口原始字节。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt, yscm_names

ypf = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf"
sid = int(sys.argv[1]) if len(sys.argv) > 1 else 22
lo = int(sys.argv[2]) if len(sys.argv) > 2 else 36
hi = int(sys.argv[3]) if len(sys.argv) > 3 else 40

d = open(ypf, "rb").read()
entries = parse_ypf(d)
yscm = read(d, entries["%ysbin\\ysc.ybn"])
names = yscm_names(yscm)
target = next(e for k, e in entries.items() if k.endswith("\\yst%05d.ybn" % sid))
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
    ro = rec_off
    rec_off += cnt * 12
    if not (lo <= gi < hi):
        continue
    print(f"g{gi}: 0x{cmd:02x} {names[cmd] if cmd < len(names) else '?'} windows={cnt}")
    for wi in range(cnt):
        base = 0x20 + 4 * g + ro + wi * 12
        tag, ln, off = struct.unpack_from("<III", b, base)
        raw = content[off:off + ln]
        hexs = raw.hex(" ")
        # 解码 0x4d 串字面量(界定符语义,成果 53)
        strs = []
        p = 0
        while p + 3 <= len(raw):
            op = raw[p]
            ilen = struct.unpack_from("<H", raw, p + 1)[0]
            operand = raw[p + 3:p + 3 + ilen]
            if op == 0x4d and operand:
                delim = operand[0]
                body = bytearray()
                for ch in operand[1:]:
                    if ch == delim:
                        break
                    body.append(ch)
                strs.append(body.decode("sjis", "replace"))
            p += 3 + ilen
        print(f"    w{wi}: B0={tag & 0xFF} B2={(tag >> 16) & 0xFF} "
              f"B3={(tag >> 24) & 0xFF} len={ln} raw={hexs}"
              + (f"  str={strs}" if strs else ""))
