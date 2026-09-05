# -*- coding: utf-8 -*-
"""P5.2 —— 反编译 LOAD(0x36)处理器 00444648 及其内部调用,定性 YSSD 装载路径。"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\p5_sysvar"

# LOAD 处理器 + 可能相关的 YSSD/存档族(先只反编译主处理器,内部调用按需补)
TARGETS = ["00444648"]


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
        return None
    res = di.decompileFunction(f, 300, monitor)
    if not res.decompileCompleted():
        print("FAIL decompile %08x: %s" % (addr_int, res.getErrorMessage()))
        return None
    code = res.getDecompiledFunction().getC()
    name = "%08x_%s.c" % (f.getEntryPoint().getOffset(), f.getName().replace(' ', '_'))
    with open(os.path.join(outdir, name), 'w') as fh:
        fh.write("// %s @ %s  size=%d\n" % (f.getName(), f.getEntryPoint(),
                                            f.getBody().getNumAddresses()))
        if not isinstance(code, str):
            code = code.encode('utf-8')
        fh.write(code)
    print("OK %s" % name)
    return name


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

    done = set()
    # 第一轮:主目标
    queue = [int(t, 16) for t in TARGETS]
    # 第二轮:主处理器内部直接调用的 FUN_(去重、只做一层)
    inner = []
    for a in list(queue):
        f = fm.getFunctionContaining(af.getAddress(a))
        if f is None:
            continue
        for callee in f.getCalledFunctions(monitor):
            ep = callee.getEntryPoint().getOffset()
            if 0x400000 <= ep < 0x600000 and ep not in queue:
                inner.append(ep)
    queue.extend(inner)

    for a in queue:
        if a in done:
            continue
        done.add(a)
        decompile(di, fm, af, monitor, a, OUTDIR)
    print("TOTAL %d functions -> %s" % (len(done), OUTDIR))
