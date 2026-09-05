# -*- coding: utf-8 -*-
"""P5.2 —— 修复 vm_v5:验证 vm_v5/vm_v6 内容差异,结果落盘。"""
import json

out = []
for name in ("vm_v5", "vm_v6"):
    path = r"C:\Users\weiss\AppData\Local\Temp\%s.jsonl" % name
    cur = None
    hits146 = 0
    first146 = None
    with open(path, encoding="utf-8") as f:
        f.readline()
        for line in f:
            e = json.loads(line)
            if e.get("ev") == "switch":
                cur = e["to"]
                continue
            if (e.get("ev") == "group" and e.get("script") == 126
                    and e.get("pc") == 146):
                if first146 is None:
                    first146 = (cur, hex(e["cmd"]))
                if e["cmd"] == 0x2B:
                    hits146 += 1
    out.append("%s s126 g146 hits=%d first=%s" % (name, hits146, first146))
with open(r"C:\Users\weiss\AppData\Local\Temp\pair_dbg.txt", "w", encoding="utf-8") as f:
    f.write("\n".join(out))
