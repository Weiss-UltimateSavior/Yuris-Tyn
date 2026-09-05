# -*- coding: utf-8 -*-
"""P2 —— 缺口命令处理器批量反编译(直方图定序,见 probe_runtime_hist.py)。

含启动链 FUN_00463714/004637d8(@53/$55 等系统数组疑似写入者)。
输出: docs/reverse/decompiled/engine/
"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine"

TARGETS = [
    (0x00463714, "boot_init_a"),
    (0x004637D8, "boot_init_b"),
    (0x00441C08, "CMD_FONTINFO_0x1b"),
    (0x00445DD4, "CMD_MATH_0x3c"),
    (0x0043EB28, "CMD_FILEINFO_0x15"),
    (0x0043DA80, "CMD_FILEACT_0x14"),
    (0x004583BC, "CMD_WINDOWINFO_0x6b"),
    (0x0044955C, "CMD_MOUSE_0x45"),
    (0x00441338, "CMD_FONT_0x1a"),
    (0x0044F968, "CMD_SYSTEM_0x5d"),
    (0x004699CC, "CMD_ERROR_0x0e"),
    (0x004426A4, "CMD_FPS_0x1c"),
    (0x00443480, "CMD_INPUT_0x31"),
    (0x004411FC, "CMD_FLT_0x19"),
]

pyghidra.start()
with pyghidra.open_program(BINARY, project_location=PROJECT_LOC,
                           project_name=PROJECT, analyze=True) as flat:
    prog = flat.getCurrentProgram()
    af = prog.getAddressFactory().getDefaultAddressSpace()
    fm = prog.getFunctionManager()

    from ghidra.app.decompiler import DecompInterface
    from ghidra.app.cmd.function import CreateFunctionCmd
    from ghidra.app.cmd.disassemble import DisassembleCommand
    from ghidra.program.model.symbol import SourceType
    from ghidra.util.task import ConsoleTaskMonitor

    di = DecompInterface()
    di.openProgram(prog)
    monitor = ConsoleTaskMonitor()

    for addr_val, tag in TARGETS:
        addr = af.getAddress(addr_val)
        f = fm.getFunctionAt(addr)
        if f is None:
            if not fm.getFunctionContaining(addr):
                DisassembleCommand(addr, None, True).applyTo(prog, monitor)
            CreateFunctionCmd("CMDH_%08x" % addr_val, addr, None,
                              SourceType.USER_DEFINED).applyTo(prog, monitor)
            f = fm.getFunctionAt(addr)
        if f is None:
            print("NO FUNC %08x %s" % (addr_val, tag))
            continue
        res = di.decompileFunction(f, 180, monitor)
        if not res.decompileCompleted():
            print("DECOMP FAIL %08x %s" % (addr_val, tag))
            continue
        c = res.getDecompiledFunction().getC()
        path = os.path.join(OUTDIR, "%08x_%s.c" % (addr_val, tag))
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(c)
        print("OK %08x %s (%d bytes)" % (addr_val, tag, len(c)))

print("DONE")
