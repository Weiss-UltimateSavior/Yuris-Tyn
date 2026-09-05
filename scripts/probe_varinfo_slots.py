#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""全语料 VARINFO(0x67)/VARACT(0x66) 组形态探针:槽组合分布 + B0=0 组窗口内容.

目的(2026-09-03 勘误验证):
  - YSCM SET(0)/LET(1) = 目标变量引用槽(kind 2 延迟求值),非写回操作
  - 统计 B0=0 组是否伴随操作槽(a2+ 非零)或为纯 fallback(TYPE/LENGTH 显示)

用法: python3 scripts/probe_varinfo_slots.py <bn.ypf>
"""
import struct
import sys
import zlib
import collections

KEY = bytes.fromhex("2b904f93")
NAME_XOR = 0xC9

VARINFO_PARAMS = [
    "SET", "LET", "TYPE", "STRTYPE", "DIMNUM", "DIMSIZE", "DIMSIZE2", "DIMSIZE3",
    "DIMSIZE4", "DIMSIZE5", "DIMSIZE6", "DIMSIZE7", "DIMSIZE8", "LENGTH",
    "SEARCH", "STRFIRST", "SJISCODE", "INT", "FLT", "STR", "NO", "NO2",
]
VARACT_PARAMS = [
    "SET", "LET", "CUT", "COPY", "POS", "LENGTH", "TYPE", "UPPER", "UPPER2",
    "LOWER", "LOWER2", "HANTOZEN", "ZENTOHAN", "DIMSIZE", "PUSH", "POP",
    "INIT", "G_INT", "G_FLT", "G_STR", "G_INT2", "G_FLT2", "G_STR2",
    "G_INT3", "G_FLT3", "G_STR3", "G_INT4", "G_FLT4", "G_STR4",
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
    slot_combo = collections.Counter()
    b0_only_examples = []
    cmd_hist = collections.Counter()

    for ent in parse_ypf(data):
        if not ent[0].startswith("$ysbin\\yst0"):
            continue
        name, flag, unc, comp, doff = ent
        raw = data[doff:doff + comp]
        buf = zlib.decompress(raw) if flag == 1 else raw
        g, p1len, clen, ctlen, p4len = struct.unpack_from("<IIIII", buf, 8)
        reg = bytearray(buf[0x20:])
        pos = 0
        for ln in (p1len, clen, ctlen, p4len):
            for i in range(ln):
                reg[pos + i] ^= KEY[i & 3]
            pos += ln
        p1 = bytes(reg[0:0 + p1len])
        cmds = bytes(reg[p1len:p1len + clen])
        content = bytes(reg[p1len + clen:p1len + clen + ctlen])
        part4 = bytes(reg[p1len + clen + ctlen:p1len + clen + ctlen + p4len])
        pool = content + part4
        first_slot = 0
        for gi in range(g):
            ctype, ccount, gparam = struct.unpack_from("<BBH", p1, gi * 4)
            if ctype in (0x66, 0x67):
                cmd_hist[ctype] += 1
                wins = []
                for w in range(ccount):
                    tag, wlen, woff = struct.unpack_from("<III", cmds, (first_slot + w) * 12)
                    b0 = tag & 0xFF
                    seg = pool[woff:woff + wlen]
                    wins.append((b0, tag >> 16, wlen, woff, seg))
                slots = sorted(b for b, *_ in wins)
                slot_combo[(ctype, tuple(slots))] += 1
                # B0=0 且无其他操作槽(除 SET/LET)的组 → fallback 路径候选
                ops = [b for b in slots if b >= 2]
                if 0 in slots and not ops and len(b0_only_examples) < 6:
                    b0_only_examples.append((name, gi, ctype, wins))
            first_slot += ccount

    print("[*] 组数:", dict(cmd_hist))
    print("\n[*] 槽组合分布 (cmd, slots) -> 组数:")
    for (c, slots), n in sorted(slot_combo.items(), key=lambda kv: -kv[1]):
        names = VARINFO_PARAMS if c == 0x67 else VARACT_PARAMS
        pretty = ",".join(names[s] if s < len(names) else str(s) for s in slots)
        print(f"  0x{c:02x} [{pretty}] x{n}")
    print("\n[*] B0=0 且无操作槽的组样例(fallback 候选):")
    for name, gi, c, wins in b0_only_examples:
        print(f"  {name} g{gi} cmd=0x{c:02x}")
        for b0, hi, wlen, woff, seg in wins:
            print(f"    B0={b0} hi=0x{hi:04x} len={wlen} off={woff} seg={seg[:24].hex()}")


if __name__ == "__main__":
    main()
