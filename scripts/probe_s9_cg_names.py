# -*- coding: utf-8 -*-
"""P7.1 —— 从 VM trace 提取 s9 的 CG/CGACT/CGINFO/CGEND 事件名序列(pc 800-1400)。"""
import json

VM = r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine\vm_trace_boot.jsonl"

cur = None
n = 0
with open(VM, encoding="utf-8") as f:
    f.readline()
    for line in f:
        e = json.loads(line)
        ev = e.get("ev")
        if ev == "switch":
            cur = e["to"]
            continue
        if cur != 9:
            continue
        if ev not in ("cg", "cgact", "cginfo", "cgend"):
            continue
        pc = e.get("pc")
        if not (800 <= pc <= 1390):
            continue
        if ev == "cg":
            print("pc%-5d CG   id=%r pos=%r" % (pc, e.get("id"), e.get("position")))
        elif ev == "cgact":
            slots = e.get("ev_", [])
            print("pc%-5d CGACT id=%r %s" % (pc, e.get("id"), slots[:3]))
        elif ev == "cginfo":
            slots = e.get("ev_", [])
            print("pc%-5d CGINFO id=%r %s" % (pc, e.get("id"), slots[-2:]))
        elif ev == "cgend":
            print("pc%-5d CGEND id=%r" % (pc, e.get("id")))
        n += 1
        if n > 120:
            break
