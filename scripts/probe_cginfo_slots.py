#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""全语料 CGINFO(0x04) 槽组合分布探针.

用法: python3 scripts/probe_cginfo_slots.py <bn.ypf>
"""
import collections
import struct
import sys
import zlib

KEY = bytes.fromhex("2b904f93")
NAME_XOR = 0xC9

# YSCM CGINFO 38 参(下标→名)
CGINFO_PARAMS = [
    "ID", "IDNO", "EXIST", "E", "A", "X", "Y", "Z", "MFILE", "T", "TID", "TA",
    "MODE", "SX", "SY", "FBX", "FBY", "FEX", "FEY", "FEZ", "TEX", "ONMOUSE",
    "ONMOUSE2", "ONMOUSE3", "COLOR", "TRIM", "LINT", "LINT2", "SEARCH", "NUM",
    "NAME", "LSET", "NO", "LET", "SET", "SET2", "",
]


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
    combo = collections.Counter()
    examples = []
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
            if ct == 0x04:
                slots = []
                for w in range(cc):
                    tag, wlen, woff = struct.unpack_from("<III", cmds, (fs + w) * 12)
                    slots.append(tag & 0xFF)
                key = tuple(slots)
                combo[key] += 1
                if len(examples) < 5:
                    examples.append((name, gi, key))
            fs += cc
    print("[*] CGINFO(0x04) 槽组合分布:")
    for slots, n in sorted(combo.items(), key=lambda kv: -kv[1]):
        pretty = ",".join(
            CGINFO_PARAMS[s] if s < len(CGINFO_PARAMS) else str(s) for s in slots
        )
        print(f"  [{pretty}] x{n}")
    print("\n样例:", examples)


if __name__ == "__main__":
    main()
