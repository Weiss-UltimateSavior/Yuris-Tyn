# -*- coding: utf-8 -*-
"""P1 —— 引擎完整性校验器定位(PyGhidra)。

对照实验结论:挂调试器不下断点 → 进程存活;下 int3 → 引擎在 0x4000001F
(异常地址恰为被 patch 的处理器入口)自杀 → 引擎对代码段做完整性校验。
本脚本:
  1) 全程序扫描立即数 0x4000001F(自定义异常码)的引用 → raise 站点;
  2) 反汇编 LET 入口 0x443808 前若干条(异常地址 = bp 地址,需确认语义);
  3) 反编译启动链第一个函数 FUN_0046bdb8(疑似自检);
输出到 docs/reverse/decompiled/engine/hook_recon/。
"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\hook_recon"

FATAL_CODE = 0x4000001F
LET_ENTRY = 0x00443808
BOOT_FIRST = 0x0046BDB8

pyghidra.start()
with pyghidra.open_program(BINARY, project_location=PROJECT_LOC,
                           project_name=PROJECT, analyze=True) as flat:
    prog = flat.getCurrentProgram()
    af = prog.getAddressFactory().getDefaultAddressSpace()
    fm = prog.getFunctionManager()
    listing = prog.getListing()

    from ghidra.app.decompiler import DecompInterface
    from ghidra.util.task import ConsoleTaskMonitor
    from ghidra.program.model.scalar import Scalar
    di = DecompInterface()
    di.openProgram(prog)
    monitor = ConsoleTaskMonitor()

    # ---- 1) 扫描 0x4000001F 立即数 ----
    print("== 0x%08x 立即数引用 ==" % FATAL_CODE)
    hits = []
    fi = listing.getInstructions(True)
    for ins in fi:
        for oi in range(ins.getNumOperands()):
            for o in ins.getOpObjects(oi):
                if isinstance(o, Scalar) and o.getUnsignedValue() == FATAL_CODE:
                    hits.append(ins.getAddress())
                    print("  %s  %s" % (ins.getAddress(), ins))
    # 反编译 raise 站点所在函数
    for a in hits:
        f = fm.getFunctionContaining(a)
        if f:
            res = di.decompileFunction(f, 120, monitor)
            if res.decompileCompleted():
                path = os.path.join(OUTDIR, "recon_%08x_fatal_raise.c"
                                    % f.getEntryPoint().getOffset())
                with open(path, "w", encoding="utf-8") as fh:
                    fh.write(res.getDecompiledFunction().getC())
                print("  raise 函数 %s -> %s" % (f.getName(), path))

    # ---- 2) LET 入口反汇编 ----
    print("== LET 0x%08x 前若干条 ==" % LET_ENTRY)
    addr = af.getAddress(LET_ENTRY)
    ins = listing.getInstructionAt(addr)
    n = 0
    while ins is not None and n < 12:
        print("  %s  %s" % (ins.getAddress(), ins))
        addr = ins.getMaxAddress()
        ins = listing.getInstructionAfter(addr)
        n += 1

    # ---- 3) 启动链第一函数 ----
    f = fm.getFunctionAt(af.getAddress(BOOT_FIRST))
    if f is None:
        print("NO FUNC %08x" % BOOT_FIRST)
    else:
        res = di.decompileFunction(f, 120, monitor)
        if res.decompileCompleted():
            path = os.path.join(OUTDIR, "recon_%08x_boot_first.c" % BOOT_FIRST)
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(res.getDecompiledFunction().getC())
            print("OK %08x boot_first -> %s" % (BOOT_FIRST, path))

print("DONE")
