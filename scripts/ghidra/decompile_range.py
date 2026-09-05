# -*- coding: utf-8 -*-
# 反编译指定地址范围的函数(含未被识别为函数的代码),输出到 ~/ghidra_all/<prog>/extra/
# 用法: analyzeHeadless ... -postScript decompile_range.py 0045c4d4 0045c5fc
# @category Yuris
import os
from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.program.model.address import AddressSet

prog = currentProgram
fm = prog.getFunctionManager()
monitor = ConsoleTaskMonitor()
outdir = os.path.join('/tmp', 'ghidra_all', prog.getName(), 'extra')
if not os.path.exists(outdir):
    os.makedirs(outdir)

args = getScriptArgs()
start = int(args[0], 16)
end = int(args[1], 16)

af = prog.getAddressFactory().getDefaultAddressSpace()
addr_start = af.getAddress(start)
addr_end = af.getAddress(end)

# 若该范围还不是函数,先创建
funcs = fm.getFunctionsContaining(addr_start)
if funcs.isEmpty():
    # 反汇编该范围再建函数
    dis = prog.getListing()
    from ghidra.app.cmd.disassemble import DisassembleCommand
    cmd = DisassembleCommand(addr_start, None, True)
    cmd.applyTo(prog, monitor)
    from ghidra.app.cmd.function import CreateFunctionCmd
    fcmd = CreateFunctionCmd("FUN_%08x" % start, addr_start, None, ghidra.program.model.symbol.SourceType.USER_DEFINED)
    fcmd.applyTo(prog, monitor)

f = fm.getFunctionContaining(addr_start)
print("function: %s" % f)
decomp = DecompInterface()
decomp.openProgram(prog)
res = decomp.decompileFunction(f, 300, monitor)
if res.decompileCompleted():
    code = res.getDecompiledFunction().getC()
    fname = "%08x_%s.c" % (f.getEntryPoint().getOffset(), f.getName().replace(' ', '_'))
    fh = open(os.path.join(outdir, fname), 'w')
    fh.write("// %s @ %s  size=%d\n" % (f.getName(), f.getEntryPoint(), f.getBody().getNumAddresses()))
    fh.write(code.encode('utf-8') if isinstance(code, unicode) else code)
    fh.close()
    print("WROTE %s" % os.path.join(outdir, fname))
else:
    print("DECOMPILE FAILED: %s" % res.getErrorMessage())
