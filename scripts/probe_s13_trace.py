#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""P5.2 —— 抽取 VM trace 中 script 13 的组序列与 call 事件。"""
import json

path = r"C:\Users\weiss\AppData\Local\Temp\vm_final.jsonl"
cur = None
s13 = []
calls_near = []
found = False
with open(path, encoding="utf-8") as f:
    f.readline()
    for line in f:
        e = json.loads(line)
        ev = e.get("ev")
        if ev == "switch":
            cur = e["to"]
            if cur == 13:
                calls_near.append(("switch->13", e))
        elif ev == "group":
            if cur == 13:
                s13.append((e["pc"], hex(e["cmd"])))
                if len(s13) >= 14:
                    found = True
                    break
            elif e["pc"] == 476 and cur == 190:
                calls_near.append(("s190pc476", hex(e["cmd"])))
        elif ev == "call" and cur == 13:
            calls_near.append(("call", e))

print("calls near:", calls_near[-6:])
print("s13 groups:", s13)
