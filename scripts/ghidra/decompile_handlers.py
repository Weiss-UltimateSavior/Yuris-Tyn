# -*- coding: utf-8 -*-
# 在命令处理器地址批量创建函数并反编译(补 Ghidra 漏识别的函数)
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

# FUN_0046305c 初始化的全部处理器地址(0x78b020 表)
HANDLERS = """0045c4d4 0045c4ec 00423080 00423864 0042607c 0043ad14 0043b084 0043bfec
0043c12c 0043c2a0 0043c71c 0043c984 0043d34c 0043d400 0043da44 004699cc 004699ac
0043da80 0043eb28 00440a7c 00440e4c 00440e9c 004411fc 00441338 00441c08 004426a4
0044272c 004428c0 004431ec 004432e4 00443324 004433ac 00443434 00443480 00443538
00443668 00443674 00443808 00444648 00445a00 00445b9c 00445c54 00445ce8 00445dd4
004466bc 00448bb0 00448e90 00449030 0044491ac 004494fc 0044951c 0044953c 0044955c
00449c4c 0044a238 0044a28c 0044a6d8 0044ad64 0044ad88 0044b024 0044b044 0044b418
0044b6a0 0044b6f0 0044da98 0044dac0 0044dae0 0044e9d0 0044ebe4 0044f804 0044f968
0044fd30 0044fe40 00451598 00451838 00452888 00452bd0 00452bf0 00453158 00453178
004550a0 00455d58 00455de4 00457ce4 00457d64 004583bc 004584a8 00458eb0 00458fe8
00455b78 00459508""".split()
HANDLERS = [h for h in HANDLERS if h.startswith("0044") or h.startswith("0045") or h.startswith("0046") or h.startswith("0042") or h.startswith("0043")]

af = prog.getAddressFactory().getDefaultAddressSpace()
decomp = DecompInterface()
decomp.openProgram(prog)

created = 0
for h in HANDLERS:
    addr = af.getAddress(int(h, 16))
    f = fm.getFunctionContaining(addr)
    if f is None or f.getEntryPoint().getOffset() != int(h, 16):
        cmd = DisassembleCommand(addr, None, True)
        cmd.applyTo(prog, monitor)
        fcmd = CreateFunctionCmd("CMDH_%s" % h, addr, None, SourceType.USER_DEFINED)
        ok = fcmd.applyTo(prog, monitor)
        if ok:
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
    else:
        print("FAIL decompile at %s: %s" % (h, res.getErrorMessage()))
print("CREATED %d functions, output -> %s" % (created, outdir))
