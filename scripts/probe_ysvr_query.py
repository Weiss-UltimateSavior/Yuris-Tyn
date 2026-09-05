# -*- coding: utf-8 -*-
"""P5 —— YSVR 条目查询:按 var_id 打印 kind/ty/bounds/init。"""
import struct
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
ysv = read(d, e[[k for k in e if k.endswith("ysv.ybn")][0]])

magic, ver, cnt = struct.unpack_from("<4sIH", ysv, 0)
print("YSVR ver=%d entries=%d" % (ver, cnt))
p = 10
want = set(int(x) for x in sys.argv[1:]) if len(sys.argv) > 1 else {2729, 2459, 1171, 1202}
hits = 0
for i in range(cnt):
    kind, cat = ysv[p], ysv[p + 1]
    script, var_id = struct.unpack_from("<HH", ysv, p + 2)
    ty, dims = ysv[p + 6], ysv[p + 7]
    q = p + 8
    bounds = []
    for _ in range(dims):
        bounds.append(struct.unpack_from("<I", ysv, q)[0])
        q += 4
    init = None
    if ty == 1:
        init = struct.unpack_from("<q", ysv, q)[0]
        q += 8
    elif ty == 2:
        init = struct.unpack_from("<d", ysv, q)[0]
        q += 8
    elif ty == 3:
        ln = struct.unpack_from("<H", ysv, q)[0]
        init = ysv[q + 2:q + 2 + ln]
        q += 2 + ln
    if var_id in want:
        hits += 1
        print("entry#%d kind=%d cat=%d script=%d var=%d ty=%d bounds=%s init=%r"
              % (i, kind, cat, script, var_id, ty, bounds, init))
    p = q
print("hits:", hits, "(consumed to", p, "of", len(ysv), ")")
