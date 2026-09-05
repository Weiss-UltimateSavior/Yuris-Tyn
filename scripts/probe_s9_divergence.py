#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""P1 收尾侦查:script9(yst00009)分歧点上下文解码。

对拍剩余 4 处分歧(script9):
  [144055] pc1092 IF:引擎假(跳 1095)/ VM 真(执行 1093/1094)
  [144407] pc633 LOOPEND:引擎出循环/ VM 回 628
  [144801] pc262 LOOPEND:VM 多跑一轮
  [144818] pc265 LOOPEND:VM 多跑一轮
本探针解码相关组的窗口(tag/len/off)+ 表达式指令,定位 IF 条件读的变量与
LOOP 计数表达式。
"""
import struct
import sys
import zlib

NAME_KEY = 0xC9
KEY = bytes.fromhex("2b904f93")


def rcs(d, dp):
    out = bytearray()
    while d[dp] != 0:
        out.append(d[dp] ^ NAME_KEY)
        dp += 1
    return bytes(out), dp + 1


def parse_ypf(d):
    _, _, cnt, data0 = struct.unpack_from("<4sIII", d, 0)
    p = 0x24
    entries = {}
    for _ in range(cnt):
        name, p = rcs(d, p)
        flags = d[p]; p += 1
        uncomp, comp, off, _res = struct.unpack_from("<IIII", d, p); p += 16
        p += min(8, data0 - p)
        entries[name.decode("ascii", "replace")] = (flags, uncomp, comp, off)
    return entries


def read(d, e):
    flags, _, comp, off = e
    raw = d[off:off + comp]
    return zlib.decompress(raw) if flags == 1 else raw


def decrypt(y):
    b = bytearray(y)
    for i in range(0x20, len(b)):
        b[i] ^= KEY[i % 4]
    return bytes(b)


OPS = {0x42: ("pushint8", 1), 0x57: ("pushint16", 2), 0x49: ("pushint32", 4),
       0x4c: ("pushint64", 8), 0x46: ("pushfloat", 8), 0x52: ("neg", 0),
       0x2a: ("mul", 0), 0x2b: ("add", 0), 0x2d: ("sub", 0), 0x2f: ("div", 0),
       0x25: ("mod", 0), 0x3c: ("lt", 0), 0x3e: ("gt", 0), 0x3d: ("eq", 0),
       0x21: ("ne", 0), 0x5a: ("ge", 0), 0x53: ("le", 0), 0x26: ("logand", 0),
       0x7c: ("logor", 0), 0x41: ("bitand", 0), 0x4f: ("bitor", 0),
       0x5e: ("xor", 0), 0x73: ("tostr", 0), 0x69: ("toint", 0),
       0x2c: ("groupsep", 0), 0x29: ("arrayload", 0)}


def decode_expr(b, off, ln, s9):
    out = []
    i = 0
    end = off + ln
    while off + i < end:
        op = b[off + i]
        if op == 0x4d:
            l = struct.unpack_from("<H", b, off + i + 1)[0]
            payload = b[off + i + 3: off + i + 3 + l]
            out.append("M<%s>" % payload.decode("cp932", "replace"))
            i += 3 + l
            continue
        if op in (0x48, 0x56, 0x76):
            l = struct.unpack_from("<H", b, off + i + 1)[0]
            if l == 3:
                pfx = b[off + i + 3]
                vid = struct.unpack_from("<H", b, off + i + 4)[0]
                pc_ = {0x48: "@", 0x56: "&", 0x76: "#"}[op]
                out.append("%svar%s%s" % (pc_, pfx, vid))
            else:
                out.append("op%02x l%d" % (op, l))
            i += 3 + l
            continue
        if op in OPS:
            name, ol = OPS[op]
            operand = b[off + i + 3: off + i + 3 + ol]
            if ol:
                if name == "pushfloat":
                    val = struct.unpack_from("<d", b, off + i + 3)[0]
                    out.append("%s(%r)" % (name, val))
                else:
                    out.append("%s(%s)" % (name, operand.hex()))
            else:
                out.append(name)
            i += 3 + ol
            continue
        out.append("??op%02x" % op)
        l = struct.unpack_from("<H", b, off + i + 1)[0] if off + i + 3 <= end else 0
        i += 3 + l
    return " ".join(out)


def main():
    d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
    entries = parse_ypf(d)
    y = read(d, entries["$ysbin\\yst00009.ybn"])
    b = decrypt(y)
    magic, ver, G = struct.unpack_from("<4sII", b, 0)
    part1_len, cmd_len, ct_len, p4_len = struct.unpack_from("<IIII", b, 0x0C)
    part1 = struct.unpack_from("<%dI" % G, b, 0x20)
    cmd_base = 0x20 + part1_len
    ct_base = cmd_base + cmd_len
    pool = b[ct_base:ct_base + ct_len + p4_len]
    names_ = None
    # yscm 命令名
    yscm = read(d, entries["%ysbin\\ysc.ybn"])
    pp = 0x10
    names = []
    for _ in range(struct.unpack_from("<I", yscm, 8)[0]):
        e = yscm.index(b"\0", pp)
        names.append(yscm[pp:e].decode("cp932", "replace"))
        pp = e + 1
        pc = yscm[pp]; pp += 1
        for _ in range(pc):
            e = yscm.index(b"\0", pp); pp = e + 1
            pp += 2

    # 组 -> 窗口表
    rec_off = cmd_base
    wins = []
    for gidx, v in enumerate(part1):
        c = v & 0xFF
        cnt = (v >> 8) & 0xFF
        gp = (v >> 16) & 0xFFFF
        ws = []
        for k in range(cnt):
            tag, ln, off = struct.unpack_from("<III", b, rec_off + k * 12)
            ws.append((tag, ln, off))
        rec_off += cnt * 12
        wins.append((gidx, c, gp, ws))

    lo = int(sys.argv[1]) if len(sys.argv) > 1 else 1085
    hi = int(sys.argv[2]) if len(sys.argv) > 2 else 1100
    for gidx, c, gp, ws in wins:
        if not (lo <= gidx <= hi):
            continue
        nm = names[c] if c < len(names) else "?"
        print("g%d: 0x%02x %s gparam=0x%04x wins=%d" % (gidx, c, nm, gp, len(ws)))
        for k, (tag, ln, off) in enumerate(ws):
            expr = decode_expr(pool, off, ln, 9)
            print("    w%d tag=[%02x %02x %02x %02x] len=%d: %s" % (
                k, tag & 0xFF, (tag >> 8) & 0xFF, (tag >> 16) & 0xFF,
                (tag >> 24) & 0xFF, ln, expr[:220]))
    # LOOP 计数窗与 IF 条件窗的 w1.len(编译期组号)
    if hi >= 1100:
        print("\n-- IF/LOOP 目标域(编译期组号) --")
        for gidx, c, gp, ws in wins:
            if c in (0x2c, 0x37) and lo - 40 <= gidx <= hi + 40:
                tg = [w[0] for w in ws]
                lens = [w[1] for w in ws]
                print("g%d cmd=0x%02x lens=%s" % (gidx, c, lens))


if __name__ == "__main__":
    main()
