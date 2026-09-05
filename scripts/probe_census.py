# -*- coding: utf-8 -*-
"""P6 修正 —— 全包条目普查:布局判定(复合判别)+ 类型码/虚拟根 census。

复合判别:
1. 尾缀(XOR 后)∈ {0xcb=PNG, 0xcf=OGG} → se
2. byte@NUL ∈ {0,1} 且 bn 解释的 offset 落界 → bn
3. 否则按 se 校验,失败报错
"""
import collections
import os
import struct

PAC = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac"
KEY = 0xC9
SE_CODES = {0xCB: "PNG", 0xCF: "OGG"}


def rcs(d, dp):
    out = bytearray()
    while d[dp] != 0:
        out.append(d[dp] ^ KEY)
        dp += 1
    return bytes(out), dp + 1


def walk(fn):
    path = os.path.join(PAC, fn)
    flen = os.path.getsize(path)
    with open(path, "rb") as f:
        head = f.read(0x24)
        ver, count, first = struct.unpack_from("<III", head, 4)
        d = f.read(first - 0x24)
    p = 0
    stats = {"bn": 0, "se": 0}
    tails = collections.Counter()
    roots = collections.Counter()
    odd = []
    for i in range(count):
        name, np = rcs(d, p)
        b0 = d[np]
        # 判别
        is_se = None
        if name and name[-1] in SE_CODES:
            is_se = True
        elif b0 in (0, 1):
            # bn 校验:off@np+10(1 flag + 4 uncomp + 4 comp) —— 等等:
            # bn:flag@np, uncomp@np+1, comp@np+5, off@np+9
            off_a = struct.unpack_from("<I", d, np + 9)[0]
            comp_a = struct.unpack_from("<I", d, np + 5)[0]
            if first <= off_a and off_a + comp_a <= flen:
                is_se = False
            else:
                is_se = True
        else:
            is_se = True
        if is_se:
            stats["se"] += 1
            uncomp, comp, off, _z = struct.unpack_from("<IIII", d, np)
            p = np + 16 + 8
            tails[name[-1] if name else None] += 1
            roots[name[:1] if name else None] += 1
            if (not name or len(name) < 3) and len(odd) < 8:
                odd.append((i, name, uncomp, comp, off))
        else:
            stats["bn"] += 1
            flag = b0
            uncomp, comp, off, _z = struct.unpack_from("<IIII", d, np + 1)
            p = np + 1 + 16 + 8
            tails["bn:" + str(flag)] += 1
            roots[name[:1] if name else None] += 1
    print(f"[{fn}] bn={stats['bn']} se={stats['se']}")
    print("  tails:", dict(tails.most_common(8)))
    print("  roots:", dict(sorted(roots.items(), key=lambda kv: -kv[1])[:10]))
    if odd:
        print("  odd(root-only?) entries:", odd[:8])
    return stats


total = {}
for fn in sorted(os.listdir(PAC)):
    if fn.endswith(".ypf"):
        try:
            walk(fn)
        except Exception as ex:
            print(f"[{fn}] FAIL {ex}")
