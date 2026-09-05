#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""全语料验证 part1=命令组表 模型(引擎 FUN_00450dfd 的解析逻辑).

模型(证据: kemonomichi2.exe FUN_00450dfd, 2026-09-03):
  header[8] = unknown1(=组数) u32 @ +0x08
  part1 = unknown1 × u32: byte0=命令类型(YSCM 下标), byte1=窗口数 count, [2:4]=u16 参数
  commands 区 = Σ count_i 条 12 字节记录 (tag/len/offset)
  引擎步进: ptr[0]=base; ptr[i+1]=ptr[i]+count_i*12

逐条断言(铁律 2):
  A. part1_len == 4 × unknown1                    (每文件)
  B. Σ count_i × 12 == command_len                (每文件)
  C. 解密后每条记录 offset+len ≤ content_len      (每条)
  D. 命令类型直方图 → 与 YSCM 命令名对照          (全语料)

用法: python3 scripts/probe_part1_groups.py <bn.ypf路径> [ysc.ybn路径]
"""
import io
import struct
import sys
import zlib
import collections
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "crates" / ".."))
KEY = bytes.fromhex("2b904f93")  # 样本游戏统一密钥(Confirmed)
NAME_XOR = 0xC9


def parse_ypf(data):
    assert data[:4] == b"YPF\0"
    _ver, count, first_data = struct.unpack_from("<III", data, 4)
    off = 0x24
    entries = []
    for _ in range(count):
        end = data.index(b"\0", off)
        name = bytes(b ^ NAME_XOR for b in data[off:end]).decode("cp932")
        off = end + 1
        flag, uncomp, comp, doff, _res = struct.unpack_from("<BIIII", data, off)
        off += 17
        tail = min(8, first_data - off)
        off += tail
        entries.append((name, flag, uncomp, comp, doff))
    return entries


def read_entry(data, ent):
    name, flag, uncomp, comp, doff = ent
    raw = data[doff:doff + comp]
    if flag == 1:
        raw = zlib.decompress(raw)
    assert len(raw) == uncomp, (name, len(raw), uncomp)
    return raw


def xor_regions(buf, lens):
    """每区独立 key[i&3] 计数(引擎 4 个独立循环, 编译器 YSTB 写出器一致)。"""
    out = bytearray(buf)
    pos = 0x20
    for ln in lens:
        for i in range(ln):
            out[pos + i] ^= KEY[i & 3]
        pos += ln
    return bytes(out)


def main():
    ypf_path = sys.argv[1]
    data = open(ypf_path, "rb").read()

    # YSCM 命令名
    names = {}
    ysc = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else "/tmp/ysc.ybn")
    if ysc.exists():
        y = ysc.read_bytes()
        cnt, = struct.unpack_from("<I", y, 8)
        off = 0x10
        for i in range(cnt):
            end = y.index(b"\0", off)
            names[i] = y[off:end].decode("ascii")
            off = end + 1
            pc = y[off]
            off += 1
            for _ in range(pc):
                off = y.index(b"\0", off) + 3

    entries = parse_ypf(data)
    yst = [e for e in entries if e[0].startswith("$ysbin\\yst") and e[0].endswith(".ybn")]
    print(f"[*] YPF 条目 {len(entries)},脚本 {len(yst)}")

    stat_files = dict(ok=0, badA=0, badB=0, badC=0)
    type_hist = collections.Counter()
    count_hist = collections.Counter()
    param_hist = collections.Counter()
    type_counts = collections.defaultdict(collections.Counter)
    bad_C_examples = []

    for ent in yst:
        raw = read_entry(data, ent)
        unk1, p1len, clen, ctlen, p4len = struct.unpack_from("<IIIII", raw, 8)
        # A: part1_len == 4*unknown1
        if p1len != 4 * unk1:
            stat_files["badA"] += 1
            print(f"[!] A 违反 {ent[0]}: part1_len={p1len} != 4*{unk1}")
            continue
        regions = xor_regions(raw, [p1len, clen, ctlen, p4len])
        part1 = regions[0x20:0x20 + p1len]
        cmds = regions[0x20 + p1len:0x20 + p1len + clen]
        content = regions[0x20 + p1len + clen:0x20 + p1len + clen + ctlen]
        # 逐组断言 B
        pos = 0
        for g in range(unk1):
            gtype, gcount, gparam = struct.unpack_from("<BBH", part1, g * 4)
            type_hist[gtype] += 1
            count_hist[gcount] += 1
            if gparam:
                param_hist[gparam] += 1
            type_counts[gtype][gcount] += 1
            for w in range(gcount):
                if pos + 12 > clen:
                    stat_files["badB"] += 1
                    print(f"[!] B 违反 {ent[0]} 组{g}: 记录越界 pos={pos}")
                    break
                tag, wlen, woff = struct.unpack_from("<III", cmds, pos)
                pos += 12
                if woff + wlen > ctlen:
                    stat_files["badC"] += 1
                    if len(bad_C_examples) < 5:
                        bad_C_examples.append((ent[0], g, w, hex(tag), woff, wlen, ctlen))
        if pos != clen:
            stat_files["badB"] += 1
            print(f"[!] B 违反 {ent[0]}: Σcount*12={pos} != command_len={clen}")
            continue
        stat_files["ok"] += 1

    total = stat_files["ok"] + stat_files["badA"] + stat_files["badB"] + stat_files["badC"]
    print(f"\n[+] 文件级: ok={stat_files['ok']} badA={stat_files['badA']} "
          f"badB={stat_files['badB']} badC(文件级以上已计)={stat_files['badC']} / {total}")
    if bad_C_examples:
        print("[!] C 违反示例:")
        for e in bad_C_examples:
            print("   ", e)
    print("\n[*] 命令类型直方图(组数):")
    for t, n in sorted(type_hist.items()):
        nm = names.get(t, "?")
        print(f"  0x{t:02x} ({n:6d} 组) {nm:16s} 窗口数分布={dict(sorted(type_counts[t].items()))}")
    print(f"\n[*] 组内窗口数分布: {dict(sorted(count_hist.items()))}")
    print(f"[*] 非零 u16 参数分布(前 20): {dict(sorted(param_hist.items())[:20])}")


if __name__ == "__main__":
    main()
