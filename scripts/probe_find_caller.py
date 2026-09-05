# -*- coding: utf-8 -*-
"""P5.2 —— 定位 s22 pc462 的标签与调用点(临时侦查脚本)。"""
import struct
import zlib
import sys

sys.path.insert(0, r"D:\yuris-kernel\scripts")
from probe_group_windows import parse_ypf, read, decrypt, yscm_names

NAME_KEY = 0xC9

d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
e = parse_ypf(d)
key = [k for k in e if k.endswith("ysl.ybn")][0]
ysl = read(d, e[key])

# YSLB: magic b"YSLB" + version u32 + u32 标签数 + 256×u32 桶头 + 标签 ×
# {u8 len, name, u32 hash, u32 target_pc, u16 script_id, u8 flag_a}
assert ysl[:4] == b"YSLB", ysl[:4]
cnt = struct.unpack_from("<I", ysl, 8)[0]
p = 0x10 + 256 * 4
labels = {}
targets = {}
for _ in range(cnt):
    ln = ysl[p]; p += 1
    name = ysl[p:p + ln].decode("cp932", "replace"); p += ln
    if p + 10 > len(ysl):  # 末条被引擎 stride 怪癖截断(见 command-layer.md §9b)
        print("last record truncated at", name)
        break
    h, pc, sid = struct.unpack_from("<IIH", ysl, p); p += 11  # hash4+pc4+sid2+flag_a1
    labels[name] = (pc, sid)
    targets.setdefault((sid, pc), []).append(name)

print("labels:", cnt)
for t in sorted(targets.get((22, 462), [])):
    print("label(s) -> s22 pc462:", t)
# 相邻 pc 也列出(标签可能指向 461/463)
for pc in (460, 461, 462, 463):
    names = targets.get((22, pc))
    if names:
        print(f"s22 pc{pc}:", names)
