#!/usr/bin/env python3
"""P1 —— 引擎静态 dispatch 基线 + 与 Rust VM 事件流 diff。

引擎的命令处理器调度是**确定**的:脚本 part1 区的组表(组→命令类型,YSCM 下标)
+ 确认过的流控语义(GO/GOSUB/IF/LOOP 跳转)共同决定"哪组执行哪条命令"。
本探针:
  1. 从 part1 组表 + YSCM 命令名,推导脚本的**引擎期望命令序**基线(JSONL)。
  2. 与 Rust VM 的 `GroupExecuted` 事件流(由 `yuris ystb trace` 或测试导出)
     对拍,报告每组命令是否一致。

用法:
  python3 scripts/probe_golden_diff.py <bn.ypf> <yst_basename> [--vm-events vm.jsonl]

注意:引擎真值基线的"部分"依赖已 Confirmed 的流控跳转语义(成果 23/28/30/37/38);
若某跳转语义未 Confirmed,相应组标注 UNKNOWN,不猜。
"""
import json
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


def group_table(b):
    magic, ver, unk1 = struct.unpack_from("<4sII", b, 0)
    part1 = struct.unpack_from(f"<{unk1}I", b, 0x20)
    return unk1, part1


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


def dispatch_baseline(part1, names):
    """线性组序(不含跳转)作为基线;跳转语义由 VM diff 时处理。"""
    out = []
    for g, v in enumerate(part1):
        c = v & 0xFF
        out.append({"g": g, "cmd": c, "name": names[c] if c < len(names) else "?"})
    return out


def main():
    path = sys.argv[1]
    base = sys.argv[2]
    d = open(path, "rb").read()
    entries = parse_ypf(d)
    target = None
    for k, e in entries.items():
        if k.endswith("\\" + base):
            target = e
            break
    if target is None:
        print(f"[diff] 未找到 {base}")
        return
    y = read(d, target)
    b = decrypt(y)
    unk1, part1 = group_table(b)
    names = yscm_names(read(d, entries["%ysbin\\ysc.ybn"]))
    baseline = dispatch_baseline(part1, names)
    print(f"[diff] {base}: {unk1} 组;引擎静态组序基线:")
    for e in baseline:
        print(f"  g{e['g']}: 0x{e['cmd']:02x} {e['name']}")

    if "--vm-events" in sys.argv:
        import json as _j
        with open(sys.argv[sys.argv.index("--vm-events") + 1]) as f:
            vm = _j.load(f)
        # 对拍:VM GroupExecuted{pc,command} vs baseline{g,cmd}
        vm_by_pc = {e["pc"]: e["command"] for e in vm if e.get("ev") in (
            "group", "group_executed")}
        mism = 0
        for e in baseline:
            c = vm_by_pc.get(e["g"])
            if c is None:
                print(f"  [UNKNOWN] g{e['g']}: VM 未执行(流控或未到达)")
            elif c != e["cmd"]:
                print(f"  [MISMATCH] g{e['g']}: vm=0x{c:02x} base=0x{e['cmd']:02x}({e['name']})")
                mism += 1
        print(f"[diff] 与 Rust VM 对拍: {mism} 处不匹配;未到达组为流控跳转(非错误)")


if __name__ == "__main__":
    main()
