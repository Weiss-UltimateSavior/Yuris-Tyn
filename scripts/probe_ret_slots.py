# -*- coding: utf-8 -*-
"""P5.2 —— 全语料:RETURN 窗 B0 分布 + @60/@61/$62 读者下标分布 + @53 写者槽号。"""
import struct
import sys
from collections import Counter

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)

ret_b0 = Counter()          # RETURN 组窗口 B0 分布
ret_slot1_windows = 0       # RETURN 单窗(B0=0)计数
ret_multi = 0               # 多窗 RETURN 计数
reader60 = Counter()        # @60 读者下标(aload 前 push 常量)
reader61 = Counter()
reader62 = Counter()


def scan_expr(content, off, ln, readers):
    """粗扫:ref(@60/@61/$62) 后最近的 push 常量 + aload。"""
    p = off
    end = off + ln
    last_ref = None
    pending = []
    while p + 3 <= end:
        op = content[p]
        ilen = struct.unpack_from("<H", content, p + 1)[0]
        operand = content[p + 3:p + 3 + ilen]
        p += 3 + ilen
        if op in (0x48, 0x56, 0x76) and ilen == 3 and operand[0] == 0x40:
            vid = struct.unpack_from("<H", operand, 1)[0]
            last_ref = vid
            pending = []
        elif op == 0x42 and ilen == 1 and last_ref is not None:
            pending.append(operand[0])
        elif op == 0x57 and ilen == 2 and last_ref is not None:
            pending.append(struct.unpack_from("<h", operand)[0])
        elif op == 0x29 and last_ref is not None and pending:
            if last_ref in readers:
                readers[last_ref][tuple(pending)] += 1
            last_ref = None
            pending = []
        elif op in (0x2b, 0x2d, 0x2a, 0x2f, 0x3d, 0x21, 0x3e, 0x3c):
            pass  # 二元运算打断跟踪
        if p >= end:
            break


for name, ent in sorted(e.items()):
    if not name.endswith(".ybn") or not name.startswith("$ysbin\\yst0"):
        continue
    try:
        b = decrypt(read(d, ent))
    except Exception:
        continue
    magic, ver, g = struct.unpack_from("<4sII", b, 0)
    if magic != b"YSTB":
        continue
    part1 = struct.unpack_from("<%dI" % g, b, 0x20)
    tw = sum((v >> 8) & 0xFF for v in part1)
    content = b[0x20 + 4 * g + tw * 12:]
    ro = 0
    for gi in range(g):
        v = part1[gi]
        cmd = v & 0xFF
        cnt = (v >> 8) & 0xFF
        if cmd == 0x4F:  # RETURN
            ret_multi += cnt > 1
            for wi in range(cnt):
                base = 0x20 + 4 * g + ro + wi * 12
                tag, ln, off = struct.unpack_from("<III", b, base)
                b0 = tag & 0xFF
                ret_b0[b0] += 1
                if b0 == 0 and cnt == 1:
                    ret_slot1_windows += 1
        elif cmd in (0x35, 0x32, 0x33, 0x34, 0x5c, 0x2c, 0x67, 0x66, 0x2b, 0x4f, 0x0d, 0x0e, 0x37):
            # 常见含表达式窗的命令:扫 @60/@61/$62 读者
            for wi in range(cnt):
                base = 0x20 + 4 * g + ro + wi * 12
                tag, ln, off = struct.unpack_from("<III", b, base)
                if ln:
                    scan_expr(content, off, ln, {
                        60: reader60, 61: reader61,
                    })
        ro += cnt * 12

print("RETURN windows B0 dist:", dict(sorted(ret_b0.items())))
print("single-window (B0=0) RETURN count:", ret_slot1_windows)
print("multi-window RETURN count:", ret_multi)
print("@60 reader indices:", dict(reader60.most_common(6)))
