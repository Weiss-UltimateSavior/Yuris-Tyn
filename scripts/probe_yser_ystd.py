#!/usr/bin/env python3
"""YSER(yse.ybn 错误消息池)与 YSTD(yst.ybn 16B)结构探针。

用法:
    python3 scripts/probe_yser_ystd.py <bn.ypf路径>

铁律: 逐字节断言;Unknown 不猜,只确认可确认的部分。
"""
import struct
import sys
import zlib


def parse_ypf(d):
    magic, ver, cnt, data0 = struct.unpack_from("<4sIII", d, 0)
    assert magic == b"YPF\0"
    p = 0x24
    entries = {}
    for _ in range(cnt):
        name = bytearray()
        while d[p] != 0:
            name.append(d[p] ^ 0xC9)
            p += 1
        p += 1
        flags = d[p]; p += 1
        uncomp, comp, off, _res = struct.unpack_from("<IIII", d, p); p += 16
        p += min(8, data0 - p)
        entries[name.decode("ascii", "replace")] = (flags, uncomp, comp, off)
    return entries


def read_entry(d, e):
    flags, uncomp, comp, off = e
    raw = d[off:off + comp]
    return zlib.decompress(raw) if flags == 1 else raw


def main(path):
    d = open(path, "rb").read()
    entries = parse_ypf(d)

    # ---- YSTD (yst.ybn, 16B) ----
    ystd = None
    for k, e in entries.items():
        if k.endswith("\\yst.ybn"):
            ystd = read_entry(d, e)
    print("[YSTD]", end=" ")
    if ystd is None:
        print("未找到 yst.ybn (跳过)")
    else:
        assert len(ystd) == 16, f"YSTD 应 16B, 实得 {len(ystd)}"
        magic = ystd[0:4]
        ver, f8, f12 = struct.unpack_from("<III", ystd, 4)
        print(f"magic={magic!r} version={ver} f8={f8:#x}({f8}) f12={f12:#x}({f12}) "
              f"-> 结构 Confirmed(恒 16B); f8/f12 语义 Unknown")
        assert magic == b"YSTD"

    # ---- YSER (yse.ybn) ----
    yser = None
    for k, e in entries.items():
        if k.endswith("\\yse.ybn"):
            yser = read_entry(d, e)
    print("\n[YSER]", end=" ")
    if yser is None:
        print("未找到 yse.ybn (跳过)")
        return
    magic, ver, count = struct.unpack_from("<4sII", yser, 0)
    print(f"magic={magic!r} ver={ver} entry_count={count}({count:#x}) file={len(yser)}B")
    assert magic == b"YSER"
    # 首条消息起点
    o = 0x14
    # 连续 C 串直到文件尾
    msgs = []
    cur = o
    while cur < len(yser):
        e = yser.find(b"\x00", cur)
        if e < 0:
            print(f"  警告: 无 0x00 终止 at {cur:#x}")
            break
        msgs.append(yser[cur:e])
        cur = e + 1
    # 断言: 终点精确封闭
    print(f"  C 串池: {len(msgs)} 条, 终点 {cur:#x} == len {len(yser):#x} "
          f"-> {'精确封闭 ✓' if cur == len(yser) else '残留!'}")
    # 可解码率
    dec = sum(1 for m in msgs if _sjis_ok(m))
    print(f"  SJIS 可解码条数: {dec}/{len(msgs)}")
    # 抽样
    for m in msgs[:5]:
        print("   ", m.decode("cp932", "replace")[:55])
    print("  ...")
    for m in msgs[-3:]:
        print("   ", m.decode("cp932", "replace")[:55])


def _sjis_ok(b):
    try:
        b.decode("cp932")
        return True
    except UnicodeDecodeError:
        return False


if __name__ == "__main__":
    main(sys.argv[1])
