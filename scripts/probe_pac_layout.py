# -*- coding: utf-8 -*-
"""P6.2 —— 索引区布局实证:各包头 128B hexdump + 相邻名字起始间距核对。

bn.ypf 模型(成果 0.2):Entry = len(name)+26 = name+NUL(1)+flag(1)+
16(uncomp/comp/off/zero)+tail(8);索引区头 4 字节未知(0x24 起)。
本轮:se.ypf/sysvo.ypf 名字出现尾残差 → 实测布局差异。
"""
import struct
import sys

PAC = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac"
KEY = 0xC9


def rcs(d, dp):
    out = bytearray()
    while d[dp] != 0:
        out.append(d[dp] ^ KEY)
        dp += 1
    return bytes(out), dp + 1


def dump(fn, n_show=3):
    with open(PAC + "\\" + fn, "rb") as f:
        head = f.read(0x24)
        magic, ver, count, first = struct.unpack_from("<4sIII", head, 0)
        if magic != b"YPF\x00":
            print(f"[{fn}] 非 YPF 跳过")
            return
        f.seek(0x24)
        d = f.read(min(first - 0x24, 0x10000))
        print(f"\n[{fn}] ver={ver} count={count} first={first:#x} 头4B={d[:4].hex()}")
        print("  索引区前 96B(hex):")
        for i in range(0, 96, 16):
            row = d[i:i + 16]
            asc = "".join(chr(c ^ KEY) if 32 <= (c ^ KEY) < 127 else "." for c in row)
            print("    %04x: %-47s  %s" % (i, row.hex(" "), asc))
        # 逐条目走 + 记录名字起始偏移
        p = 0
        starts = []
        names = []
        for i in range(min(count, 8)):
            starts.append(p)
            name, p = rcs(d, p)
            names.append(name)
            flag = d[p]
            uncomp, comp, off, _ = struct.unpack_from("<IIII", d, p + 1)
            p += 1 + 16 + 8
            print("    #%d off=%#x name=%r flag=%d uncomp=%d comp=%d dataoff=%#x"
                  % (i, starts[-1], name, flag, uncomp, comp, off))
        # 间距 vs len(name)+26
        for i in range(len(starts) - 1):
            gap = starts[i + 1] - starts[i]
            model = len(names[i]) + 26
            print("    间距[%d]=%d 模型 len+26=%d %s"
                  % (i, gap, model, "OK" if gap == model else "差 %d" % (gap - model)))


for fn in ["bn.ypf", "se.ypf", "sysvo.ypf", "cg.ypf", "bgm.ypf", "update1.ypf",
           "cgsys_ec.ypf", "sc.ypf"]:
    dump(fn)
