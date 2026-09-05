# -*- coding: utf-8 -*-
"""P1 引擎真值 hook —— 配置生成器(PyGhidra)。

从已分析好的 Ghidra 工程(kemonomichi2.exe)提取运行期 hook 所需全部地址:
  1. 反编译表初始化器 FUN_0046305c,**按反编译文本的字面量赋值**提取
     121 项命令处理器表(DAT_0078b020,槽 = 0x78b020 + 4*cmd)。
     —— 旧 extract_handler_table.py 的「扫 MOV 指令」路线产出恒 0 项(已废弃),
        本脚本改走 decompile_handlers.py 的「补建函数 + 反编译」经验路线。
  2. 补建 + 反编译 GO/GOSUB/RETURN/任务激活(FUN_0040480c),供人工钉出
     运行时对象里「当前脚本号」的字段偏移。
  3. 产出 scripts/engine_trace_config.json,供 scripts/engine_trace.py 消费。

用法: python scripts/ghidra/hook_trace_setup.py
"""
import json
import os
import re

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import pyghidra

GHIDRA_DIR = r"D:\Dev\ghidra_12.1.3_PUBLIC"
PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"

RECON_DIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\hook_recon"
CONFIG_OUT = r"D:\yuris-kernel\scripts\engine_trace_config.json"

TABLE_BASE = 0x0078B020
TABLE_SLOTS = 121
DEFAULT_STUB = 0x0045C4D4
INIT_FUNC = 0x0046305C

# 需要补建函数并反编译的关键函数(地址, 标签)
RECON_TARGETS = [
    (0x0040480C, "task_activate"),
    (0x0044272C, "GO"),
    (0x004428C0, "GOSUB"),
    (0x0044B418, "RETURN"),
]

IMAGE_BASE = 0x00400000

# 运行期全局(主循环 recon_0040449c 已钉出):
OBJ_GLOBAL = 0x00872404  # DAT_00872404 = 当前脚本运行时对象(loader obj - 0x20)
TASK_GLOBAL = 0x00872408  # DAT_00872408 = 当前任务结构
PC_OFF = 0x20  # obj+0x20 = 运行期 pc(loader obj+0x00 的 G 被复用;先取后增)


def main():
    os.makedirs(RECON_DIR, exist_ok=True)
    pyghidra.start()
    with pyghidra.open_program(BINARY, project_location=PROJECT_LOC,
                               project_name=PROJECT, analyze=True) as flat:
        prog = flat.getCurrentProgram()
        af = prog.getAddressFactory().getDefaultAddressSpace()
        fm = prog.getFunctionManager()

        from ghidra.app.decompiler import DecompInterface
        from ghidra.util.task import ConsoleTaskMonitor
        from ghidra.app.cmd.function import CreateFunctionCmd
        from ghidra.app.cmd.disassemble import DisassembleCommand
        from ghidra.program.model.symbol import SourceType

        di = DecompInterface()
        di.openProgram(prog)
        monitor = ConsoleTaskMonitor()

        def ensure_func(addr_val, name):
            """补建函数(decompile_handlers.py 经验):强制反汇编 + CreateFunctionCmd。"""
            addr = af.getAddress(addr_val)
            f = fm.getFunctionAt(addr)
            if f is None:
                if not fm.getFunctionContaining(addr):
                    DisassembleCommand(addr, None, True).applyTo(prog, monitor)
                cmd = CreateFunctionCmd(name, addr, None, SourceType.USER_DEFINED)
                if not cmd.applyTo(prog, monitor):
                    print("CREATE FUNC FAIL %08x %s" % (addr_val, name))
                    return None
                f = fm.getFunctionAt(addr)
            return f

        def decompile(f):
            res = di.decompileFunction(f, 120, monitor)
            if not res.decompileCompleted():
                return None
            return res.getDecompiledFunction().getC()

        # ---- 1) 表初始化器 → 处理器表映射 ----
        f_init = fm.getFunctionAt(af.getAddress(INIT_FUNC))
        if f_init is None:
            f_init = ensure_func(INIT_FUNC, "CMDTBL_init")
        c = decompile(f_init)
        assert c, "table init 反编译失败"
        table = {}  # slot_addr -> handler
        pat = re.compile(r"(?:_)?DAT_([0-9A-Fa-f]{8})\s*=\s*&?(?:LAB_|FUN_)?([0-9A-Fa-f]{8})")
        for m in pat.finditer(c):
            slot = int(m.group(1), 16)
            if TABLE_BASE <= slot < TABLE_BASE + TABLE_SLOTS * 4 and (slot - TABLE_BASE) % 4 == 0:
                table[slot] = int(m.group(2), 16)
        # 补默认 stub:表初始化先全部填 0x45c4d4
        for i in range(TABLE_SLOTS):
            table.setdefault(TABLE_BASE + i * 4, DEFAULT_STUB)
        cmd_map = {}  # cmd -> handler
        for i in range(TABLE_SLOTS):
            cmd_map[i] = table[TABLE_BASE + i * 4]
        print("TABLE: %d 项(显式 %d,余为默认 stub 0x45c4d4)"
              % (TABLE_SLOTS, sum(1 for v in cmd_map.values() if v != DEFAULT_STUB)))

        # 反向:handler -> [cmds](共享处理器,如 no-op FUN_00423080)
        handler_cmds = {}
        for cmd, h in cmd_map.items():
            handler_cmds.setdefault(h, []).append(cmd)

        # ---- 2) 关键函数反编译 ----
        for addr_val, tag in RECON_TARGETS:
            f = ensure_func(addr_val, "CMDH_%08x" % addr_val)
            if f is None:
                print("NO FUNC %08x %s" % (addr_val, tag))
                continue
            cc = decompile(f)
            if cc is None:
                print("DECOMP FAIL %08x %s" % (addr_val, tag))
                continue
            path = os.path.join(RECON_DIR, "recon_%08x_%s.c" % (addr_val, tag))
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(cc)
            print("OK %08x %s (%d bytes)" % (addr_val, tag, len(cc)))

        # ---- 3) 配置 ----
        cfg = {
            "binary": BINARY,
            "image_base": IMAGE_BASE,
            "obj_global": OBJ_GLOBAL,
            "task_global": TASK_GLOBAL,
            "pc_off": PC_OFF,
            "script_id_off": 0x3C,  # u16;CMDH_0044272c/004428c0/0044b418 三处交叉验证
            "script_id_size": 2,
            "default_stub": DEFAULT_STUB,
            "cmd_map": {"0x%02x" % k: "0x%08x" % v for k, v in sorted(cmd_map.items())},
            "handler_cmds": {"0x%08x" % h: cs for h, cs in sorted(handler_cmds.items())},
        }
        with open(CONFIG_OUT, "w", encoding="utf-8") as fh:
            json.dump(cfg, fh, indent=1, ensure_ascii=False)
        print("CONFIG -> %s" % CONFIG_OUT)


if __name__ == "__main__":
    main()
