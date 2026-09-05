#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""解析 PE 节表,提供 VA->文件偏移换算,并 dump 指定 VA 处的函数指针表.

用法:
  python3 scripts/probe_engine_va.py <exe路径> <表VA(hex)> [最大项数]

输出: 每项 VA、文件偏移、值(按 code VA 0x4xxxxx 解读),直到首个非指针项.
"""
import struct
import sys


def load_sections(data):
    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    assert data[pe_off:pe_off + 4] == b"PE\0\0", "not PE"
    coff = pe_off + 4
    num_sec, = struct.unpack_from("<H", data, coff + 2)
    opt_size, = struct.unpack_from("<H", data, coff + 16)
    opt = coff + 20
    image_base, = struct.unpack_from("<I", data, opt + 28)
    secs = []
    sec_off = opt + opt_size
    for i in range(num_sec):
        off = sec_off + i * 40
        name = data[off:off + 8].rstrip(b"\0").decode(errors="replace")
        vsize, va, rsize, rptr = struct.unpack_from("<IIII", data, off + 8)
        secs.append((name, va, vsize, rptr, rsize))
    return image_base, secs


def va2off(secs, image_base, va):
    rva = va - image_base
    for name, sva, vsize, rptr, rsize in secs:
        if sva <= rva < sva + max(vsize, rsize):
            return rptr + (rva - sva)
    return None


def main():
    path, table_va = sys.argv[1], int(sys.argv[2], 16)
    max_n = int(sys.argv[3]) if len(sys.argv) > 3 else 400
    data = open(path, "rb").read()
    image_base, secs = load_sections(data)
    print(f"[*] image_base=0x{image_base:x} sections:")
    for name, sva, vsize, rptr, rsize in secs:
        print(f"    {name:8s} va=0x{image_base+sva:06x} vsize=0x{vsize:06x} raw=0x{rptr:06x}")
    off = va2off(secs, image_base, table_va)
    if off is None:
        print(f"[!] VA 0x{table_va:x} not mapped")
        return 1
    print(f"\n[*] table @VA 0x{table_va:x} -> file 0x{off:x}")
    n = 0
    for i in range(max_n):
        val, = struct.unpack_from("<I", data, off + i * 4)
        if not (0x401000 <= val < 0x600000):  # 典型 .text 范围
            print(f"  [{i:3d}] 0x{val:08x}  <- 非代码指针,停止")
            break
        print(f"  [{i:3d}] 0x{val:08x}")
        n += 1
    print(f"[*] {n} code pointers")
    return 0


if __name__ == "__main__":
    sys.exit(main())
