#!/usr/bin/env python3
"""P4 —— sc.ypf 明文剧本(scenario*.txt)命令清单与频次探针。

用法:
  python scripts/probe_scenario.py                     # 全文件清单 + 命令频次
  python scripts/probe_scenario.py <name>              # 单文件命令逐条 dump
  python scripts/probe_scenario.py <name> --raw        # 原文(解码 SJIS)
"""
import re
import struct
import sys
import zlib
from collections import Counter

NAME_KEY = 0xC9


def rcs(d, dp):
    out = bytearray()
    while d[dp] != 0:
        out.append(d[dp] ^ NAME_KEY)
        dp += 1
    return bytes(out), dp + 1


def parse_ypf(path):
    d = open(path, "rb").read()
    _, _, cnt, data0 = struct.unpack_from("<4sIII", d, 0)
    p = 0x24
    entries = {}
    for _ in range(cnt):
        name, p = rcs(d, p)
        flags = d[p]; p += 1
        uncomp, comp, off, _res = struct.unpack_from("<IIII", d, p); p += 16
        p += min(8, data0 - p)
        entries[name.decode("ascii", "replace")] = (flags, uncomp, comp, off)
    return d, entries


def read_entry(d, e):
    flags, uncomp, comp, off = e
    raw = d[off:off + comp]
    return zlib.decompress(raw) if flags == 1 else raw


TOKEN = re.compile(rb"\\([A-Za-z_][A-Za-z0-9_]*)(\()?")

# YSTB 侧已定性命令(跨系统对应候选,等级标注在报告阶段)
YSTB_KNOWN = {"BG", "CG", "CGACT", "CGEND", "CGINFO", "TEXT", "VO", "SE", "BGM",
              "WAIT", "LOAD", "SAVE", "QUAKE", "ANIME", "MASK", "ALLMASK",
              "QUAKEEND", "ANIMEEND", "BGMSTOP", "SESTOP", "VOSTOP"}


def scan(data):
    """返回 (命令频次 Counter, 带参命令 Counter, 总 token 数)"""
    c = Counter()
    witharg = Counter()
    for m in TOKEN.finditer(data):
        name = m.group(1).decode("ascii")
        c[name] += 1
        if m.group(2):
            witharg[name] += 1
    return c, witharg


def dump_file(data, raw=False, limit=None):
    text = data.decode("cp932", "replace")
    lines = text.splitlines()
    for i, line in enumerate(lines[:limit] if limit else lines, 1):
        print("%5d| %s" % (i, line.rstrip()))


def main():
    scypf = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\sc.ypf"
    d, entries = parse_ypf(scypf)
    scen = sorted(k for k in entries if "scenario" in k.lower())
    if len(sys.argv) > 1:
        name = sys.argv[1]
        key = next((k for k in entries if name in k), None)
        if key is None:
            print("未找到:", name)
            return
        data = read_entry(d, entries[key])
        print("== %s(%d 字节)==" % (key, len(data)))
        if "--raw" in sys.argv:
            dump_file(data, raw=True)
        else:
            c, witharg = scan(data)
            for name, n in c.most_common():
                print("  \\%-12s ×%-5d 带参×%d" % (name, n, witharg[name]))
        return
    print("== sc.ypf 条目:%d(其中 scenario* %d 个)==" % (len(entries), len(scen)))
    for k in sorted(entries)[:40]:
        flags, uncomp, comp, off = entries[k]
        print("  %-24s uncomp=%6d comp=%6d zlib=%d" % (k, uncomp, comp, flags))

    if len(sys.argv) > 1:
        name = sys.argv[1]
        key = next((k for k in entries if name in k), None)
        if key is None:
            print("未找到:", name)
            return
        data = read_entry(d, entries[key])
        print("== %s(%d 字节)==" % (key, len(data)))
        if "--raw" in sys.argv:
            dump_file(data, raw=True)
        else:
            c, witharg = scan(data)
            for name, n in c.most_common():
                print("  \\%-12s ×%-5d 带参×%d" % (name, n, witharg[name]))
        return

    # 全库频次
    total = Counter()
    per_file = {}
    for k in scen:
        data = read_entry(d, entries[k])
        c, _ = scan(data)
        per_file[k] = c
        total.update(c)
    print("\n== scenario* 明文命令全库频次(前 60)==")
    for name, n in total.most_common(60):
        files = sum(1 for c in per_file.values() if name in c)
        mark = "  [YSTB 对应已定性]" if name in YSTB_KNOWN else ""
        print("  \\%-14s ×%-7d 出现于 %2d/%d 文件%s" % (name, n, files, len(scen), mark))
    print("\n命令种类:", len(total))


if __name__ == "__main__":
    main()
