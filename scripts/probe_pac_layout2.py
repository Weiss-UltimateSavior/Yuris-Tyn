# -*- coding: utf-8 -*-
"""P6.2 —— se/cg 系包条目布局假说验证:dump 候选 offset 处数据头。

假说(se 系):Entry = prefix(1) + name + suffix(1) + NUL + uncomp(4) +
comp(4) + off(4) + zero(4) + tail(8) —— 与 bn 的区别在 flag 字段位置
(bn: NUL 后立即 flag;se 系: 无独立 flag,suffix 承载?)。
验证:按两种模板解析,dump 各自 off 处 16B,看 OggS/PNG/zlib 头。
"""
import struct
import sys
import zlib

PAC = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac"
KEY = 0xC9


def rcs(d, dp):
    out = bytearray()
    while d[dp] != 0:
        out.append(d[dp] ^ KEY)
        dp += 1
    return bytes(out), dp + 1


def parse_bntmpl(d, count):
    """bn 模板:name NUL flag uncomp comp off zero tail8。"""
    p = 0
    out = []
    for _ in range(count):
        name, p = rcs(d, p)
        flag = d[p]
        uncomp, comp, off, zero = struct.unpack_from("<IIII", d, p + 1)
        p += 1 + 16 + 8
        out.append((name, flag, uncomp, comp, off))
    return out


def parse_setmpl(d, count):
    """se 模板:name(含首尾标记) NUL uncomp comp off zero tail8。"""
    p = 0
    out = []
    for _ in range(count):
        name, p = rcs(d, p)
        uncomp, comp, off, zero = struct.unpack_from("<IIII", d, p)
        p += 16 + 8
        out.append((name, None, uncomp, comp, off))
    return out


def show(fn, n=4):
    with open(PAC + "\\" + fn, "rb") as f:
        head = f.read(0x24)
        ver, count, first = struct.unpack_from("<III", head, 4)
        f.seek(0x24)
        d = f.read(first - 0x24)
        print(f"\n=== {fn} (count={count}) ===")
        for label, parser in [("bn模板(flag@NUL后)", parse_bntmpl),
                              ("se模板(无flag)", parse_setmpl)]:
            try:
                entries = parser(d, count)
            except (IndexError, struct.error) as ex:
                print(f"  [{label}] 解析失败: {ex}")
                continue
            ok = 0
            for (name, flag, uncomp, comp, off) in entries[:n]:
                f.seek(off)
                raw = f.read(16)
                tag = raw[:4]
                print(f"  [{label}] {name.decode('cp932','replace')!r}"
                      f" flag={flag} uncomp={uncomp} comp={comp} off={off:#x}"
                      f" head={raw.hex(' ')}")
                if raw[:4] == b"OggS" or raw[:1] == b"\x78" or raw[:3] == b"\x89P":
                    ok += 1
            print(f"  [{label}] 前{n}条中合理头:{ok}")


for fn in ["se.ypf", "cg.ypf", "bgm.ypf", "sysvo.ypf", "bn.ypf", "update1.ypf"]:
    show(fn)
