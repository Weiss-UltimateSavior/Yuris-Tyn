#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""INT(0x32)/FLT(0x19)/STR(0x5c) 声明组窗口形态统计(标量/数组/其他).

用法: python3 scripts/probe_decl_forms.py <bn.ypf>
"""
import collections
import struct
import sys
import zlib

KEY = bytes.fromhex("2b904f93")
NAME_XOR = 0xC9
DECL_CMDS = {0x19: "FLT", 0x32: "INT", 0x5C: "STR"}


def parse_ypf(data):
    _v, count, first = struct.unpack_from("<III", data, 4)
    off = 0x24
    entries = []
    for _ in range(count):
        end = data.index(b"\0", off)
        name = bytes(b ^ NAME_XOR for b in data[off:end]).decode("cp932")
        off = end + 1
        flag, uncomp, comp, doff, _r = struct.unpack_from("<BIIII", data, off)
        off += 17 + min(8, first - off)
        entries.append((name, flag, uncomp, comp, doff))
    return entries


def main():
    data = open(sys.argv[1], "rb").read()
    forms = collections.Counter()
    examples = {}
    for name, flag, unc, comp, doff in parse_ypf(data):
        if not name.startswith("$ysbin\\yst0"):
            continue
        raw = data[doff:doff + comp]
        buf = zlib.decompress(raw) if flag == 1 else raw
        g, p1len, clen, ctlen, p4len = struct.unpack_from("<IIIII", buf, 8)
        reg = bytearray(buf[0x20:])
        pos = 0
        for ln in (p1len, clen, ctlen, p4len):
            for i in range(ln):
                reg[pos + i] ^= KEY[i & 3]
            pos += ln
        p1 = bytes(reg[0:p1len])
        cmds = bytes(reg[p1len:p1len + clen])
        pool = bytes(reg[p1len + clen:])
        fs = 0
        for gi in range(g):
            ct, cc, gp = struct.unpack_from("<BBH", p1, gi * 4)
            if ct in DECL_CMDS and cc >= 1:
                tag, wlen, woff = struct.unpack_from("<III", cmds, fs * 12)
                seg = pool[woff:woff + wlen]
                # 形态判定:首字节 48/56=直接引用;76..29=数组;4d=串
                if seg[:1] in (b"H", b"V"):
                    kind = "scalar"
                elif seg[:1] == b"v" or (b"\x29\x01\x00" in seg and seg[:1] == b"v"):
                    kind = "array76"
                elif seg[:1] in (b"v", b"V") and b"\x29\x01\x00" in seg:
                    kind = "array"
                else:
                    kind = f"other:{seg[:1].hex()}"
                forms[(DECL_CMDS[ct], cc, kind)] += 1
                if kind not in ("scalar",) and (DECL_CMDS[ct], kind) not in examples:
                    examples[(DECL_CMDS[ct], kind)] = (name, gi, cc, f"{tag:08x}", seg.hex())
            fs += cc
    print("声明组形态 (cmd, 窗数, kind) -> 组数:")
    for k, n in sorted(forms.items()):
        print(f"  {k} x{n}")
    print("\n非标量样例:")
    for k, v in examples.items():
        print(f"  {k} = {v}")


if __name__ == "__main__":
    main()
