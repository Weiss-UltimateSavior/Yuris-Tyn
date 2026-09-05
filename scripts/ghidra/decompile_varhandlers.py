# -*- coding: utf-8 -*-
# 反编译表达式层变量类 opcode 处理器(pushvar/pushvarref/pushvaridx/arrayload/占位)
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

# FUN_0046a21c 运行期表(param==1)中的变量类处理器
ADDRESSES = [
    "00420ec4",  # 0x48 pushvar(值)
    "004218b0",  # 0x56 pushvarref(引用)
    "00421994",  # 0x76 pushvarindexed(下标)
    "00421a3c",  # 载入期占位(三变量类共用)
    "00421a4c",  # 0x29 arrayload
    "00420cb8",  # 0x4d pushstr
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
        fcmd = CreateFunctionCmd("VARH_%s" % h, addr, None, SourceType.USER_DEFINED)
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
