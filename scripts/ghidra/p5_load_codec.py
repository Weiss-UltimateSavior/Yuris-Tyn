# -*- coding: utf-8 -*-
"""P5.2 —— 反压缩器初始化 FUN_00467488 + 压缩侧用户,定性 SNP 编码。"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\p5_sysvar"
TARGETS = ["00467488", "00418b18", "0041b7f0", "0041f754", "0041f7e4", "0044cb9d"]


def decompile(di, fm, af, monitor, addr_int, outdir):
    from ghidra.app.cmd.function import CreateFunctionCmd
    from ghidra.app.cmd.disassemble import DisassembleCommand
    from ghidra.program.model.symbol import SourceType
    addr = af.getAddress(addr_int)
    f = fm.getFunctionContaining(addr)
    if f is None or f.getEntryPoint().getOffset() != addr_int:
        cmd = DisassembleCommand(addr, None, True)
        cmd.applyTo(prog, monitor)
        fcmd = CreateFunctionCmd("FUN_%08x" % addr_int, addr, None,
                                 SourceType.USER_DEFINED)
        fcmd.applyTo(prog, monitor)
        f = fm.getFunctionContaining(addr)
    if f is None:
        print("FAIL create at %08x" % addr_int)
        return
    res = di.decompileFunction(f, 300, monitor)
    if not res.decompileCompleted():
        print("FAIL decompile %08x: %s" % (addr_int, res.getErrorMessage()))
        return
    code = res.getDecompiledFunction().getC()
    if not isinstance(code, str):
        code = code.encode('utf-8')
    name = "%08x_%s.c" % (f.getEntryPoint().getOffset(), f.getName().replace(' ', '_'))
    with open(os.path.join(outdir, name), 'w') as fh:
        fh.write("// %s @ %s  size=%d\n" % (f.getName(), f.getEntryPoint(),
                                            f.getBody().getNumAddresses()))
        fh.write(code)
    print("OK %s" % name)


pyghidra.start()
with pyghidra.open_program(BINARY, project_location=PROJECT_LOC,
                           project_name=PROJECT, analyze=True) as flat:
    prog = flat.getCurrentProgram()
    af = prog.getAddressFactory().getDefaultAddressSpace()
    fm = prog.getFunctionManager()

    from ghidra.app.decompiler import DecompInterface
    from ghidra.util.task import ConsoleTaskMonitor
    di = DecompInterface()
    di.openProgram(prog)
    monitor = ConsoleTaskMonitor()

    for t in TARGETS:
        decompile(di, fm, af, monitor, int(t, 16), OUTDIR)
