#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""YSCM 下标表 + tail 结构验证(引擎模型:35 个 C 串 + 256 字节表).

证据源: kemonomichi2.exe FUN_0046305c(命令处理器表初始化):
  - body: 121 × {name\\0, u8 param_count, param×{name\\0, u8 low, u8 high}}
  - tail: 35 × C 字符串 → DAT_00667fc0, 256 字节 → DAT_00667d40

用法: python3 scripts/probe_yscm_index.py [ysc.ybn路径]
"""
import sys
import pathlib

p = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path("/tmp/ysc.ybn")
data = p.read_bytes()
assert data[:4] == b"YSCM"
import struct
count, = struct.unpack_from("<I", data, 8)
off = 0x10
commands = []
for i in range(count):
    end = data.index(b"\0", off)
    name = data[off:end].decode("ascii")
    off = end + 1
    pc = data[off]
    off += 1
    params = []
    for _ in range(pc):
        end = data.index(b"\0", off)
        pname = data[off:end].decode("ascii")
        off = end + 1
        lo, hi = data[off], data[off + 1]
        off += 2
        params.append((pname, lo, hi))
    commands.append((name, params))
body_end = off
print(f"[*] body end = 0x{body_end:x}, tail = {len(data) - body_end} bytes")

# tail: 37 个 C 串(引擎 do-while i<0x91, i+=4 → 先解析后判步,恰 37 次;
# 注意「35 次」的朴素读法会巧合闭合 785+256+4=1045 但把末两条空格串并进了表)
tail = data[body_end:]
strings = []
t = 0
step = 0
while True:
    end = tail.index(b"\0", t)
    strings.append(tail[t:end])
    t = end + 1
    step += 4
    if step >= 0x91:
        break
print(f"[*] {len(strings)} strings consume {t} bytes, remaining = {len(tail) - t} bytes")
for i, s in enumerate(strings):
    try:
        txt = s.decode("cp932")
    except UnicodeDecodeError:
        txt = repr(s)
    print(f"  str[{i:2d}] ({len(s):3d}B) {txt!r}")
rest = tail[t:]
print(f"[*] rest {len(rest)} bytes (engine 拷 256 字节到 DAT_00667d40)")
if len(rest) >= 256:
    tbl = rest[:256]
    ones = [i for i, b in enumerate(tbl) if b]
    print(f"    非零字节 {len(ones)} 个: {ones[:40]}{'...' if len(ones) > 40 else ''}")
    print(f"    尾部剩余 {len(rest) - 256} 字节: {rest[256:][:32].hex()}")

print("\n[*] YSCM 命令下标表(引擎命令类型 byte0 = 此下标):")
for i, (name, params) in enumerate(commands):
    print(f"  [{i:3d}] 0x{i:02x} {name:20s} params={len(params)}")
