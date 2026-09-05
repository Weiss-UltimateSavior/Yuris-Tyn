#!/usr/bin/env python3
"""P2 —— 运行期命令直方图(引擎真值 trace)+ VM 覆盖缺口分析。

比静态 part1 组表更精确:引擎 trace 是真实执行的命令频率。
输出:
  1. 引擎侧 cmd 频次(执行序)。
  2. VM 侧事件分布(group/decl/unsupported/语义事件)。
  3. 缺口表:引擎执行了、但 VM 按未实现/声明 no-op 处理的命令
     (排除确证 no-op 处理器族:0x00/0x09/0x33/0x52/0x53/0x54/0x4d/0x50)。

用法:
  python scripts/probe_runtime_hist.py <engine.jsonl> <vm.jsonl>
"""
import json
import sys
from collections import Counter

# 引擎处理器表中已确证的 no-op(执行但无副作用;FUN_00423080 族/标签等):
# 0x00 ALIAS 0x09 DEBUGLIST 0x33 LABEL 0x52 S_FLT 0x53 S_INT 0x54 S_STR
# 0x4d/0x50 RETURNBREAK(编译期标记)——探针实测引擎 handler 表提取
KNOWN_NOOP = {0x00, 0x09, 0x33, 0x52, 0x53, 0x54, 0x4D, 0x50}

# VM 语义事件 → cmd(yuris_vm::cmd 常量,与 diff_engine_vm.py 一致)
SEMANTIC_CMD = {"text": 0x62, "cg": 0x01, "sound": 0x59, "cgact": 0x02,
                "cginfo": 0x04, "cgend": 0x03, "load": 0x36, "save": 0x56}


def main():
    eng = sys.argv[1]
    vm = sys.argv[2]

    eng_hist = Counter()
    with open(eng, "r", encoding="utf-8") as f:
        for line in f:
            e = json.loads(line)
            if e.get("ev") == "group":
                c = e["cmd"]
                if isinstance(c, list):
                    # 共享处理器(ALIAS 等多 cmd 一址)→ 记 each? 记 None 桶
                    for x in c:
                        eng_hist[x] += 0  # 占位:不精确计入
                    eng_hist["AMBIG:" + "/".join(map(str, sorted(c)))] += 1
                else:
                    eng_hist[c] += 1
            elif e.get("ev") == "default_stub":
                eng_hist["DEFAULT_STUB"] += 1

    vm_hist = Counter()
    vm_unsup = Counter()
    with open(vm, "r", encoding="utf-8") as f:
        for line in f:
            e = json.loads(line)
            ev = e.get("ev")
            if ev == "group":
                vm_hist[e["cmd"]] += 1
            elif ev in SEMANTIC_CMD:
                vm_hist[SEMANTIC_CMD[ev]] += 1
            elif ev in ("decl", "unsupported", "varquery"):
                vm_hist[e["cmd"]] += 1
                if ev == "unsupported":
                    vm_unsup[e["cmd"]] += 1

    def name(c):
        return "0x%02x" % c if isinstance(c, int) else str(c)

    print("== 引擎运行期命令直方图(执行序,前 40)==")
    rows = []
    for c, n in eng_hist.most_common(40):
        if isinstance(c, str) and c.startswith("AMBIG"):
            rows.append((c, n, "共享 no-op 处理器"))
            continue
        rows.append((name(c), n, ""))
    for c, n, note in rows:
        if isinstance(c, int):
            vm_n = vm_hist.get(c, 0)
            mark = ""
            if c in vm_unsup:
                mark = "  <- VM Unsupported"
            elif vm_n == 0 and c not in KNOWN_NOOP:
                mark = "  <- VM 未覆盖"
            elif vm_n == 0 and c in KNOWN_NOOP:
                mark = "  (no-op)"
            print(f"  {c:>8} {n:>8}  VM={vm_n:>8}{mark} {note}")
        else:
            print(f"  {c:>24} {n:>8}  {note}")

    print("\n== VM Unsupported 命令 ==")
    for c, n in vm_unsup.most_common(20):
        print(f"  {name(c)} × {n}")
    if not vm_unsup:
        print("  (无)")

    print("\n== 引擎执行但 VM 缺口(排除 no-op 族)==")
    gaps = []
    for c, n in eng_hist.items():
        if isinstance(c, str):
            continue
        if c in KNOWN_NOOP:
            continue
        if vm_hist.get(c, 0) == 0 or c in vm_unsup:
            gaps.append((c, n, vm_unsup.get(c, 0)))
    gaps.sort(key=lambda x: -x[1])
    for c, n, u in gaps:
        print(f"  {name(c)} 引擎×{n}  VM unsupported×{u}")
    if not gaps:
        print("  (无缺口)")


if __name__ == "__main__":
    main()
