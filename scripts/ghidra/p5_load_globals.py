# -*- coding: utf-8 -*-
"""P5.2 —— 定性 LOAD 相关全局:DAT_0059b718 / DAT_00871fe8 / DAT_008724d8 / DAT_00872504 的写入者。"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"

pyghidra.start()
with pyghidra.open_program(BINARY, project_location=PROJECT_LOC,
                           project_name=PROJECT, analyze=True) as flat:
    prog = flat.getCurrentProgram()
    af = prog.getAddressFactory().getDefaultAddressSpace()
    fm = prog.getFunctionManager()
    rm = prog.getReferenceManager()

    for sym in ["DAT_0059b718", "DAT_00871fe8", "DAT_008724d8", "DAT_00872504",
                "DAT_00871fe4", "DAT_0059b718"]:
        # 按名字找符号
        syms = list(prog.getSymbolTable().getSymbols(sym))
        if not syms:
            # 也可能是未命名 DAT,按地址解析
            try:
                addr_int = int(sym.replace("DAT_", ""), 16)
                addr = af.getAddress(addr_int)
                refs = rm.getReferencesTo(addr)
                print("== %s (addr) ==" % sym)
                for r in refs:
                    f = fm.getFunctionContaining(r.getFromAddress())
                    print("   %s  from %s  in %s" % (r.getReferenceType(), r.getFromAddress(),
                                                    f.getName() if f else "?"))
            except Exception as e:
                print("== %s: no symbol (%s)" % (sym, e))
            continue
        for s in syms:
            addr = s.getAddress()
            print("== %s @ %s ==" % (sym, addr))
            for r in rm.getReferencesTo(addr):
                f = fm.getFunctionContaining(r.getFromAddress())
                print("   %s  from %s  in %s" % (r.getReferenceType(), r.getFromAddress(),
                                                f.getName() if f else "?"))
