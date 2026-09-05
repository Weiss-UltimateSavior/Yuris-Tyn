# -*- coding: utf-8 -*-
"""P7.1 —— CG 对象构造/查找函数反编译(默认 SX/SY/COLOR 定性)。"""
import os

os.environ.setdefault("GHIDRA_INSTALL_DIR", r"D:\Dev\ghidra_12.1.3_PUBLIC")
import pyghidra

GHIDRA_DIR = r"D:\Dev\ghidra_12.1.3_PUBLIC"
PROJECT_LOC = r"D:\Dev\GhidraUser"
PROJECT = "yuris_p1"
BINARY = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe"
OUTDIR = r"D:\yuris-kernel\docs\reverse\decompiled\engine\p71_cg"

TARGETS = ["004635a0", "0042c1a0", "0042c0d0"]


def main():
    os.makedirs(OUTDIR, exist_ok=True)
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
        for h in TARGETS:
            av = int(h, 16)
            addr = af.getAddress(av)
            f = fm.getFunctionAt(addr)
            if f is None:
                if fm.getFunctionContaining(addr) is None:
                    DisassembleCommand(addr, None, True).applyTo(prog, monitor)
                CreateFunctionCmd("FUN_%s" % h, addr, None,
                                  SourceType.USER_DEFINED).applyTo(prog, monitor)
                f = fm.getFunctionAt(addr)
            if f is None:
                print("FAIL create %s" % h)
                continue
            r = di.decompileFunction(f, 300, monitor)
            if r.decompileCompleted():
                code = r.getDecompiledFunction().getC()
                fname = "%08x_%s.c" % (av, f.getName().replace(' ', '_'))
                with open(os.path.join(OUTDIR, fname), "w") as fh:
                    fh.write("// %s @ %s size=%d\n" % (
                        f.getName(), f.getEntryPoint(), f.getBody().getNumAddresses()))
                    fh.write(code)
                print("OK %s size=%d" % (fname, f.getBody().getNumAddresses()))
            else:
                print("FAIL decompile %s" % h)


if __name__ == "__main__":
    main()
