# -*- coding: utf-8 -*-
"""P1 对拍分歧定性 —— 反编译运行期 INT/STR 处理器。

对拍实锤:引擎运行期真实执行 cmd 0x32(INT, 0x443538)与 0x5c(STR, 0x451598),
VM 现按 Declaration no-op 处理 → 190 pc29 的 IF 条件分歧(引擎真,VM 假)。
本脚本反编译两个处理器定性其运行期副作用。
"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine"

TARGETS = [
    (0x00443538, "CMD_INT_runtime"),
    (0x00451598, "CMD_STR_runtime"),
]

pyghidra.start()
with pyghidra.open_program(BINARY, project_location=PROJECT_LOC,
                           project_name=PROJECT, analyze=True) as flat:
    prog = flat.getCurrentProgram()
    af = prog.getAddressFactory().getDefaultAddressSpace()
    fm = prog.getFunctionManager()

    from ghidra.app.decompiler import DecompInterface
    from ghidra.app.cmd.function import CreateFunctionCmd
    from ghidra.app.cmd.disassemble import DisassembleCommand
    from ghidra.program.model.symbol import SourceType
    from ghidra.util.task import ConsoleTaskMonitor

    di = DecompInterface()
    di.openProgram(prog)
    monitor = ConsoleTaskMonitor()

    for addr_val, tag in TARGETS:
        addr = af.getAddress(addr_val)
        f = fm.getFunctionAt(addr)
        if f is None:
            if not fm.getFunctionContaining(addr):
                DisassembleCommand(addr, None, True).applyTo(prog, monitor)
            CreateFunctionCmd("CMDH_%08x" % addr_val, addr, None,
                              SourceType.USER_DEFINED).applyTo(prog, monitor)
            f = fm.getFunctionAt(addr)
        if f is None:
            print("NO FUNC %08x %s" % (addr_val, tag))
            continue
        res = di.decompileFunction(f, 180, monitor)
        if not res.decompileCompleted():
            print("DECOMP FAIL %08x %s" % (addr_val, tag))
            continue
        path = os.path.join(OUTDIR, "%08x_%s.c" % (addr_val, tag))
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(res.getDecompiledFunction().getC())
        print("OK %08x %s (%d bytes) -> %s"
              % (addr_val, tag, len(res.getDecompiledFunction().getC()), path))

print("DONE")
