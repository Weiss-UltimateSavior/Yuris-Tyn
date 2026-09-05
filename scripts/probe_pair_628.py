# -*- coding: utf-8 -*-
"""P5.2 —— 并排对比引擎/VM trace(VM 侧用 switch 跟踪脚本号),结果落盘。"""
import json

VM = r"C:\Users\weiss\AppData\Local\Temp\vm_v5.jsonl"
ENG = r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine\engine_trace_boot.jsonl"
OUT = r"C:\Users\weiss\AppData\Local\Temp\pair628.txt"


def collect(path, engine, anchor_pc, nth=1):
    cur = None
    seq = []
    hits = 0
    started = False
    with open(path, encoding="utf-8") as f:
        if not engine:
            f.readline()
        for line in f:
            e = json.loads(line)
            ev = e.get("ev")
            if ev == "switch":
                cur = e["to"]
                continue
            if ev != "group":
                continue
            sc = cur if engine is False else e.get("script")
            pc = e.get("pc")
            cmd = e.get("cmd")
            if isinstance(cmd, list):
                cmd = "<noop>"
            else:
                cmd = "%02x" % cmd
            if not started:
                if sc == 126 and pc == anchor_pc and cmd == "2b":
                    hits += 1
                    if hits == nth:
                        started = True
                        seq.append((sc, pc, cmd))
                continue
            seq.append((sc, pc, cmd))
            if len(seq) >= 26:
                break
    return seq


eng = collect(ENG, True, 146, 1)
vm = collect(VM, False, 146, 1)
n = max(len(eng), len(vm))
out = []
out.append("%22s | %s" % ("engine", "vm"))
for i in range(n):
    a = "%s s%s g%s" % (eng[i][2], eng[i][0], eng[i][1]) if i < len(eng) else ""
    b = "%s s%s g%s" % (vm[i][2], vm[i][0], vm[i][1]) if i < len(vm) else ""
    mark = "  <<<" if (a.split()[0] if a else "?") != (b.split()[0] if b else "?") else ""
    out.append("%22s | %-22s%s" % (a, b, mark))
with open(OUT, "w", encoding="utf-8") as f:
    f.write("\n".join(out))
