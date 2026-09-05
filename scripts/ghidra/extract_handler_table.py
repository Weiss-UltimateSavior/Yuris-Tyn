# -*- coding: utf-8 -*-
# 提取命令处理器表（DAT_0078b020，BSS，运行时填充）并反编译指定命令的处理器。
#
# 背景：处理器表在 BSS，文件里读不到；由 FUN_0046305c 在运行时逐项赋值。
#       Ghidra 的 C 反编译只保留了部分赋值，因此本脚本**直接扫指令**提取。
#
# 输出（仓库内持久路径，避免 /tmp 丢失）：
#   docs/reverse/decompiled/engine/handler_table.txt
#   docs/reverse/decompiled/engine/CMDH_<addr>_<CMD>.c
#
# @category Yuris
import os
import re

from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.program.model.symbol import SourceType

REPO = "/Users/weiss/Desktop/yuris/yuris-kernel"
OUTDIR = os.path.join(REPO, "docs", "reverse", "decompiled", "engine")
TABLE_BASE = 0x0078b020
INIT_FUNC = 0x0046305c

prog = currentProgram
af = prog.getAddressFactory().getDefaultAddressSpace()
fm = prog.getFunctionManager()
listing = prog.getListing()
monitor = ConsoleTaskMonitor()

if not os.path.exists(OUTDIR):
    os.makedirs(OUTDIR)


def write(path, text):
    fh = open(path, "w")
    fh.write(text if not isinstance(text, unicode) else text.encode("utf-8"))
    fh.close()


# ---------- 1. 扫指令提取处理器表 ----------
f = fm.getFunctionAt(af.getAddress(INIT_FUNC))
if f is None:
    print("FAIL: no function at %08x" % INIT_FUNC)
else:
    insns = listing.getInstructions(f.getBody(), True)
    assigns = {}
    for ins in insns:
        mnem = str(ins.getMnemonicString()).upper()
        if mnem != "MOV":
            continue
        # 目标操作数
        ops = ins.getOpObjects(0)
        if not ops:
            continue
        # 源操作数（立即数）
        src = ins.getOpObjects(1)
        if len(src) != 1:
            continue
        srcobj = src[0]
        # 地址形如 0x78b1b8（Ghidra 可能给出 Address/Register/Scalar）
        dst_txt = str(ops[0])
        src_txt = str(srcobj)
        m = re.match(r"^0x([0-9a-f]{6,8})$", dst_txt.strip(), re.I)
        if not m:
            continue
        dst = int(m.group(1), 16)
        if not (TABLE_BASE <= dst < TABLE_BASE + 0x400):
            continue
        try:
            val = int(str(srcobj), 16)
        except Exception:
            continue
        if not (0x00400000 <= val < 0x00500000):
            continue
        assigns[(dst - TABLE_BASE) // 4] = val

    lines = ["# 命令处理器表 DAT_0078b020（BSS，运行时填充）",
             "# 提取方式：扫 FUN_0046305c 的 MOV 指令（ghidra/extract_handler_table.py）",
             "# 槽位 index -> 命令类型 cmd = index - 8（成果 30 / 勘误）",
             "#",
             "# 格式: index  cmd  表偏移     处理器地址",
             ""]
    for idx in sorted(assigns):
        addr = assigns[idx]
        lines.append("  %3d  0x%02x  0x78b%03x   FUN_%08x" %
                     (idx, idx - 8, (TABLE_BASE + idx * 4) & 0xFFFF, addr))
    write(os.path.join(OUTDIR, "handler_table.txt"), "\n".join(lines) + "\n")
    print("TABLE: %d entries -> handler_table.txt" % len(assigns))
    for idx in sorted(assigns):
        if idx - 8 in (0x66, 0x67):
            print("  HIT cmd=0x%02x index=%d handler=FUN_%08x" %
                  (idx - 8, idx, assigns[idx]))

# ---------- 2. 反编译指定命令的处理器 ----------
# (cmd, 期望的处理器地址)；地址取自上一步，若为 None 则跳过
TARGETS = []


def load_table():
    path = os.path.join(OUTDIR, "handler_table.txt")
    if not os.path.exists(path):
        return {}
    out = {}
    for line in open(path):
        m = re.match(r"\s*(\d+)\s+0x([0-9a-f]{2})\s+0x[0-9a-f]+\s+FUN_([0-9a-f]{8})", line)
        if m:
            out[int(m.group(2), 16)] = int(m.group(3), 16)
    return out


table = load_table()
for cmd in (0x66, 0x67):
    if cmd in table:
        TARGETS.append((cmd, table[cmd]))

decomp = DecompInterface()
decomp.openProgram(prog)

for cmd, addr in TARGETS:
    target = af.getAddress(addr)
    fn = fm.getFunctionContaining(target)
    if fn is None or fn.getEntryPoint().getOffset() != addr:
        c = DisassembleCommand(target, None, True)
        c.applyTo(prog, monitor)
        fcmd = CreateFunctionCmd("CMDH_%08x" % addr, target, None, SourceType.USER_DEFINED)
        fcmd.applyTo(prog, monitor)
        fn = fm.getFunctionContaining(target)
    if fn is None:
        print("FAIL create handler for cmd 0x%02x @ %08x" % (cmd, addr))
        continue
    res = decomp.decompileFunction(fn, 300, monitor)
    if not res.decompileCompleted():
        print("FAIL decompile cmd 0x%02x: %s" % (cmd, res.getErrorMessage()))
        continue
    code = res.getDecompiledFunction().getC()
    name = "CMDH_%08x_cmd%02x.c" % (fn.getEntryPoint().getOffset(), cmd)
    write(os.path.join(OUTDIR, name),
          "// cmd 0x%02x  handler @ %s  size=%d\n%s" %
          (cmd, fn.getEntryPoint(), fn.getBody().getNumAddresses(), code))
    print("OK %s size=%d" % (name, fn.getBody().getNumAddresses()))

print("DONE")
