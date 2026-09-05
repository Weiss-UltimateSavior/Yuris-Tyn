# -*- coding: utf-8 -*-
"""P6.2(修正版)—— 按探测到的正确模板解析各包,全量统计:
尾缀(类型码)分布 + 数据头魔数分布 + 压缩判定(uncomp vs comp)。

两种已实证布局:
  bn 系: name + NUL + flag(1=zlib) + uncomp + comp + off + zero + tail8
  se 系: prefix(1) + name + suffix(1=类型码) + NUL + uncomp + comp + off + zero + tail8
模板选择:两条候选 off 处读 16B,头合法(zlib/PNG/OggS/文本)者胜。
"""
import collections
import os
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


def parse(d, count, tmpl):
    p = 0
    out = []
    for _ in range(count):
        name, p = rcs(d, p)
        if tmpl == "bn":
            flag = d[p]
            uncomp, comp, off, _z = struct.unpack_from("<IIII", d, p + 1)
            p += 1 + 16 + 8
        else:
            flag = None
            uncomp, comp, off, _z = struct.unpack_from("<IIII", d, p)
            p += 16 + 8
        out.append((name, flag, uncomp, comp, off))
    return out, p


def head_ok(raw):
    if not raw:
        return False
    if raw[:1] == b"\x78":
        return True  # zlib
    if raw[:4] == b"OggS":
        return True
    if raw[:8] == b"\x89PNG\r\n\x1a\n":
        return True
    if raw[:3] == b"\xef\xbb\xbf" or raw[:2] in (b"\r\n", b"//"):
        return True
    if all(32 <= c < 127 or c in (9, 10, 13) for c in raw[:8]):
        return True  # 文本
    return False


def detect(f, d, count, flen):
    for tmpl in ("bn", "se"):
        try:
            entries, _c = parse(d, count, tmpl)
        except (IndexError, struct.error):
            continue
        if not entries:
            continue
        ok = 0
        for (name, flag, uncomp, comp, off) in entries[:6]:
            if off + 8 > flen:
                continue
            f.seek(off)
            raw = f.read(8)
            if head_ok(raw):
                ok += 1
        if ok >= 3:
            return tmpl, entries
    return None, None


def main():
    for fn in sorted(os.listdir(PAC)):
        if not fn.endswith(".ypf"):
            continue
        path = os.path.join(PAC, fn)
        flen = os.path.getsize(path)
        with open(path, "rb") as f:
            head = f.read(0x24)
            if head[:4] != b"YPF\x00":
                continue
            ver, count, first = struct.unpack_from("<III", head, 4)
            f.seek(0x24)
            d = f.read(first - 0x24)
            tmpl, entries = detect(f, d, count, flen)
            if tmpl is None:
                print(f"\n[{fn}] 模板探测失败!")
                continue
            tails = collections.Counter()
            magics = collections.Counter()
            comp_eq = 0
            zlibish = 0
            for (name, flag, uncomp, comp, off) in entries:
                if tmpl == "bn":
                    tails[flag] += 1
                else:
                    tails[name[-1] if name else 0] += 1
                if uncomp == comp:
                    comp_eq += 1
                f.seek(off)
                raw = f.read(4)
                if raw[:1] == b"\x78":
                    zlibish += 1
                    tag = "zlib"
                elif raw[:4] == b"OggS":
                    tag = "OGG"
                elif raw[:8] == b"\x89PNG\r\n\x1a\n":
                    tag = "PNG"
                else:
                    tag = "raw:" + raw[:4].hex()
                magics[tag] += 1
            print(f"\n[{fn}] 模板={tmpl} 条目={len(entries)}"
                  f" uncomp==comp:{comp_eq} zlib头:{zlibish}")
            print(f"  尾缀/flag 分布: {dict(tails.most_common(6))}")
            print(f"  魔数分布: {dict(magics.most_common(6))}")


if __name__ == "__main__":
    main()
