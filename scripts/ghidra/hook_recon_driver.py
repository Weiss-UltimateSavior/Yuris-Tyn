# -*- coding: utf-8 -*-
# P1 引擎真值 hook 前置侦察(PyGhidra 驱动):
#   1) 反编译 hook 设计所需关键函数
#   2) 扫 FUN_0046305c 的 MOV 提取 121 项处理器表
# 复用已分析好的 Ghidra 项目 D:/Dev/GhidraUser/yuris_p1(kemonomichi2.exe)
# 输出: docs/reverse/decompiled/engine/hook_recon/
import os

import pyghidra

GHIDRA_DIR = r"D:\Dev\ghidra_12.1.3_PUBLIC"
PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = "kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\hook_recon"
TABLE_BASE = 0x0078B020
INIT_FUNC = 0x0046305C

TARGETS = [
    (0x0040449C, "main_loop"),
    (0x0043BE6C, "task_rotate"),
    (0x00450DFD, "loader"),
    (0x00451348, "ysvr_apply"),
    (0x0046305C, "table_init"),
    (0x0044272C, "GO"),
    (0x004428C0, "GOSUB"),
    (0x0044B418, "RETURN"),
    (0x004431EC, "IF"),
    (0x00451724, "script_end"),
    (0x0046B63C, "boot_chain"),
    (0x00463C7C, "label_load"),
    (0x0045124C, "label_hash_find"),
]

os.makedirs(OUTDIR, exist_ok=True)

pyghidra.start()
with pyghidra.open_program(os.path.join(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2", BINARY),
                           project_location=PROJECT_LOC, project_name=PROJECT,
                           analyze=True) as ctx:
    flat = ctx
    prog = flat.getCurrentProgram()
    af = prog.getAddressFactory().getDefaultAddressSpace()
    fm = prog.getFunctionManager()

    from ghidra.app.decompiler import DecompInterface
    from ghidra.util.task import ConsoleTaskMonitor
    di = DecompInterface()
    di.openProgram(prog)
    monitor = ConsoleTaskMonitor()

    for addr_val, tag in TARGETS:
        f = fm.getFunctionAt(af.getAddress(addr_val))
        if f is None:
            print("NO FUNC at %08x %s" % (addr_val, tag))
            continue
        res = di.decompileFunction(f, 120, monitor)
        if not res.decompileCompleted():
            print("DECOMP FAIL %08x %s" % (addr_val, tag))
            continue
        c = res.getDecompiledFunction().getC()
        path = os.path.join(OUTDIR, "recon_%08x_%s.c" % (addr_val, tag))
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(c)
        print("OK %08x %s (%d bytes)" % (addr_val, tag, len(c)))

    # ---- 扫表 ----
    f = fm.getFunctionAt(af.getAddress(INIT_FUNC))
    assigns = {}
    listing = prog.getListing()
    insns = listing.getInstructions(f.getBody(), True)
    for ins in insns:
        if str(ins.getMnemonicString()).upper() != "MOV":
            continue
        disp = None
        for oi in range(ins.getNumOperands()):
            for o in ins.getOpObjects(oi):
                try:
                    v = o.getOffset()
                except AttributeError:
                    continue
                if v is not None and TABLE_BASE <= v < TABLE_BASE + 0x400:
                    disp = v
        if disp is None:
            continue
        for o in ins.getOpObjects(1):
            try:
                v = o.getOffset() & 0xFFFFFFFF
            except AttributeError:
                continue
            if 0x00400000 <= v < 0x00500000:
                assigns[disp] = v
    lines = ["# DAT_0078b020 处理器表(扫 %08x MOV 提取, %d 项)" % (INIT_FUNC, len(assigns)),
             "# 格式: index cmd 表偏移 处理器地址", ""]
    for disp in sorted(assigns):
        idx = (disp - TABLE_BASE) // 4
        cmd = idx - 8
        lines.append("%4d  cmd=%4d(0x%02x)  [%08x]  -> %08x"
                     % (idx, cmd, cmd & 0xFF, disp, assigns[disp]))
    with open(os.path.join(OUTDIR, "handler_table.txt"), "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\n")
    print("TABLE: %d entries" % len(assigns))

print("DONE -> %s" % OUTDIR)
