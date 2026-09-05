# -*- coding: utf-8 -*-
# P1 引擎真值 hook 的前置侦察:
#   1) 反编译 hook 设计所需的关键函数(主循环/任务轮转/加载器/GO/表初始化/启动链)
#   2) 扫 FUN_0046305c 的 MOV 指令提取 121 项处理器表(槽位->地址)
# 输出: docs/reverse/decompiled/engine/hook_recon/ 下
#   recon_<addr>_<NAME>.c + handler_table.txt
#
# @category Yuris
import os

from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor

OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\hook_recon"
TABLE_BASE = 0x0078B020
INIT_FUNC = 0x0046305C

# (addr, tag)  反编译目标
TARGETS = [
    (0x0040449c, "main_loop"),
    (0x0043be6c, "task_rotate"),
    (0x00450dfd, "loader"),
    (0x00451348, "ysvr_apply"),
    (0x0046305c, "table_init"),
    (0x0044272c, "GO"),
    (0x004428c0, "GOSUB"),
    (0x0044b418, "RETURN"),
    (0x004431ec, "IF"),
    (0x00451724, "script_end"),
    (0x0046b63c, "boot_chain"),
    (0x00463c7c, "label_load"),
    (0x0045124c, "label_hash_find"),
]

prog = currentProgram
af = prog.getAddressFactory().getDefaultAddressSpace()
fm = prog.getFunctionManager()
monitor = ConsoleTaskMonitor()

if not os.path.exists(OUTDIR):
    os.makedirs(OUTDIR)


def write(path, text):
    fh = open(path, "w")
    fh.write(text.encode("utf-8"))
    fh.close()


# ---------- 1. 反编译目标函数 ----------
di = DecompInterface()
di.openProgram(prog)

for addr, tag in TARGETS:
    f = fm.getFunctionAt(af.getAddress(addr))
    if f is None:
        print("NO FUNC at %08x (%s)" % (addr, tag))
        continue
    res = di.decompileFunction(f, 120, monitor)
    if not res.decompileCompleted():
        print("DECOMP FAIL %08x %s" % (addr, tag))
        continue
    c = res.getDecompiledFunction().getC()
    write(os.path.join(OUTDIR, "recon_%08x_%s.c" % (addr, tag)), c)
    print("OK %08x %s (%d bytes)" % (addr, tag, len(c)))

# ---------- 2. 扫表初始化函数的 MOV 提取处理器表 ----------
f = fm.getFunctionAt(af.getAddress(INIT_FUNC))
if f is None:
    print("FAIL: no function at %08x" % INIT_FUNC)
else:
    listing = prog.getListing()
    insns = listing.getInstructions(f.getBody(), True)
    assigns = {}
    for ins in insns:
        if str(ins.getMnemonicString()).upper() != "MOV":
            continue
        ops = ins.getOpObjects(0)
        src = ins.getOpObjects(1)
        if not ops or len(src) != 1:
            continue
        # 目标 [disp] 形式 -> 表槽位
        if len(ops) < 2:
            continue
        disp = None
        for o in ops:
            try:
                d = o.getOffset()
            except Exception:
                continue
            if d is not None and d >= TABLE_BASE and d < TABLE_BASE + 0x400:
                disp = d
                break
        if disp is None:
            continue
        val = src[0]
        try:
            v = val.getOffset() & 0xFFFFFFFF
        except Exception:
            continue
        if 0x00400000 <= v < 0x00500000:
            assigns[disp] = v
    lines = []
    for disp in sorted(assigns):
        idx = (disp - TABLE_BASE) / 4
        cmd = idx - 8
        lines.append("%4d  cmd=%4d(0x%02x)  [%08x]  -> %08x"
                     % (idx, cmd, cmd & 0xFF, disp, assigns[disp]))
    write(os.path.join(OUTDIR, "handler_table.txt"),
          "# DAT_0078b020 处理器表(扫 %08x MOV 提取, %d 项)\n"
          "# 格式: index cmd 表偏移 处理器地址\n\n%s\n"
          % (INIT_FUNC, len(assigns), "\n".join(lines)))
    print("TABLE: %d entries" % len(assigns))

print("DONE -> %s" % OUTDIR)
