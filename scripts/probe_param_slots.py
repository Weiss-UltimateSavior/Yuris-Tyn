#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
按命令统计「参数槽(B0)」使用分布。

用途：VARINFO/VARACT 这类多参数命令有 22/29 个参数，但语料往往只用其中几个。
      按铁律 2（逐条断言、按真实重要度排序），先实现语料真正用到的槽位。

用法：
    python3 scripts/probe_param_slots.py "path/to/bn.ypf" 0x67 0x66
"""

import json
import struct
import sys
import zlib
from collections import Counter, defaultdict

NAME_KEY = 0xC9
KEY = bytes.fromhex("2b904f93")
SLOT = 12
HDR = 0x20


def xor_region(b, key, start, length):
    for i in range(length):
        b[start + i] ^= key[i & 3]


def parse_ypf(data):
    magic, ver, cnt, data0 = struct.unpack_from("<4sIII", data, 0)
    assert magic == b"YPF\0"
    p = 0x24
    out = []
    for _ in range(cnt):
        nb = bytearray()
        while data[p] != 0:
            nb.append(data[p] ^ NAME_KEY)
            p += 1
        p += 1
        flags = data[p]
        p += 1
        uncomp, comp, off, _r = struct.unpack_from("<IIII", data, p)
        p += 16 + 8
        out.append((nb.decode("ascii", "replace"), flags, uncomp, comp, off))
    return out


def read(data, e):
    name, flags, uncomp, comp, off = e
    raw = data[off: off + comp]
    return zlib.decompress(raw) if flags == 1 and comp else raw


def groups(blob):
    if len(blob) < HDR:
        return None
    magic, ver, G, p1, cl, ct, p4, _u = struct.unpack_from("<4sIIIIIII", blob, 0)
    if magic != b"YSTB":
        return None
    b = bytearray(blob)
    xor_region(b, KEY, HDR, p1)
    xor_region(b, KEY, HDR + p1, cl)
    xor_region(b, KEY, HDR + p1 + cl, ct)
    xor_region(b, KEY, HDR + p1 + cl + ct, p4)
    cs = HDR + p1
    xs = cs + cl
    part1 = b[HDR:cs]
    cmds = b[cs:xs]
    out = []
    ptr = 0
    for i in range(G):
        w = struct.unpack_from("<I", part1, i * 4)[0]
        gtype = w & 0xFF
        cnt = (w >> 8) & 0xFF
        gparam = (w >> 16) & 0xFFFF
        wins = []
        for j in range(cnt):
            o = ptr + j * SLOT
            tag, ln, off = struct.unpack_from("<III", cmds, o)
            wins.append((tag, ln, off))
        ptr += cnt * SLOT
        out.append((gtype, cnt, gparam, wins))
    return out


def main():
    path = sys.argv[1]
    wanted = [int(x, 0) for x in sys.argv[2:]] or [0x66, 0x67]
    data = open(path, "rb").read()
    entries = parse_ypf(data)

    names = {}
    try:
        j = json.load(open("docs/opcode/yscm-commands-v555.json"))
        for i, c in enumerate(j["commands"]):
            names[i] = c["name"]
    except Exception:
        pass

    slot_hist = defaultdict(Counter)
    b2_hist = defaultdict(Counter)
    group_count = Counter()
    win_count = Counter()

    for e in entries:
        name = e[0]
        if not name.endswith(".ybn"):
            continue
        blob = read(data, e)
        g = groups(blob)
        if g is None:
            continue
        for gtype, cnt, gparam, wins in g:
            if gtype not in wanted:
                continue
            group_count[gtype] += 1
            win_count[gtype] += len(wins)
            for tag, ln, off in wins:
                b0 = tag & 0xFF
                b1 = (tag >> 8) & 0xFF
                b2 = (tag >> 16) & 0xFF
                b3 = (tag >> 24) & 0xFF
                slot_hist[gtype][b0] += 1
                b2_hist[gtype][b2] += 1

    for cmd in wanted:
        params = {}
        try:
            j = json.load(open("docs/opcode/yscm-commands-v555.json"))
            params = {i: p[0] for i, p in enumerate(j["commands"][cmd]["params"])}
        except Exception:
            pass
        print("=" * 62)
        print("cmd 0x%02x %s  组=%d  窗口=%d" %
              (cmd, names.get(cmd, "?"), group_count[cmd], win_count[cmd]))
        print("  参数槽(B0) 使用分布：")
        for slot, n in slot_hist[cmd].most_common():
            print("    [%2d] %-10s %d" % (slot, params.get(slot, "?"), n))
        print("  B2(值类型) 分布：", dict(b2_hist[cmd]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
