# -*- coding: utf-8 -*-
"""P7.1 —— 引擎 trace 中实际执行的 CGINFO(0x04)/CGEND(0x03) 组号统计。"""
import collections
import json

ENG = r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine\engine_trace_boot.jsonl"

hits = collections.Counter()
with open(ENG, encoding="utf-8") as f:
    for line in f:
        e = json.loads(line)
        if e.get("ev") != "group":
            continue
        cmd = e.get("cmd")
        if isinstance(cmd, list) or cmd not in (0x04, 0x03, 0x01, 0x02):
            continue
        hits[(e["script"], cmd, e["pc"])] += 1

for (sc, cmd, pc), n in sorted(hits.items()):
    print("s%d g%-5d cmd=%02x x%d" % (sc, pc, cmd, n))
print("distinct:", len(hits))
