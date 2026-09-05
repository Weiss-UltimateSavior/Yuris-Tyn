#!/usr/bin/env python3
"""P5.2 —— 引擎 trace 事件类型分布 + 字符串值样本。"""
import json
from collections import Counter

eng = r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine\engine_trace_boot.jsonl"
hist = Counter()
samples = {}
with open(eng, encoding="utf-8") as f:
    for line in f:
        e = json.loads(line)
        ev = e.get("ev", "?")
        hist[ev] += 1
        if ev not in samples:
            samples[ev] = json.dumps(e, ensure_ascii=False)[:180]
for ev, n in hist.most_common():
    print(f"{ev:>16} x{n}  {samples[ev]}")
