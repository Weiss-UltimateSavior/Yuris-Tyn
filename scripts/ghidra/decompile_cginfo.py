# -*- coding: utf-8 -*-
# 反编译 CGINFO 处理器 LAB_0043b084
# @category Yuris
import os
from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.program.model.symbol import SourceType

prog = currentProgram
fm = prog.getFunctionManager()
monitor = ConsoleTaskMonitor()
outdir = os.path.join('/tmp', 'ghidra_all', prog.getName(), 'extra')
if not os.path.exists(outdir):
    os.makedirs(outdir)

for h in ["00443674"]:
    addr = prog.getAddressFactory().getDefaultAddressSpace().getAddress(int(h, 16))
    f = fm.getFunctionContaining(addr)
    if f is None or f.getEntryPoint().getOffset() != int(h, 16):
        cmd = DisassembleCommand(addr, None, True)
        cmd.applyTo(prog, monitor)
        fcmd = CreateFunctionCmd("CMDH_%s" % h, addr, None, SourceType.USER_DEFINED)
        fcmd.applyTo(prog, monitor)
        f = fm.getFunctionContaining(addr)
    if f is None:
        print("FAIL create at %s" % h)
        continue
    res = DecompInterface()
    res.openProgram(prog)
    r = res.decompileFunction(f, 300, monitor)
    if r.decompileCompleted():
        code = r.getDecompiledFunction().getC()
        fname = "%08x_%s.c" % (f.getEntryPoint().getOffset(), f.getName().replace(' ', '_'))
        fh = open(os.path.join(outdir, fname), 'w')
        fh.write("// %s @ %s  size=%d\n" % (f.getName(), f.getEntryPoint(), f.getBody().getNumAddresses()))
        fh.write(code.encode('utf-8') if isinstance(code, unicode) else code)
        fh.close()
        print("OK %s size=%d" % (fname, f.getBody().getNumAddresses()))
    else:
        print("FAIL decompile %s" % h)
