# -*- coding: utf-8 -*-
"""P5 —— vm trace 行号 → 对齐组下标映射,定位 [2091] 前后的 LABELINFO。"""
import json
import sys

path = r"C:\Users\weiss\AppData\Local\Temp\vm_v8.jsonl"
import os
LO = int(os.environ.get("LO", "2080")); HI = int(os.environ.get("HI", "2100"))
SEMANTIC = {"text": 0x62, "cg": 0x01, "sound": 0x59, "cgact": 0x02,
            "cginfo": 0x04, "cgend": 0x03, "load": 0x36, "save": 0x56}

idx = 0
cur = None
rows = []          # (group_index, line_no, ev, extra)
with open(path, "r", encoding="utf-8") as f:
    for ln, line in enumerate(f, 1):
        line = line.strip()
        if not line:
            continue
        e = json.loads(line)
        ev = e.get("ev")
        if ev == "meta":
            cur = e.get("entry_script")
            continue
        if ev == "switch":
            cur = e["to"]
            continue
        if ev == "group":
            rows.append((idx, ln, ev, (cur, e["pc"], e["cmd"])))
            idx += 1
        elif ev in SEMANTIC:
            rows.append((idx, ln, ev, (cur, e["pc"], SEMANTIC[ev])))
            idx += 1
        elif ev in ("decl", "unsupported", "varquery", "sub"):
            rows.append((idx, ln, ev, (cur, e["pc"], e["cmd"], e.get("ev_"))))
            idx += 1

print("total groups:", idx)
# [2091] 前后窗口
for r in rows:
    gi = r[0]
    if LO <= gi <= HI:
        print(r)
# 全部 varquery 的下标
print("--- varquery indices ---")
for r in rows:
    if r[2] == "varquery":
        print(r[0], r[1], r[3])
