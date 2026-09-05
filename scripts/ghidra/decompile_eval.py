# -*- coding: utf-8 -*-
# 在表达式求值器 thunk 地址批量创建函数并反编译(U2b)
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

# FUN_00468160 求值表赋值 + LET/IF 引用的全部 thunk
ADDRESSES = [
    "00420a90", "00420ba8", "00420bdc", "00420c0c",
    "00420c60", "00420c8c",
]

af = prog.getAddressFactory().getDefaultAddressSpace()
decomp = DecompInterface()
decomp.openProgram(prog)

created = 0
for h in ADDRESSES:
    addr = af.getAddress(int(h, 16))
    f = fm.getFunctionContaining(addr)
    if f is None or f.getEntryPoint().getOffset() != int(h, 16):
        cmd = DisassembleCommand(addr, None, True)
        cmd.applyTo(prog, monitor)
        fcmd = CreateFunctionCmd("EVAL_%s" % h, addr, None, SourceType.USER_DEFINED)
        if fcmd.applyTo(prog, monitor):
            created += 1
        f = fm.getFunctionContaining(addr)
    if f is None:
        print("FAIL create at %s" % h)
        continue
    res = decomp.decompileFunction(f, 300, monitor)
    if res.decompileCompleted():
        code = res.getDecompiledFunction().getC()
        fname = "%08x_%s.c" % (f.getEntryPoint().getOffset(), f.getName().replace(' ', '_'))
        fh = open(os.path.join(outdir, fname), 'w')
        fh.write("// %s @ %s  size=%d\n" % (f.getName(), f.getEntryPoint(), f.getBody().getNumAddresses()))
        fh.write(code.encode('utf-8') if isinstance(code, unicode) else code)
        fh.close()
        print("OK %s size=%d" % (fname, f.getBody().getNumAddresses()))
    else:
        print("FAIL decompile at %s: %s" % (h, res.getErrorMessage()))
print("CREATED %d" % created)
