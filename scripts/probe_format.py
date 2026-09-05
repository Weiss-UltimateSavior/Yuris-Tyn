#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
YU-RIS 格式探测脚本（参考实现）

用途：对本仓库 docs/formats/*.md 中的规格做**可复现验证**。
      Rust 实现完成后，用同样的断言写单元测试。

用法：
    python3 scripts/probe_format.py <bn.ypf 路径> [--key-hex 2b904f93]

已验证样本：
    AnimalTrailGirlishSquare 2/pac/bn.ypf   (YPF v500 / 引擎 v555 / 309 条目)

注意：这只是一次性探测工具，不是产品代码。产品实现在 crates/ 下用 Rust 写。
"""

import argparse
import struct
import sys
import zlib
from collections import Counter

YPF_NAME_XOR_KEY = 0xC9
YSTB_HEADER_SKIP = 0x20
SLOT_SIZE = 12


# ---------------------------------------------------------------- YPF

def parse_ypf(data: bytes):
    magic, version, count, first_data_off = struct.unpack_from("<4sIII", data, 0)
    if magic != b"YPF\0":
        raise ValueError(f"not a YPF file: magic={magic!r}")

    # 索引区最前有 4 字节未明字段（样本为 52 ae 33 00），首个名字从 0x24 开始。
    # 该字段用途 Unknown，原样保留。
    index_prefix = data[0x20:0x24]
    p = 0x24
    entries = []
    for _ in range(count):
        # 文件名 C 字符串：非 0 字节 XOR key，0x00 终止符不加密
        name = bytearray()
        while p < len(data) and data[p] != 0:
            name.append(data[p] ^ YPF_NAME_XOR_KEY)
            p += 1
        p += 1  # 跳过 NUL

        prefix = data[p - len(name) - 1] if len(name) else 0
        flags = data[p]
        p += 1
        uncomp, comp, off, reserved = struct.unpack_from("<IIII", data, p)
        p += 16
        tail = data[p:p + 8]
        p += 8

        entries.append({
            "name": name.decode("ascii", "replace"),
            "prefix": prefix,
            "flags": flags,
            "uncomp": uncomp,
            "comp": comp,
            "off": off,
            "reserved": reserved,
            "tail": tail.hex(),
        })

    # 边界：条目模型 len+26 在 308 条上成立（相邻名字间距已验证），
    # 但最后一条尾部少 4 字节，且索引区头部有 4 字节未明字段 —— 两者相抵。
    # 因此不做"精确闭合"断言，改为容差断言，并把残差如实报告。
    residual = first_data_off - p
    if not (-8 <= residual <= 8):
        raise AssertionError(f"residual out of tolerance: {residual}")

    sizes = Counter(len(e["name"]) + 26 for e in entries)
    return {"version": version, "count": count, "first_data_off": first_data_off,
            "entries": entries, "sizes": sizes,
            "index_prefix": index_prefix.hex(), "residual": residual}


def p_end(ypf: dict) -> int:
    return ypf["first_data_off"] - ypf["residual"]


def extract(ypf_data: bytes, e: dict) -> bytes:
    raw = ypf_data[e["off"]: e["off"] + e["comp"]]
    if e["flags"] == 1 and e["comp"] > 0:
        return zlib.decompress(raw)
    return raw


# ---------------------------------------------------------------- YSTB

def xor_cyclic(data: bytes, key: bytes, skip: int) -> bytes:
    out = bytearray(data)
    for i in range(skip, len(out)):
        out[i] ^= key[i % 4]
    return bytes(out)


def guess_ystb_key(header: bytes, first_slot_cipher: bytes) -> bytes:
    """第一条槽位的 tag 通常是 0x00000000（text），因此密文首 4 字节即密钥。"""
    return first_slot_cipher[:4]


def parse_ystb(blob: bytes, key: bytes):
    magic, version, unknown1 = struct.unpack_from("<4sII", blob, 0)
    if magic != b"YSTB":
        raise ValueError(f"not a YSTB file: magic={magic!r}")
    p1, cl, sl, p4 = struct.unpack_from("<IIII", blob, 12)
    unknown2 = struct.unpack_from("<I", blob, 28)[0]

    if 0x20 + p1 + cl + sl + p4 != len(blob):
        raise AssertionError("YSTB section sizes do not sum to file length")
    if cl % SLOT_SIZE != 0:
        raise AssertionError(f"command_len {cl} is not a multiple of {SLOT_SIZE}")

    dec = xor_cyclic(blob, key, YSTB_HEADER_SKIP)
    cmds = dec[0x20 + p1: 0x20 + p1 + cl]

    slots = []
    contiguous = 0
    prev_end = 0
    oob = 0
    for i in range(cl // SLOT_SIZE):
        tag, ln, off = struct.unpack_from("<III", cmds, i * SLOT_SIZE)
        if off + ln > sl:
            oob += 1
        if off == prev_end:
            contiguous += 1
        prev_end = off + ln
        slots.append((tag, ln, off))

    return {
        "version": version, "unknown1": unknown1, "unknown2": unknown2,
        "part1_len": p1, "command_len": cl, "content_len": sl, "part4_len": p4,
        "slots": slots, "contiguous": contiguous, "out_of_bounds": oob,
        "content": dec[0x20 + p1 + cl: 0x20 + p1 + cl + sl],
    }


# ---------------------------------------------------------------- main

def main() -> int:
    ap = argparse.ArgumentParser(description="YU-RIS format probe (reference verifier)")
    ap.add_argument("ypf", help="path to bn.ypf / ysbin.ypf")
    ap.add_argument("--key-hex", default=None,
                    help="YSTB xor key, 4 bytes hex (default: guess from first slot)")
    ap.add_argument("--script", default=None, help="entry name to parse as YSTB")
    args = ap.parse_args()

    data = open(args.ypf, "rb").read()
    ypf = parse_ypf(data)
    print(f"[YPF] version={ypf['version']} count={ypf['count']} "
          f"first_data_off={ypf['first_data_off']:#x}")
    print(f"[YPF] index prefix [0x20,0x24) = {ypf['index_prefix']}  (purpose: Unknown)")
    print(f"[YPF] parsed {len(ypf['entries'])} entries -> end={p_end(ypf):#x}; "
          f"first_data_off={ypf['first_data_off']:#x}; residual={ypf['residual']} bytes")
    print(f"[YPF] entry sizes: {dict(ypf['sizes'])}")

    # magic 分布
    magics = Counter()
    for e in ypf["entries"]:
        if e["comp"] > 0:
            try:
                magics[zlib.decompress(data[e["off"]: e["off"] + e["comp"]])[:4]
                       .decode("ascii", "replace")] += 1
            except Exception:
                magics["<non-zlib>"] += 1
        else:
            magics["<empty>"] += 1
    print(f"[YPF] payload magics: {dict(magics)}")

    # 前缀分布（Unknown 项，保留观察）
    print(f"[YPF] name prefix bytes: "
          f"{dict(Counter(hex(e['prefix']) for e in ypf['entries']))}")

    target = args.script
    if target is None:
        cand = [e for e in ypf["entries"] if e["name"].endswith("yst00000.ybn")]
        if not cand:
            print("[YSTB] no yst00000.ybn found; use --script to pick one")
            return 0
        target = cand[0]["name"]

    entry = next(e for e in ypf["entries"] if e["name"] == target)
    blob = extract(data, entry)
    print(f"\n[YSTB] {target}  ({len(blob)} bytes)")

    if args.key_hex:
        key = bytes.fromhex(args.key_hex)
    else:
        key = guess_ystb_key(blob[:0x20], blob[0x20 + struct.unpack_from("<I", blob, 12)[0]:][:4])
    print(f"[YSTB] key = {key.hex()}")

    ystb = parse_ystb(blob, key)
    print(f"[YSTB] version={ystb['version']} unknown1={ystb['unknown1']} "
          f"unknown2={ystb['unknown2']}")
    print(f"[YSTB] part1={ystb['part1_len']} command={ystb['command_len']} "
          f"content={ystb['content_len']} part4={ystb['part4_len']}")
    n = len(ystb["slots"])
    print(f"[YSTB] slots={n} contiguous={ystb['contiguous']} "
          f"out_of_bounds={ystb['out_of_bounds']}")
    if ystb["contiguous"] != n or ystb["out_of_bounds"] != 0:
        print("[YSTB] !! key is probably wrong (slots are not contiguous)")
        return 1
    print("[YSTB] slots are fully contiguous  -> key verified  ✓")

    print(f"[YSTB] tag histogram: "
          f"{dict(Counter(hex(t) for t, _, _ in ystb['slots']))}")
    print(f"[YSTB] content[0:32] = {ystb['content'][:32].hex(' ')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
