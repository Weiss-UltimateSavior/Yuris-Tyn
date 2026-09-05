# -*- coding: utf-8 -*-
"""P7.1 —— 提取两侧 trace 中 script 9 的组执行流(到首处分歧为止),对比结构。"""
import json
import sys

ENG = r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine\engine_trace_boot.jsonl"
VM = r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine\vm_trace_boot.jsonl"
LIMIT = 138400  # 只取到首处分歧(pc230 族)之后一点


def collect(path, engine):
    seq = []
    cur = None
    with open(path, encoding="utf-8") as f:
        if not engine:
            f.readline()  # meta 行
        for line in f:
            e = json.loads(line)
            ev = e.get("ev")
            if ev == "switch":
                cur = e["to"]
                continue
            if engine:
                if ev != "group":
                    continue
                sc = e.get("script")
                cmd = e.get("cmd")
            else:
                if ev == "group":
                    sc, cmd = cur, e.get("cmd")
                elif ev in ("decl", "unsupported", "varquery", "sub"):
                    sc, cmd = cur, e.get("cmd")
                elif ev in ("cg", "cgact", "cginfo", "cgend", "text", "sound",
                            "load", "save"):
                    sc, cmd = cur, {"cg": 0x01, "cgact": 0x02, "cginfo": 0x04,
                                   "cgend": 0x03, "text": 0x62, "sound": 0x59,
                                   "load": 0x36, "save": 0x56}[ev]
                else:
                    continue
            if isinstance(cmd, list):
                continue
            if sc != 9:
                continue
            seq.append((e.get("pc"), cmd))
            if len(seq) >= LIMIT:
                break
    return seq


def runlen(seq):
    out = []
    for pc, cmd in seq:
        if out and out[-1][0] == pc:
            out[-1][1] += 1
        else:
            out.append([pc, cmd, 1])
    return out


eng = collect(ENG, True)
vm = collect(VM, False)
print("engine s9 groups:", len(eng), " vm s9 groups:", len(vm))

re_, rv = runlen(eng), runlen(vm)
# 并排打印(对齐 run 边界以 engine 为准,vm 超出部分单独列出)
print("%-28s | %-28s" % ("engine (pc cmd xN)", "vm (pc cmd xN)"))
i = j = 0
first_shown = 0
while i < len(re_) or j < len(rv):
    a = "%4d %02x x%-6d" % (re_[i][0], re_[i][1], re_[i][2]) if i < len(re_) else ""
    b = "%4d %02x x%-6d" % (rv[j][0], rv[j][1], rv[j][2]) if j < len(rv) else ""
    # 简单同步:engine 与 vm 的 run 逐个对(首处分歧前应完全一致)
    mark = ""
    if a.split()[0] != b.split()[0] if a and b else bool(a) != bool(b):
        mark = "  <<<"
    print("%-28s | %-28s%s" % (a, b, mark))
    i += 1
    j += 1
    if mark and i > first_shown + 30:
        print("...(首个分歧后 30 run 截断)")
        break
    if mark and not first_shown:
        first_shown = i
