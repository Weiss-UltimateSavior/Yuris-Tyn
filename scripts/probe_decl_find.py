#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""全语料搜索声明类命令组中引用指定变量 id 的窗口.

用法: python3 scripts/probe_decl_find.py <bn.ypf> <varid_hex不含0x前缀,如 1894>
"""
import struct
import sys
import zlib

KEY = bytes.fromhex("2b904f93")
NAME_XOR = 0xC9

DECL = {
    0x10, 0x11, 0x12, 0x13,                     # F_* 族
    0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23,
    0x24, 0x25, 0x26, 0x27, 0x28, 0x29,         # G_* 族
    0x32, 0x33, 0x34, 0x35,                     # INT/FLT/STR/LET 单声明
    0x52, 0x53, 0x54, 0x55,                     # S_* 族
}


def parse_ypf(data):
    _v, count, first = struct.unpack_from("<III", data, 4)
    off = 0x24
    entries = []
    for _ in range(count):
        end = data.index(b"\0", off)
        name = bytes(b ^ NAME_XOR for b in data[off:end]).decode("cp932")
        off = end + 1
        vals = struct.unpack_from("<BIIII", data, off)
        off += 17 + min(8, first - off)
        entries.append((name, vals[0], vals[1], vals[2], vals[3]))
    return entries


def main():
    vid = bytes.fromhex(sys.argv[2])
    data = open(sys.argv[1], "rb").read()
    found = []
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
            if ct in DECL:
                for w in range(cc):
                    tag, wlen, woff = struct.unpack_from("<III", cmds, (fs + w) * 12)
                    seg = pool[woff:woff + wlen]
                    if vid in seg:
                        found.append((name, gi, ct, w, f"{tag:08x}", seg.hex()))
            fs += cc
    print(f"含 {vid.hex()} 的声明组窗口: {len(found)}")
    for f in found[:12]:
        print(" ", f)


if __name__ == "__main__":
    main()
