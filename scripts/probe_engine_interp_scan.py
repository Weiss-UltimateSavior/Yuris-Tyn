#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""扫描 kemonomichi2.exe 反编译产物(Ghidra 输出),定位 YSTB 解释器候选.

判据(项目铁律:逐条断言、不猜):
  A. switch-case 扫描:case 集包含锚点 {0x42,0x48,0x4d,0x2b} 且各**恰好一次**
     (2026-09-03 教训:memset/CRT 的 switch 会污染常量搜索,真求值器按恰好一次过滤)
  B. 强锚点打分:观察到的 27 个 Confirmed opcode 中有多少也以 case 出现
  C. 字符串锚点:引用 "yst%05d" / "ysbin" 的函数(加载点走数据流的起点)

用法: python3 scripts/probe_engine_interp_scan.py [反编译目录]
默认目录: ~/ghidra_all/kemonomichi2.exe
"""
import re
import sys
import pathlib
from collections import Counter

ROOT = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else \
    pathlib.Path.home() / "ghidra_all" / "kemonomichi2.exe"

CASE_RE = re.compile(r'case\s+(0x[0-9a-fA-F]+|\d+)\s*:')

# 27 个 Confirmed opcode(见 docs/opcode/opcode-table.md),0x4d 除外(宽度可变,
# 常被编译器拆成 memcpy 而非 case;故锚点集不含 0x4d 的宽度假设)
CONFIRMED = [
    0x48, 0x42, 0x56, 0x29, 0x3d, 0x2b, 0x57, 0x2c, 0x69, 0x52, 0x2a, 0x2d,
    0x4c, 0x49, 0x2f, 0x73, 0x26, 0x3e, 0x76, 0x21, 0x7c, 0x46, 0x3c, 0x5a,
    0x25, 0x53, 0x5e, 0x41, 0x4f,
]
# 2026-09-03 日志规定的过滤锚点
ANCHORS = (0x42, 0x48, 0x4d, 0x2b)


def case_values(text):
    return [int(m.group(1), 0) for m in CASE_RE.finditer(text)]


def main():
    files = sorted(ROOT.glob("*.c"))
    if not files:
        print(f"[!] no .c files under {ROOT}")
        return 1
    print(f"[*] scanning {len(files)} files in {ROOT}")

    anchors_hits = []
    str_hits = {"yst%05d": [], "ysbin": [], "YSTB": []}
    str_res = {k: re.compile(re.escape(k)) for k in str_hits}

    for f in files:
        text = f.read_text(errors="ignore")
        for k, r in str_res.items():
            if r.search(text):
                str_hits[k].append(f.name)
        cs = case_values(text)
        if not cs:
            continue
        cnt = Counter(cs)
        # 锚点各恰好一次
        if all(cnt.get(a, 0) == 1 for a in ANCHORS):
            n_conf = sum(1 for op in CONFIRMED if cnt.get(op, 0) >= 1)
            anchors_hits.append((f.name, len(cs), n_conf, cnt))

    print(f"\n[+] anchor filter (case 0x42/0x48/0x4d/0x2b each exactly once): "
          f"{len(anchors_hits)} candidates")
    anchors_hits.sort(key=lambda t: (-t[2], t[1]))
    for name, total, n_conf, cnt in anchors_hits[:15]:
        extra = [hex(k) for k in sorted(cnt) if k not in CONFIRMED]
        print(f"  {name}  cases={total} confirmed_ops={n_conf}/{len(CONFIRMED)}"
              f"  extra_cases={extra[:12]}")

    print("\n[+] string anchors:")
    for k, names in str_hits.items():
        print(f"  {k!r}: {len(names)} files -> {names[:10]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
