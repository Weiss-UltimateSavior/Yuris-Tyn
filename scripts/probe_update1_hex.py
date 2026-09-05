# -*- coding: utf-8 -*-
"""P6.2 —— update1 索引区 hexdump:定位 ogg/png 条目与 txt 条目的布局差异。"""
import struct

PAC = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\update1.ypf"
KEY = 0xC9


def rcs(d, dp):
    out = bytearray()
    while d[dp] != 0:
        out.append(d[dp] ^ KEY)
        dp += 1
    return bytes(out), dp + 1


with open(PAC, "rb") as f:
    head = f.read(0x24)
    ver, count, first = struct.unpack_from("<III", head, 4)
    f.seek(0x24)
    d = f.read(first - 0x24)
    print(f"count={count} first={first:#x}")

    # 找 txt 条目(名字含 .txt)
    p = 0
    hits = []
    names_bn = []
    for i in range(count):
        start = p
        name, p = rcs(d, p)
        flag = d[p]
        uncomp, comp, off, _z = struct.unpack_from("<IIII", d, p + 1)
        p += 1 + 16 + 8
        names_bn.append((start, name, flag, uncomp, comp, off))
        if b".txt" in name:
            hits.append((i, start, name, flag, uncomp, comp, off))

    print("txt 条目(bn 模板读):")
    for (i, start, name, flag, uncomp, comp, off) in hits[:4]:
        print(f"  #{i} start={start:#x} name={name.decode('cp932','replace')!r}"
              f" flag={flag} uncomp={uncomp} comp={comp} off={off:#x}")
        seg = d[start:start + 96]
        for j in range(0, min(len(seg), 96), 16):
            row = seg[j:j + 16]
            asc = "".join(chr(c ^ KEY) if 32 <= (c ^ KEY) < 127 else "." for c in row)
            print("    %04x: %-47s  %s" % (start + j, row.hex(" "), asc))
        # dump 数据 off 处
        f.seek(off)
        raw = f.read(24)
        print(f"    data@{off:#x}: {raw.hex(' ')}")

    print("\nogg 条目对照(bn 模板读):")
    for (i, start, name, flag, uncomp, comp, off) in names_bn[:2]:
        print(f"  #{i} start={start:#x} name={name.decode('cp932','replace')!r}"
              f" flag={flag} uncomp={uncomp} comp={comp} off={off:#x}")
        seg = d[start:start + 96]
        for j in range(0, min(len(seg), 96), 16):
            row = seg[j:j + 16]
            asc = "".join(chr(c ^ KEY) if 32 <= (c ^ KEY) < 127 else "." for c in row)
            print("    %04x: %-47s  %s" % (start + j, row.hex(" "), asc))
