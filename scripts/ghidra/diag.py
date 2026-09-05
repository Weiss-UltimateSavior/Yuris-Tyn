# -*- coding: utf-8 -*-
# 诊断: 枚举内存块 + 测试 findBytes
# @category Yuris
from ghidra.program.model.mem import MemoryAccessException

prog = currentProgram
mem = prog.getMemory()
print("IMAGE BASE:", prog.getImageBase())
print("=== memory blocks ===")
for b in mem.getBlocks():
    print("  %-20s %s - %s  size=%d  exec=%s write=%s init=%s" % (
        b.getName(), b.getStart(), b.getEnd(), b.getSize(),
        b.isExecute(), b.isWrite(), b.isInitialized()))

print("=== findBytes tests ===")
tests = [
    ("ascii_nospace", "YSTB"),
    ("hex_spaces", "59 53 54 42"),
    ("ascii_pct", "yst%05d"),
    ("and3", "83 e7 03"),
]
for label, pat in tests:
    start = prog.getMinAddress()
    hits = []
    while len(hits) < 5:
        a = findBytes(start, pat)
        if a is None:
            break
        hits.append(a)
        start = a.add(1)
    print("  %-14s -> %d hits: %s" % (label, len(hits), [str(h) for h in hits]))

fm = currentProgram.getFunctionManager()
print("function count:", fm.getFunctionCount())
