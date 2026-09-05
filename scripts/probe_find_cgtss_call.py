#!/usr/bin/env python3
"""P5.2 —— 在 VM trace 中定位 s22 g462(es.CGTSS) 入口前的调用点。"""
import json
import sys

path = sys.argv[1] if len(sys.argv) > 1 else r"C:\Users\weiss\AppData\Local\Temp\vm_trace_fresh2.jsonl"
cur = None
groups = []   # (script, pc, cmd)
calls = []    # (caller_script, from_pc, to_pc)
with open(path, encoding="utf-8") as f:
    f.readline()
    for line in f:
        e = json.loads(line)
        ev = e.get("ev")
        if ev == "switch":
            cur = e["to"]
        elif ev in ("group", "decl"):
            g = (cur, e["pc"], e["cmd"])
            groups.append(g)
            if g == (22, 462, 0x5C):
                print("=== 到达 s22 g462(es.CGTSS) ===")
                for p in groups[-16:]:
                    print("   ", p)
                print("近期 call 事件:")
                for c in calls[-8:]:
                    print("   call", c)
                break
            if len(groups) > 500:
                groups = groups[-200:]
        elif ev == "call":
            calls.append((cur, e["from"], e["to"]))
            if len(calls) > 500:
                calls = calls[-200:]
