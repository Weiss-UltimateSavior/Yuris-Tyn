#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""YSTB 组模型深挖: tag 语义 + part4 溢出.

问题:
  Q1. tag 分布是否由 (组类型, 组内窗口下标) 决定? → U3
  Q2. tag0 窗口 off+len > content_len 的溢出是否落进 part4 且为 SJIS 文本?
      → 修正「共享字节码池」旧模型

用法: python3 scripts/probe_tag_semantics.py <bn.ypf路径>
"""
import struct
import sys
import zlib
import collections
import pathlib

KEY = bytes.fromhex("2b904f93")
NAME_XOR = 0xC9


def parse_ypf(data):
    _ver, count, first_data = struct.unpack_from("<III", data, 4)
    off = 0x24
    entries = []
    for _ in range(count):
        end = data.index(b"\0", off)
        name = bytes(b ^ NAME_XOR for b in data[off:end]).decode("cp932")
        off = end + 1
        flag, uncomp, comp, doff, _res = struct.unpack_from("<BIIII", data, off)
        off += 17 + min(8, first_data - off)
        entries.append((name, flag, uncomp, comp, doff))
    return entries


def read_entry(data, ent):
    name, flag, uncomp, comp, doff = ent
    raw = data[doff:doff + comp]
    if flag == 1:
        raw = zlib.decompress(raw)
    return raw


def regions(raw):
    unk1, p1len, clen, ctlen, p4len = struct.unpack_from("<IIIII", raw, 8)
    out = bytearray(raw)
    pos = 0x20
    for ln in (p1len, clen, ctlen, p4len):
        for i in range(ln):
            out[pos + i] ^= KEY[i & 3]
        pos += ln
    return unk1, p1len, clen, ctlen, p4len, bytes(out)


def try_sjis(b):
    try:
        return b.decode("cp932")
    except UnicodeDecodeError:
        return None


def main():
    data = open(sys.argv[1], "rb").read()
    entries = [e for e in parse_ypf(data) if e[0].startswith("$ysbin\\yst0")]
    print(f"[*] {len(entries)} 个 YSTB 脚本")

    tag_by_pos = collections.defaultdict(collections.Counter)   # (gtype, widx) -> tag>>16 counter
    tag_lo_by_pos = collections.defaultdict(collections.Counter)  # (gtype, widx) -> tag&0xffff counter
    spill_files = collections.Counter()
    spill_total = 0
    spill_text_hits = 0
    spill_samples = []
    window_total = 0

    for ent in entries:
        raw = read_entry(data, ent)
        unk1, p1len, clen, ctlen, p4len, reg = regions(raw)
        p1 = reg[0x20:0x20 + p1len]
        cmds = reg[0x20 + p1len:0x20 + p1len + clen]
        content = reg[0x20 + p1len + clen:0x20 + p1len + clen + ctlen]
        part4 = reg[0x20 + p1len + clen + ctlen:0x20 + p1len + clen + ctlen + p4len]
        pos = 0
        for g in range(unk1):
            gtype, gcount, gparam = struct.unpack_from("<BBH", p1, g * 4)
            for w in range(gcount):
                tag, wlen, woff = struct.unpack_from("<III", cmds, pos)
                pos += 12
                window_total += 1
                key = (gtype, w)
                tag_by_pos[key][tag >> 16] += 1
                tag_lo_by_pos[key][tag & 0xFFFF] += 1
                if woff + wlen > ctlen:
                    spill = woff + wlen - ctlen
                    spill_files[ent[0]] += 1
                    spill_total += 1
                    in_part4 = spill <= p4len
                    seg = content[woff:ctlen] + part4[:spill] if in_part4 else content[woff:ctlen]
                    txt = try_sjis(seg)
                    if txt is not None and len(txt) > 1:
                        spill_text_hits += 1
                        if len(spill_samples) < 8:
                            spill_samples.append((ent[0], g, w, hex(tag), woff, wlen, ctlen, txt[:48]))
    print(f"[*] 窗口总数 {window_total}")
    print(f"\n[Q1] (组类型, 窗口下标) -> tag>>16 分布(样例, 组数≥50 的类型):")
    big_types = sorted({g for (g, w) in tag_by_pos
                        if sum(tag_by_pos[(g, w)].values()) >= 50})
    for g in big_types:
        nwin = max(w for (gg, w) in tag_by_pos if gg == g) + 1
        for w in range(nwin):
            c = tag_by_pos.get((g, w))
            if c:
                lo = tag_lo_by_pos.get((g, w), {})
                lo_s = " ".join(f"0x{k:04x}×{v}" for k, v in sorted(lo.items())[:6])
                print(f"  type 0x{g:02x} w{w}: hi={dict(sorted(c.items()))} lo={{{lo_s}}}")
    print(f"\n[Q2] 溢出窗口 {spill_total} 个 / {len(spill_files)} 文件; 落入 part4 且 SJIS 可解码: {spill_text_hits}")
    for s in spill_samples:
        print("   ", s)


if __name__ == "__main__":
    main()
