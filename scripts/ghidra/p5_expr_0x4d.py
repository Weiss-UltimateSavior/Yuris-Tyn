# -*- coding: utf-8 -*-
"""P5.2 —— 反编译 0x4d 表达式处理器(007e1660[op=0x4d] 目标)。"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\p5_sysvar"

pyghidra.start()
with pyghidra.open_program(BINARY, project_location=PROJECT_LOC,
                           project_name=PROJECT, analyze=True) as flat:
    prog = flat.getCurrentProgram()
    af = prog.getAddressFactory().getDefaultAddressSpace()
    fm = prog.getFunctionManager()
    monitor = ConsoleTaskMonitor() if False else None

    from ghidra.app.decompiler import DecompInterface
    from ghidra.util.task import ConsoleTaskMonitor
    from ghidra.program.model.symbol import RefType

    di = DecompInterface()
    di.openProgram(prog)
    monitor = ConsoleTaskMonitor()

    # 1) 找写 007e1660 的代码(表初始化器)
    addr = af.getAddress(0x007e1660)
    refs = prog.getReferenceManager().getReferencesTo(addr)
    writers = []
    for r in refs:
        ft = r.getReferenceType()
        if ft.isWrite() or "WRITE" in str(ft):
            writers.append(r.getFromAddress().getOffset())
    print("writers of 007e1660:", [hex(x) for x in writers])

    # 2) 反编译写者,提取 0x4d 项
    seen = set()
    for w in writers[:4]:
        f = fm.getFunctionContaining(af.getAddress(w))
        if f is None or f.getEntryPoint().getOffset() in seen:
            continue
        seen.add(f.getEntryPoint().getOffset())
        res = di.decompileFunction(f, 240, monitor)
        if res.decompileCompleted():
            c = res.getDecompiledFunction().getC()
            path = os.path.join(OUTDIR, "%08x_expr_table_init.c" % f.getEntryPoint().getOffset())
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(c)
            print("OK %08x (%d bytes)" % (f.getEntryPoint().getOffset(), len(c)))
        else:
            print("DECOMP FAIL %08x" % w)

print("DONE")
