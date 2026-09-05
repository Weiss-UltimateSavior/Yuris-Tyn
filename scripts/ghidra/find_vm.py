# -*- coding: utf-8 -*-
# 找 YU-RIS VM 锚点并反编译相关函数(Python2/Jython)
# @category Yuris
import os
import struct
from jarray import array
from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor

prog = currentProgram
mem = prog.getMemory()
fm = prog.getFunctionManager()
monitor = ConsoleTaskMonitor()
outdir = os.path.join(os.environ.get('HOME', '/tmp'), 'ghidra_out')
if not os.path.exists(outdir):
    os.makedirs(outdir)

decomp = DecompInterface()
decomp.openProgram(prog)
dumped = {}

def dump_func(addr, tag):
    f = fm.getFunctionContaining(addr)
    if f is None:
        return None
    key = f.getEntryPoint().getOffset()
    if key in dumped:
        return dumped[key]
    res = decomp.decompileFunction(f, 240, monitor)
    if res.decompileCompleted():
        code = res.getDecompiledFunction().getC()
        fname = "%s_%08x_%s.c" % (tag, key, f.getName().replace(' ', '_'))
        path = os.path.join(outdir, fname)
        fh = open(path, 'w')
        fh.write("// func @ %s  (tag=%s)\n" % (f.getEntryPoint(), tag))
        fh.write(code.encode('utf-8') if isinstance(code, unicode) else code)
        fh.close()
        dumped[key] = path
        print("DUMP %s -> %s" % (f.getEntryPoint(), path))
        return path
    return None

def to_signed_list(raw):
    return [(ord(c) if ord(c) < 128 else ord(c) - 256) for c in raw]

def find_all_bin(raw, limit=32, block=None):
    """二进制搜索。raw = 原始字节串;可选限定在某内存块内。"""
    jb = array(to_signed_list(raw), 'b')
    hits = []
    if block is not None:
        start, end = block.getStart(), block.getEnd()
    else:
        start, end = prog.getMinAddress(), prog.getMaxAddress()
    cur = start
    while len(hits) < limit:
        a = mem.findBytes(cur, jb, None, True, monitor)
        if a is None or a.compareTo(end) > 0:
            break
        hits.append(a)
        cur = a.add(1)
    return hits

def hexpat(s):
    """'59 50 46 00' -> 原始字节串"""
    return ''.join(chr(int(x, 16)) for x in s.split())

print("=== 1) string anchors -> immediate refs in .text ===")
text = mem.getBlock(".text")
needles = ["yst%05d", "Yu-ris", "yscfg", "YU-RISCompiler", "yst_list", "YSCom.ycd"]
for n in needles:
    for a in find_all_bin(n, 8):
        print("STR %r @ %s" % (n, a))
        # 在 .text 中搜该地址的 LE 立即数(如 push 0x007e8d1e)
        imm = struct.pack('<I', a.getOffset())
        refs = find_all_bin(imm, 8, text)
        for r in refs:
            print("   imm-ref @ %s" % r)
            dump_func(r, "str_" + filter(str.isalnum, n))

print("=== 2) binary anchors ===")
bin_anchors = [
    ("ypf_magic", "59 50 46 00"),
    ("ystb_magic", "59 53 54 42"),
    ("yscm_magic", "59 53 43 4d"),
    ("yscf_magic", "59 53 43 46"),
    ("tag3_cmp_eax", "3d 00 03 00 00"),
    ("tag3_cmp_r32", "81 f8 00 03 00 00"),
    ("tag3_cmp_r32b", "81 f9 00 03 00 00"),
    ("tag3_cmp_r32c", "81 fa 00 03 00 00"),
    ("tag3_cmp_r32d", "81 fb 00 03 00 00"),
    ("tag3_cmp_r32e", "81 fe 00 03 00 00"),
    ("tag3_cmp_r32f", "81 ff 00 03 00 00"),
    ("and3_eax", "83 e0 03"),
    ("and3_ecx", "83 e1 03"),
    ("and3_edx", "83 e2 03"),
    ("and3_ebx", "83 e3 03"),
    ("and3_esi", "83 e6 03"),
    ("and3_edi", "83 e7 03"),
]
for tag, pat in bin_anchors:
    hits = find_all_bin(hexpat(pat), 24, text)
    print("ANCHOR %-16s %d hits" % (tag, len(hits)))
    for a in hits:
        dump_func(a, tag)

print("=== 3) jump-table scan (non-exec blocks) ===")
for b in mem.getBlocks():
    if b.isExecute():
        continue
    size = b.getSize()
    if size < 512 or size > 8000000:
        continue
    try:
        jb = b.getBytes(b.getStart(), size)
    except Exception:
        continue
    n = size / 4
    if n < 48:
        continue
    words = struct.unpack('<%dI' % n, ''.join(chr(x & 0xFF) for x in jb[:n * 4]))
    lo = text.getStart().getOffset()
    hi = text.getEnd().getOffset()
    run = 0
    run_start = 0
    for i in range(n):
        ok = lo <= words[i] <= hi
        if ok:
            if run == 0:
                run_start = i
            run += 1
        else:
            if run >= 48:
                addr = b.getStart().add(run_start * 4)
                print("TABLE %s +0x%x len=%d" % (b.getName(), run_start * 4, run))
                dump_func(addr, "jumptable")
            run = 0

print("DONE: %d functions dumped to %s" % (len(dumped), outdir))
