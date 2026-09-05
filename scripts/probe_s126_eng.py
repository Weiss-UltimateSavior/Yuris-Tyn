# -*- coding: utf-8 -*-
"""P5.2 —— 引擎 trace:抓 script126 pc 138-152 首个完整事件窗(含去重)。"""
import json

path = r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine\engine_trace_boot.jsonl"
win = []
printed = False
with open(path, encoding="utf-8") as f:
    for line in f:
        e = json.loads(line)
        if e.get("ev") != "group":
            continue
        if e.get("script") != 126:
            continue
        pc = e.get("pc")
        if printed:
            break
        if 138 <= pc <= 152:
            win.append((pc, str(e["cmd"])))
            if pc >= 152:
                printed = True

# 引擎 trace 每组可能多事件(共享处理器表展开)→ 只保留首次出现的 pc 序列
seen = set()
seq = []
for pc, cmd in win:
    if pc not in seen:
        seen.add(pc)
        seq.append((pc, cmd))
print("engine s126 first pass 138-152 (dedup):")
for w in seq:
    print("  ", w)
