#!/usr/bin/env python3
"""P1 对拍分歧探针:dump 指定脚本的组窗口(含表达式解码)。"""
import struct
import sys

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


import zlib


def decrypt(y):
    b = bytearray(y)
    for i in range(0x20, len(b)):
        b[i] ^= KEY[i % 4]
    return bytes(b)


def yscm_names(yscm):
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
    return names


def decode_expr(content, off, ln):
    """表达式字节码 → 可读串([op][len:u16][operand])。"""
    out = []
    p = off
    end = off + ln
    while p < end:
        op = content[p]
        ilen = struct.unpack_from("<H", content, p + 1)[0]
        operand = content[p + 3:p + 3 + ilen]
        p += 3 + ilen
        if op in (0x48, 0x56, 0x76) and ilen == 3:
            pre = chr(operand[0])
            vid = struct.unpack_from("<H", operand, 1)[0]
            kind = {0x48: "var", 0x56: "ref", 0x76: "varidx"}[op]
            out.append(f"{kind}({pre}{vid})")
        elif op == 0x42 and ilen == 1:
            out.append(f"push({operand[0]})")
        elif op == 0x57 and ilen == 2:
            out.append(f"push({struct.unpack_from('<h', operand)[0]})")
        elif op == 0x49 and ilen == 4:
            out.append(f"push({struct.unpack_from('<i', operand)[0]})")
        elif op == 0x4c and ilen == 8:
            out.append(f"push({struct.unpack_from('<q', operand)[0]})")
        elif op == 0x29:
            out.append("aload")
        elif op == 0x2b:
            out.append("+")
        elif op == 0x2d:
            out.append("-")
        elif op == 0x2a:
            out.append("*")
        elif op == 0x2f:
            out.append("/")
        else:
            out.append(f"op{op:#04x}")
    return " ".join(out)


def main():
    ypf = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf"
    script_id = int(sys.argv[1]) if len(sys.argv) > 1 else 190
    lo = int(sys.argv[2]) if len(sys.argv) > 2 else 25
    hi = int(sys.argv[3]) if len(sys.argv) > 3 else 42
    hexdump = "--hex" in sys.argv
    d = open(ypf, "rb").read()
    entries = parse_ypf(d)
    yscm = read(d, entries["%ysbin\\ysc.ybn"])
    names = yscm_names(yscm)
    target = None
    for k, e in entries.items():
        if k.endswith("\\yst%05d.ybn" % script_id):
            target = e
            break
    if target is None:
        print("script %d not found" % script_id)
        return
    b = decrypt(read(d, target))
    magic, ver, g = struct.unpack_from("<4sII", b, 0)
    part1 = struct.unpack_from("<%dI" % g, b, 0x20)
    # 布局:[0x20 header][part1 4G][commands Σcnt×12][content ctlen][part4]
    total_windows = sum((v >> 8) & 0xFF for v in part1)
    content = b[0x20 + 4 * g + total_windows * 12:]
    ctlen = len(content)
    print(f"script {script_id}: {g} groups, {total_windows} windows, "
          f"pool {ctlen}B (yscm {len(names)} names)")
    rec_off = 0
    recs = []
    for gi in range(g):
        v = part1[gi]
        cmd = v & 0xFF
        cnt = (v >> 8) & 0xFF
        recs.append((cmd, cnt, rec_off))
        rec_off += cnt * 12
    for gi in range(lo, min(hi, g)):
        cmd, cnt, ro = recs[gi]
        name = names[cmd] if cmd < len(names) else "?"
        print(f"g{gi}: 0x{cmd:02x} {name} windows={cnt}")
        for wi in range(cnt):
            base = 0x20 + 4 * g + ro + wi * 12
            tag, ln, off = struct.unpack_from("<III", b, base)
            b0 = tag & 0xFF
            b2 = (tag >> 16) & 0xFF
            b3 = (tag >> 24) & 0xFF
            expr = decode_expr(content, off, min(ln, 120)) if ln else ""
            print(f"    w{wi}: tag=({b0},{b2},{b3}) len={ln} off={off} {expr}")
            if hexdump and ln:
                seg = content[off:off + ln]
                for i in range(0, len(seg), 16):
                    print("        %04x: %s" % (off + i, " ".join(
                        "%02x" % x for x in seg[i:i + 16])))


if __name__ == "__main__":
    main()
