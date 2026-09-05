# -*- coding: utf-8 -*-
"""P5.2 —— YSVR 中 @1176 声明/初值(直接借 probe_variables 解析逻辑)。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
yv_key = [k for k in e if k.endswith("ysv.ybn")][0]
ysv = read(d, e[yv_key])
assert ysv[:4] == b"YSVR"
cnt = struct.unpack_from("<H", ysv, 8)[0]
p = 10
prod = lambda t: __import__("math").prod(t) if t else 1
found = 0
for _ in range(cnt):
    kind, cls = ysv[p], ysv[p + 1]
    sid = struct.unpack_from("<H", ysv, p + 2)[0]
    vid = struct.unpack_from("<H", ysv, p + 4)[0]
    ty, dim = ysv[p + 6], ysv[p + 7]
    dims = struct.unpack_from("<%dI" % dim, ysv, p + 8) if dim else ()
    p += 8 + 4 * dim
    if vid == 1176:
        print("YSVR @1176: kind=%d sid=%d type=%d dim=%d dims=%s" % (kind, sid, ty, dim, dims))
        if ty == 1:
            n = prod(dims) if dim else 1
            vals = struct.unpack_from("<%dq" % n, ysv, p)
            print("  init:", list(vals[:8]), "..." if n > 8 else "")
            p += 8 * n
        elif ty == 2:
            n = prod(dims) if dim else 1
            vals = struct.unpack_from("<%dd" % n, ysv, p)
            print("  init:", list(vals[:8]), "..." if n > 8 else "")
            p += 8 * n
        elif ty == 3:
            n = prod(dims) if dim else 1
            for k in range(n):
                slen = struct.unpack_from("<H", ysv, p)[0]
                s = ysv[p + 2:p + 2 + slen]
                if k < 3:
                    print("  init[%d]:" % k, s[:20])
                p += 2 + slen
        found += 1
        if found >= 1:
            break
    else:
        if ty == 0:
            continue
        if ty == 3:
            n = prod(dims) if dim else 1
            for _ in range(n):
                slen = struct.unpack_from("<H", ysv, p)[0]
                p += 2 + slen
        else:
            n = prod(dims) if dim else 1
            p += 8 * n
print("found:", found)
