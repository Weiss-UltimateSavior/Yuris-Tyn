# -*- coding: utf-8 -*-
"""P3 —— YSCM 参数表 dump(CG 族)+ 真实 CG 处理器反编译。

修正:cmd 0x01 CG 处理器 = 0x423864(表初始化 DAT_0078b024);
     0x43c984 = cmd 0x0a DIALOG(此前 PROGRESS/lib.rs 误标为 CG)。
"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")

import struct
import zlib

def rcs(d, dp):
    out = bytearray()
    while d[dp] != 0:
        out.append(d[dp] ^ 0xC9)
        dp += 1
    return bytes(out), dp + 1

def load_yscm():
    d = open(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac\bn.ypf", "rb").read()
    _, _, cnt, data0 = struct.unpack_from("<4sIII", d, 0)
    p = 0x24
    entries = {}
    for _ in range(cnt):
        name, p = rcs(d, p)
        flags = d[p]; p += 1
        uncomp, comp, off, _r = struct.unpack_from("<IIII", d, p); p += 16
        p += min(8, data0 - p)
        entries[name.decode("ascii", "replace")] = (flags, comp, off)
    flags, comp, off = entries["%ysbin\\ysc.ybn"]
    raw = d[off:off + comp]
    y = zlib.decompress(raw) if flags == 1 else raw
    b = bytearray(y)
    key = bytes.fromhex("2b904f93")
    for i in range(0x20, len(b)):
        b[i] ^= key[i % 4]
    return bytes(b)

def dump_cg_params():
    y = load_yscm()
    ncmd = struct.unpack_from("<I", y, 8)[0]
    pp = 0x10
    for ci in range(ncmd):
        e = y.index(b"\0", pp)
        name = y[pp:e].decode("cp932", "replace")
        pp = e + 1
        pc = y[pp]; pp += 1
        params = []
        ok = True
        for _ in range(pc):
            e2 = y.find(b"\0", pp)
            if e2 < 0 or e2 + 3 > len(y):
                ok = False
                break
            pn = y[pp:e2].decode("cp932", "replace")
            a1, a2 = y[e2 + 1], y[e2 + 2]
            pp = e2 + 3
            params.append((pn, a1, a2))
        if name in ("CG", "CGACT", "CGEND", "CGINFO", "DIALOG") or not ok:
            print("== %s (cmd 0x%02x) params=%d ok=%s" % (name, ci, pc, ok))
            for pn, a1, a2 in params:
                print("   %-8s kind=%d attr2=%d" % (pn, a1, a2))
        if not ok:
            break

dump_cg_params()

# ---- pyghidra:反编译真实 CG 处理器 + DIALOG ----
import pyghidra

PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine"

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

    for addr_val, tag in [(0x00423864, "CMD_CG_real_0x01"),
                          (0x0043C984, "CMD_DIALOG_0x0a")]:
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
        res = di.decompileFunction(f, 240, monitor)
        if not res.decompileCompleted():
            print("DECOMP FAIL %08x %s" % (addr_val, tag))
            continue
        c = res.getDecompiledFunction().getC()
        path = os.path.join(OUTDIR, "%08x_%s.c" % (addr_val, tag))
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(c)
        print("OK %08x %s (%d bytes)" % (addr_val, tag, len(c)))

print("DONE")
