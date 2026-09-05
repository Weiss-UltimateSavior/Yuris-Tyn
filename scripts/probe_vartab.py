#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""按引擎 FUN_00451348 模型解析变量定义表(候选: %ysbin\\ysv.ybn / yst.ybn / ysl.ybn).

表布局(证据: FUN_00451348 消费 DAT_0087280c):
  +8  u16 条目数
  +10 条目流, 每条:
    +0 u8  kind (1/2/3; 2 需匹配脚本号)
    +1 u8  desc[0]
    +2 u16 脚本号 (kind==2 的匹配键)
    +4 u16 变量 id (描述符表下标)
    +6 u8  type (1=INT 2=FLT 3=STR)
    +7 u8  dims
    +8 dims × u32 各维边界
    然后: INT/FLT = 8B 初值; STR = u16 len + len B
验证: 条目流恰好消费到文件尾.
"""
import struct
import sys
import zlib

YPF = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf"
KEY = bytes.fromhex("2b904f93")


def parse_ypf(data):
    _v, count, first = struct.unpack_from("<III", data, 4)
    off = 0x24
    ents = {}
    for _ in range(count):
        end = data.index(b"\0", off)
        name = bytes(b ^ 0xC9 for b in data[off:end]).decode("cp932")
        off = end + 1
        flag, unc, comp, doff, _r = struct.unpack_from("<BIIII", data, off)
        off += 17 + min(8, first - off)
        ents[name] = (flag, unc, comp, doff)
    return ents


def read(data, ent):
    flag, unc, comp, doff = ent
    raw = data[doff:doff + comp]
    return zlib.decompress(raw) if flag == 1 else raw


def parse_vartab(buf):
    count, = struct.unpack_from("<H", buf, 8)
    pos = 10
    entries = []
    for _ in range(count):
        kind = buf[pos]
        d0 = buf[pos + 1]
        script, = struct.unpack_from("<H", buf, pos + 2)
        varid, = struct.unpack_from("<H", buf, pos + 4)
        ty = buf[pos + 6]
        dims = buf[pos + 7]
        pos += 8
        bounds = []
        for _ in range(dims):
            b, = struct.unpack_from("<I", buf, pos)
            bounds.append(b)
            pos += 4
        if ty == 1:
            val, = struct.unpack_from("<q", buf, pos)
            pos += 8
            init = val
        elif ty == 2:
            val, = struct.unpack_from("<d", buf, pos)
            pos += 8
            init = val
        elif ty == 3:
            ln, = struct.unpack_from("<H", buf, pos)
            pos += 2 + ln
            init = f"<{ln}B>"
        else:
            # type 0 = 仅声明无初值(引擎 if/else 链对 1/2/3 之外的类型不消费任何字节)
            init = None
        entries.append((kind, d0, script, varid, ty, dims, bounds, init))
    return count, pos, entries


def main():
    data = open(YPF, "rb").read()
    ents = parse_ypf(data)
    for name in ("%ysbin\\ysv.ybn", "%ysbin\\yst.ybn", "%ysbin\\ysl.ybn"):
        ent = ents.get(name)
        if not ent:
            print(f"[!] {name} 不存在")
            continue
        buf = read(data, ent)
        print(f"\n=== {name} ({len(buf)} B) magic={buf[:4]!r} ===")
        print("    +4:", buf[4:8].hex(), " +8:", buf[8:10].hex(), " +10:", buf[10:16].hex())
        if len(buf) < 16:
            continue
        try:
            count, consumed, entries = parse_vartab(buf)
        except Exception as e:  # noqa: BLE001
            print(f"    模型解析失败: {e}")
            continue
        print(f"    条目数={count}, 消费到 {consumed}/{len(buf)} "
              f"{'✓ 精确闭合' if consumed == len(buf) else '✗ 未闭合'}")
        from collections import Counter
        print("    kind 分布:", Counter(e[0] for e in entries))
        print("    type 分布:", Counter(e[4] for e in entries))
        print("    涉及变量 id 数:", len({e[3] for e in entries}),
              " id 范围:", min((e[3] for e in entries), default="-"),
              "..", max((e[3] for e in entries), default="-"))
        for e in entries[:8]:
            print("      ", e)
        if len(entries) > 8:
            print("      ...")


if __name__ == "__main__":
    main()
