# -*- coding: utf-8 -*-
"""P5.2 —— 反编译 YSSNP.DLL 导出(YSSnp_Compress/Uncompress/Length),定性 SNP 编码。"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_snp"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\YSSNP.DLL"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\p5_sysvar"

pyghidra.start()
with pyghidra.open_program(BINARY, project_location=PROJECT_LOC,
                           project_name=PROJECT, analyze=True) as flat:
    prog = flat.getCurrentProgram()
    af = prog.getAddressFactory().getDefaultAddressSpace()
    fm = prog.getFunctionManager()
    st = prog.getSymbolTable()

    from ghidra.app.decompiler import DecompInterface
    from ghidra.util.task import ConsoleTaskMonitor
    di = DecompInterface()
    di.openProgram(prog)
    monitor = ConsoleTaskMonitor()

    # 列出导出符号
    for s in st.getAllSymbols(True):
        if s.getSymbolType().toString() == "Function" and not s.getName().startswith("FUN_"):
            print("EXPORT: %s @ %s" % (s.getName(), s.getAddress()))

    # 反编译全部外部符号指向的函数 + 入口点
    targets = set()
    for ep in prog.getSymbolTable().getExternalEntryPointIterator():
        targets.add(ep.getOffset())
    print("entry points: %s" % [hex(t) for t in sorted(targets)])
    for t in sorted(targets):
        f = fm.getFunctionContaining(af.getAddress(t))
        if f is None:
            continue
        res = di.decompileFunction(f, 120, monitor)
        if res.decompileCompleted():
            code = res.getDecompiledFunction().getC()
            if not isinstance(code, str):
                code = code.encode('utf-8')
            name = "%08x_%s.c" % (f.getEntryPoint().getOffset(),
                                  f.getName().replace(' ', '_'))
            with open(os.path.join(OUTDIR, "snp_" + name), 'w') as fh:
                fh.write("// YSSNP.DLL %s @ %s  size=%d\n" % (
                    f.getName(), f.getEntryPoint(), f.getBody().getNumAddresses()))
                fh.write(code)
            print("OK snp_%s" % name)
