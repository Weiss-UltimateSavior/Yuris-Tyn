#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
变量系统语料探针（P1 级证据收集）

目的：把「语料里出现的变量引用」与「YSVR 变量定义表」交叉比对，
     为 Rust 侧变量存储模型提供**逐条**证据。

用法：
    python3 scripts/probe_variables.py "path/to/bn.ypf"

输出：
  - 变量引用（按 opcode / 前缀 / id）直方图
  - id 与 YSVR 定义的匹配率
  - 数组维数分布、下标参数形态
"""

import struct
import sys
import zlib
from collections import Counter, defaultdict

NAME_KEY = 0xC9
KEY = bytes.fromhex("2b904f93")
SLOT = 12
HDR = 0x20

# 已证实 opcode（docs/opcode/opcode-table.md 第二版）
OP_PUSHVAR = 0x48
OP_PUSHVARREF = 0x56
OP_PUSHVARIDX = 0x76
OP_ARRAYLOAD = 0x29


def xor_region(buf: bytearray, key: bytes, start: int, length: int) -> None:
    """分区独立计数（编译器 004160ac 实测模型）。"""
    for i in range(length):
        buf[start + i] ^= key[i & 3]


def parse_ypf(data: bytes):
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
        uncomp, comp, off, _res = struct.unpack_from("<IIII", data, p)
        p += 16 + 8
        out.append((nb.decode("ascii", "replace"), flags, uncomp, comp, off))
    return out


def read(ypf_data, e):
    name, flags, uncomp, comp, off = e
    raw = ypf_data[off: off + comp]
    return zlib.decompress(raw) if flags == 1 and comp else raw


def groups(blob: bytes):
    """返回 [(group_type, count, gparam, [window bytes])]"""
    if len(blob) < HDR:
        return None
    magic, ver, G, p1, cl, ct, p4, _u2 = struct.unpack_from("<4sIIIIIII", blob, 0)
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
    pool = bytes(b[xs: xs + ct + p4])  # content + part4（窗口可伸入）

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
            wins.append((tag, ln, off, pool[off: off + ln]))
        ptr += cnt * SLOT
        out.append((gtype, cnt, gparam, wins))
    return out


def decode_var_ops(win: bytes):
    """切分自描述编码，收集变量类指令。

    返回 (refs, anomalies)：
      refs      = [(op, prefix, id)]
      anomalies = [(offset, reason)]  —— 窗口被截断时如实记录，不外推
    """
    refs = []
    bad = []
    i = 0
    n = len(win)
    while i + 3 <= n:
        op = win[i]
        ln = struct.unpack_from("<H", win, i + 1)[0]
        if i + 3 + ln > n:
            bad.append((i, "operand truncated (window cut)"))
            break
        operand = win[i + 3: i + 3 + ln]
        if op in (OP_PUSHVAR, OP_PUSHVARREF, OP_PUSHVARIDX):
            # 证实形态：[前缀字符][id:u16] = 3 字节
            if ln == 3:
                refs.append((op, operand[0], struct.unpack_from("<H", operand, 1)[0]))
            else:
                bad.append((i, "var operand len=%d (expected 3)" % ln))
        elif op == OP_ARRAYLOAD:
            refs.append((op, None, None))
        i += 3 + ln
    return refs, bad


def parse_ysvr(blob: bytes):
    magic, ver = struct.unpack_from("<4sI", blob, 0)
    cnt = struct.unpack_from("<H", blob, 8)[0]
    p = 10
    out = []
    for _ in range(cnt):
        kind = blob[p]
        cat = blob[p + 1]
        script, vid = struct.unpack_from("<HH", blob, p + 2)
        ty = blob[p + 6]
        dims = blob[p + 7]
        p += 8
        bounds = []
        for _ in range(dims):
            bounds.append(struct.unpack_from("<I", blob, p)[0])
            p += 4
        if ty == 1:
            p += 8
        elif ty == 2:
            p += 8
        elif ty == 3:
            sl = struct.unpack_from("<H", blob, p)[0]
            p += 2 + sl
        out.append(dict(kind=kind, cat=cat, script=script, id=vid,
                        ty=ty, dims=dims, bounds=bounds))
    return out, p


def main() -> int:
    path = sys.argv[1]
    data = open(path, "rb").read()
    entries = parse_ypf(data)
    byname = {e[0]: e for e in entries}

    # ---- YSVR ----
    ysv_name = next((n for n in byname if n.endswith("\\ysv.ybn")), None)
    vardefs = {}
    if ysv_name:
        vd, consumed = parse_ysvr(read(data, byname[ysv_name]))
        vardefs = {v["id"]: v for v in vd}
        print(f"[YSVR] {len(vd)} 定义条目, 消费 {consumed} 字节")

    # ---- 语料扫描 ----
    op_hist = Counter()
    prefix_hist = Counter()
    id_hist = Counter()
    id_seen = set()
    arr_dims = Counter()
    per_prefix_ids = defaultdict(set)
    scripts = 0

    for e in entries:
        name = e[0]
        if not name.endswith(".ybn"):
            continue
        blob = read(data, e)
        g = groups(blob)
        if g is None:
            continue
        scripts += 1
        for gtype, cnt, gparam, wins in g:
            for tag, ln, off, w in wins:
                for op, prefix, vid in decode_var_ops(w):
                    op_hist[op] += 1
                    if op == OP_ARRAYLOAD:
                        continue
                    prefix_hist[prefix] += 1
                    id_hist[vid] += 1
                    id_seen.add(vid)
                    per_prefix_ids[prefix].add(vid)

    print(f"\n[语料] {scripts} 个 YSTB 脚本")
    print("\n=== 变量类 opcode 直方图 ===")
    for op, n in op_hist.most_common():
        nm = {OP_PUSHVAR: "PushVar", OP_PUSHVARREF: "PushVarRef",
              OP_PUSHVARIDX: "PushVarIndexed", OP_ARRAYLOAD: "ArrayLoad"}.get(op, hex(op))
        print(f"  0x{op:02x} {nm:<15} {n}")

    print("\n=== 前缀字符直方图 ===")
    for p, n in prefix_hist.most_common():
        ch = chr(p) if 32 <= p < 127 else "."
        print(f"  0x{p:02x} '{ch}'  {n}  (distinct ids={len(per_prefix_ids[p])})")

    print("\n=== id 范围 / 与 YSVR 匹配 ===")
    if id_seen:
        lo, hi = min(id_seen), max(id_seen)
        matched = len(id_seen & set(vardefs))
        print(f"  id 范围 = {lo}..{hi}   distinct = {len(id_seen)}")
        print(f"  YSVR 命中 = {matched}/{len(id_seen)}  ({100*matched/len(id_seen):.1f}%)")
        missing = sorted(id_seen - set(vardefs))[:20]
        print(f"  未命中样例 = {missing}")
        if vardefs:
            print(f"  YSVR id 范围 = {min(vardefs)}..{max(vardefs)}")

    print("\n=== YSVR 类型 / 维数分布 ===")
    print("  ty  :", dict(Counter(v["ty"] for v in vardefs.values())))
    print("  dims:", dict(Counter(v["dims"] for v in vardefs.values())))
    print("  kind:", dict(Counter(v["kind"] for v in vardefs.values())))

    print("\n=== 出现频次 Top 15 变量 id ===")
    for vid, n in id_hist.most_common(15):
        v = vardefs.get(vid)
        ty = {0: "decl", 1: "INT", 2: "FLT", 3: "STR"}.get(v["ty"], "?") if v else "-"
        dims = v["dims"] if v else "-"
        print(f"  id={vid:<6} n={n:<7} ty={ty:<5} dims={dims}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
