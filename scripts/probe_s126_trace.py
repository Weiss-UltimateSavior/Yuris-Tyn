# -*- coding: utf-8 -*-
"""P5.2 —— VM trace:s126 pc140-155 的 group+call 事件(首次)。"""
import json

path = r"C:\Users\weiss\AppData\Local\Temp\vm_v5.jsonl"
cur = None
win = []
printed = False
with open(path, encoding="utf-8") as f:
    f.readline()
    for line in f:
        e = json.loads(line)
        ev = e.get("ev")
        if ev == "switch":
            cur = e["to"]
            continue
        if cur != 126:
            continue
        if printed:
            break
        if ev == "group":
            pc = e["pc"]
            if 138 <= pc <= 155:
                win.append(("g", pc, hex(e["cmd"])))
                if pc >= 150:
                    printed = True
        elif ev == "call":
            win.append(("call", e["from"], e["to"]))

print("VM s126 first pass:")
for w in win:
    print("  ", w)
