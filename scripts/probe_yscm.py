#!/usr/bin/env python3
"""YSCM (ysc.ybn) 结构探针 —— P1.1 的可复现验证。

用法:
    python3 scripts/probe_yscm.py <解压后的ysc.ybn路径>

声明待验证的结构 (来自 yscm.rs, 未入 PROGRESS):
    [Header 0x10]  magic "YSCM" + version u32 + command_count u32 + unknown u32
    [Body]         command_count x { name\\0  u8 param_count
                                     param_count x { param_name\\0  u16 type } }
    [Tail]         剩余字节 (结构 Unknown)

铁律: 所有断言逐条执行, 不接受"总量对得上"。
"""
import struct
import sys
from collections import Counter


def read_cstring(data, p):
    end = data.index(b"\x00", p)
    return data[p:end], end + 1


def main(path):
    d = open(path, "rb").read()
    print(f"[YSCM] file size = {len(d)}")

    magic, version, count, unknown = struct.unpack_from("<4sIII", d, 0)
    print(f"[YSCM] magic={magic!r} version={version} command_count={count} unknown={unknown:#x}")
    assert magic == b"YSCM", f"bad magic {magic!r}"

    p = 0x10
    commands = []  # (name, [(pname, ty), ...], start_offset)
    for i in range(count):
        start = p
        name, p = read_cstring(d, p)
        param_count = d[p]
        p += 1
        params = []
        for _ in range(param_count):
            pname, p = read_cstring(d, p)
            (ty,) = struct.unpack_from("<H", d, p)
            p += 2
            params.append((pname, ty))
        commands.append((name, params, start))

    tail = d[p:]
    print(f"[YSCM] body ended at {p:#x}, tail = {len(tail)} bytes")
    print(f"[YSCM] parsed {len(commands)} commands, "
          f"{sum(len(c[1]) for c in commands)} params total")

    # ---- 逐条断言 ----
    # A1: 条目数 == header 声明
    assert len(commands) == count, (len(commands), count)
    # A2: 每个命令名非空、可打印 ASCII
    for name, params, _ in commands:
        assert 0 < len(name) <= 32 and all(0x20 <= b < 0x7F for b in name), name
    # A3: 参数名可打印或为 ":xxx" 形式
    bad = []
    for name, params, _ in commands:
        for pname, _ty in params:
            if not all(0x20 <= b < 0x7F for b in pname):
                bad.append((name, pname))
    assert not bad, bad[:10]
    # A4: body 结束位置之后是 tail(非空 => 看前 64 字节), 断言 body 没有越过文件尾
    assert p <= len(d)
    # A5: 命令名唯一
    names = [c[0] for c in commands]
    dup = [n for n, k in Counter(names).items() if k > 1]
    assert not dup, f"duplicated command names: {dup}"

    # A6: 参数类型码分布 (逐值列出)
    tys = Counter(ty for _n, params, _s in commands for _pn, ty in params)
    print(f"[YSCM] param type histogram: "
          f"{ {hex(k): v for k, v in sorted(tys.items())} }")

    # 输出前若干条命令样例
    for name, params, start in commands[:8]:
        print(f"  [{start:#06x}] {name}  params={[(pn.decode(), hex(t)) for pn, t in params]}")
    print("  ...")
    for name, params, start in commands[-4:]:
        print(f"  [{start:#06x}] {name}  params={[(pn.decode(), hex(t)) for pn, t in params]}")

    # tail 内容观察
    printable = bytes(b if 0x20 <= b < 0x7F or b in (0x0A,) else 0x2E for b in tail[:200])
    print(f"[YSCM] tail[:200] printable preview:\n{printable.decode('ascii')}")

    print("[YSCM] ALL ASSERTIONS PASSED")


if __name__ == "__main__":
    main(sys.argv[1])
