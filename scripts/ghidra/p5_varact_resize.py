# -*- coding: utf-8 -*-
"""P5.2 —— 反编译数组 resize 与 VARINFO 处理器(定性 desc+4 边界语义)。"""
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

    from ghidra.app.decompiler import DecompInterface
    from ghidra.app.cmd.function import CreateFunctionCmd
    from ghidra.app.cmd.disassemble import DisassembleCommand
    from ghidra.program.model.symbol import SourceType
    from ghidra.util.task import ConsoleTaskMonitor

    di = DecompInterface()
    di.openProgram(prog)
    monitor = ConsoleTaskMonitor()

    for addr_val, tag in [
        (0x00454710, "array_resize"),
        (0x004550A0, "CMDH_varinfo_0x67"),
    ]:
        addr = af.getAddress(addr_val)
        f = fm.getFunctionAt(addr)
        if f is None:
            if not fm.getFunctionContaining(addr):
                DisassembleCommand(addr, None, True).applyTo(prog, monitor)
            CreateFunctionCmd("P5_%08x" % addr_val, addr, None,
                              SourceType.USER_DEFINED).applyTo(prog, monitor)
            f = fm.getFunctionAt(addr)
        if f is None:
            print("NO FUNC %08x %s" % (addr_val, tag))
            continue
        res = di.decompileFunction(f, 300, monitor)
        if not res.decompileCompleted():
            print("DECOMP FAIL %08x %s" % (addr_val, tag))
            continue
        code = res.getDecompiledFunction().getC()
        fname = "%08x_%s.c" % (addr_val, tag)
        with open(os.path.join(OUTDIR, fname), "w", encoding="utf-8") as fh:
            fh.write("// %s @ %s size=%d\n" % (f.getName(), f.getEntryPoint(),
                                               f.getBody().getNumAddresses()))
            fh.write(code)
        print("WROTE %s" % fname)
