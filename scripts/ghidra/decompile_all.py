# -*- coding: utf-8 -*-
# 反编译全部函数到文件
# @category Yuris
import os
from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor

prog = currentProgram
fm = prog.getFunctionManager()
monitor = ConsoleTaskMonitor()
outdir = os.path.join(os.environ.get('HOME', '/tmp'), 'ghidra_all', prog.getName())
if not os.path.exists(outdir):
    os.makedirs(outdir)

decomp = DecompInterface()
decomp.openProgram(prog)

n = 0
for f in fm.getFunctions(True):
    res = decomp.decompileFunction(f, 180, monitor)
    if res.decompileCompleted():
        code = res.getDecompiledFunction().getC()
        fname = "%08x_%s.c" % (f.getEntryPoint().getOffset(), f.getName().replace(' ', '_'))
        fh = open(os.path.join(outdir, fname), 'w')
        fh.write("// %s @ %s  size=%d\n" % (f.getName(), f.getEntryPoint(), f.getBody().getNumAddresses()))
        fh.write(code.encode('utf-8') if isinstance(code, unicode) else code)
        fh.close()
        n += 1
print("DECOMPILED %d functions -> %s" % (n, outdir))
