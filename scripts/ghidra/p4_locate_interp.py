# -*- coding: utf-8 -*-
"""P4 —— 定位明文剧本解释器:锚点字符串 → 交叉引用 → 主循环/派发表反编译。

锚点(成果 20/P4 计划):行号报错 "invalid use of '%s'"、token 期待 $ = FUN_004cbd5f。
输出: docs/reverse/decompiled/engine/p4_interp/
"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\p4_interp"

ANCHOR_STRINGS = [b"invalid use of", b"scenario"]

pyghidra.start()
with pyghidra.open_program(BINARY, project_location=PROJECT_LOC,
                           project_name=PROJECT, analyze=True) as flat:
    prog = flat.getCurrentProgram()
    af = prog.getAddressFactory().getDefaultAddressSpace()
    fm = prog.getFunctionManager()
    listing = prog.getListing()

    from ghidra.app.decompiler import DecompInterface
    from ghidra.app.cmd.function import CreateFunctionCmd
    from ghidra.app.cmd.disassemble import DisassembleCommand
    from ghidra.program.model.symbol import RefType, SourceType
    from ghidra.util.task import ConsoleTaskMonitor

    di = DecompInterface()
    di.openProgram(prog)
    monitor = ConsoleTaskMonitor()

    # ---- 1) 找锚点字符串 ----
    hits = {}
    di_iter = listing.getDefinedData(True)
    for d in di_iter:
        v = d.getValue()
        if d.hasStringValue():
            s = str(v)
            for a in ANCHOR_STRINGS:
                if a.decode() in s:
                    hits.setdefault(s[:60], []).append(d.getAddress())
    for s, addrs in hits.items():
        for a in addrs:
            print("STR %s @ %s" % (s, a))

    # ---- 2) 字符串交叉引用 → 所在函数 ----
    funcs = set()
    for s, addrs in hits.items():
        for a in addrs:
            for ref in prog.getReferenceManager().getReferencesTo(a):
                fa = fm.getFunctionContaining(ref.getFromAddress())
                if fa:
                    funcs.add(fa.getEntryPoint().getOffset())
                    print("  xref %s from %s in %s" %
                          (a, ref.getFromAddress(), fa.getName()))
    print("锚点函数:", ["%08x" % f for f in sorted(funcs)])

    # ---- 3) 反编译锚点函数 + FUN_004cbd5f + 调用者一层 ----
    todo = set(funcs) | {0x004CBD5F}
    seen_callers = set()
    for faddr in sorted(todo):
        addr = af.getAddress(faddr)
        f = fm.getFunctionAt(addr)
        if f is None:
            if not fm.getFunctionContaining(addr):
                DisassembleCommand(addr, None, True).applyTo(prog, monitor)
            CreateFunctionCmd("P4_%08x" % faddr, addr, None,
                              SourceType.USER_DEFINED).applyTo(prog, monitor)
            f = fm.getFunctionAt(addr)
        if f is None:
            print("NO FUNC %08x" % faddr)
            continue
        res = di.decompileFunction(f, 240, monitor)
        if res.decompileCompleted():
            path = os.path.join(OUTDIR, "p4_%08x.c" % faddr)
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(res.getDecompiledFunction().getC())
            print("OK %08x %s" % (faddr, f.getName()))
            # 调用者一层
            for ref in prog.getReferenceManager().getReferencesTo(addr):
                fa = fm.getFunctionContaining(ref.getFromAddress())
                if fa and fa.getEntryPoint().getOffset() not in todo:
                    seen_callers.add(fa.getEntryPoint().getOffset())
        else:
            print("DECOMP FAIL %08x" % faddr)

    # 锚点函数的调用者(若锚点函数本身不是主循环,主循环在其调用者)
    for faddr in sorted(seen_callers):
        f = fm.getFunctionAt(af.getAddress(faddr))
        if f is None:
            continue
        res = di.decompileFunction(f, 240, monitor)
        if res.decompileCompleted():
            path = os.path.join(OUTDIR, "p4_%08x_caller.c" % faddr)
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(res.getDecompiledFunction().getC())
            print("OK caller %08x %s" % (faddr, f.getName()))

print("DONE")
