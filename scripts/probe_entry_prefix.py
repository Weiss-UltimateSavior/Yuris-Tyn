# -*- coding: utf-8 -*-
"""P6 修正侦查 —— 条目名字段的前导/尾缀字节结构定性。

事实链:
- bn 型(name NUL flag uncomp...):名字后无尾缀,flag∈{0,1}
- se 型(name? NUL uncomp...):名字尾有类型码(raw 02/06,XOR 后 cb/cf),
  且部分条目名字段有 1 字节前导(cg=`"`、se=\x17、sysvo=`/`、
  update1-ogg=`(`/`+`/`*` —— 同包内取值不一;txt 条目无前导)
本探针:逐包 dump (前导字节, 名字, 尾缀字节),找前导字节的结构规律。
"""
import struct
import sys
import os

PAC = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac"
KEY = 0xC9


def rcs(d, dp):
    out = bytearray()
    while d[dp] != 0:
        out.append(d[dp] ^ KEY)
        dp += 1
    return bytes(out), dp + 1


def walk(fn, limit=12):
    with open(os.path.join(PAC, fn), "rb") as f:
        head = f.read(0x24)
        ver, count, first = struct.unpack_from("<III", head, 4)
        d = f.read(first - 0x24)
    p = 0
    print(f"\n=== {fn} (count={count}) ===")
    for i in range(min(count, limit)):
        start = p
        name, p = rcs(d, p)
        # 候选 bn:flag @ p;候选 se:uncomp @ p
        b0 = d[p]
        bn_flag = b0
        bn_uncomp = struct.unpack_from("<I", d, p + 1)[0]
        se_uncomp = struct.unpack_from("<I", d, p)[0]
        is_bn = bn_flag in (0, 1)
        # 继续走:bn 消费 1(flag)+16+8;se 消费 16+8
        if is_bn:
            p += 1 + 16 + 8
        else:
            p += 16 + 8
        lead = name[:1]
        tail = name[-1:]
        core = name[1:-1] if len(name) > 1 else b""
        print("  #%-3d start=%#06x lead=%r core=%r tail=%r byte@NUL=%#04x "
              "bn_uncomp=%d se_uncomp=%d"
              % (i, start, lead, core, tail, b0, bn_uncomp, se_uncomp))


for fn in ["cg.ypf", "se.ypf", "update1.ypf", "sysvo.ypf", "bgm.ypf", "bn.ypf", "sc.ypf"]:
    walk(fn)
