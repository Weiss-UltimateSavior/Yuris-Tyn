# -*- coding: utf-8 -*-
"""P5.2 —— 引擎 trace s126 pc63-67 原始事件(seq 相邻性)。"""
import json

ENG = r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine\engine_trace_boot.jsonl"
OUT = r"C:\Users\weiss\AppData\Local\Temp\eng64.txt"

lines = []
with open(ENG, encoding="utf-8") as f:
    prev = None
    for line in f:
        e = json.loads(line)
        if e.get("ev") != "group":
            continue
        if e.get("script") == 126 and 60 <= e.get("pc", -1) <= 70:
            lines.append("seq=%s pc=%s cmd=%s handler=%s"
                         % (e.get("seq"), e.get("pc"), e.get("cmd"),
                            e.get("handler", "?")))
            if len(lines) > 40:
                break
with open(OUT, "w", encoding="utf-8") as f:
    f.write("\n".join(lines))
