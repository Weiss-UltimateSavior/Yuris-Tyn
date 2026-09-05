#!/usr/bin/env python3
"""P1 引擎真值采集器 —— Windows 调试器 hook 原引擎命令处理器入口。

原理(docs/engine/command-layer.md §3,Confirmed):
  主循环 FUN_0040449c 每次派发 `X[sel][pc]()`;X[1][g] = DAT_0078b020[cmd_g]
  (loader FUN_00450dfd 逐组展开,反编译 130-131 行实锤)。故对全部处理器
  入口下 int3 断点,命中即等价于「引擎执行了一条命令」:
    - pc     = *(u32*)(DAT_00872404 + 0x20) - 1   (先取后增)
    - 脚本号 = *(u16*)(DAT_00872404 + 0x3c)       (GO/GOSUB/RETURN 三处交叉验证)
    - cmd    = 命中处理器地址反查处理器表(scripts/engine_trace_config.json)
  输出 JSONL 与 Rust VM 事件流(crates/yuris-vm/examples/vm_trace.rs)同域对拍
  (diff 工具:scripts/diff_engine_vm.py)。

用法:
  python scripts/engine_trace.py [--timeout 90] [--max-events 200000]

注意:
  - 会在桌面短暂弹出游戏窗口(采集完即杀进程)。
  - 64 位 Python 调 32 位引擎:必须 Wow64Get/SetThreadContext;DEBUG_EVENT 按
    调试器位布局(64 位),EXCEPTION_RECORD.ExceptionInformation = ULONG_PTR[15]。
  - 引擎带启动期反调试(0x4000001F FatalAppExit 自杀):hide_debugger() 清
    PEB.BeingDebugged/NtGlobalFlag;多进程跟踪(启动器派生真身进程也能跟)。
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import faulthandler
import json
import os
import struct
import sys
import time

faulthandler.enable()

kernel32 = ctypes.windll.kernel32
_k = kernel32

# ---------------- 常量 ----------------
DEBUG_ONLY_THIS_PROCESS = 0x02
DBG_CONTINUE = 0x00010002
DBG_EXCEPTION_NOT_HANDLED = 0x80010001

CREATE_THREAD_DEBUG_EVENT = 2
EXCEPTION_DEBUG_EVENT = 1
CREATE_PROCESS_DEBUG_EVENT = 3
EXIT_THREAD_DEBUG_EVENT = 4
EXIT_PROCESS_DEBUG_EVENT = 5
LOAD_DLL_DEBUG_EVENT = 6
UNLOAD_DLL_DEBUG_EVENT = 7
OUTPUT_DEBUG_STRING_EVENT = 8
RIP_EVENT = 9

EXCEPTION_BREAKPOINT = 0x80000003
EXCEPTION_SINGLE_STEP = 0x80000004
# WOW64:64 位调试器收 32 位进程的 int3/单步,以 WX86 码上报(实测 0x443808
# 命中时 code=0x4000001F、Eip=trap 后地址;不处理会被 NOT_HANDLED 二次机会处死)
EXCEPTION_WX86_BREAKPOINT = 0x4000001F
EXCEPTION_WX86_SINGLE_STEP = 0x4000001E

CONTEXT_i386 = 0x00010000
CONTEXT_CONTROL = CONTEXT_i386 | 0x00000001
CONTEXT_INTEGER = CONTEXT_i386 | 0x00000002
CONTEXT_DEBUG_REGISTERS = CONTEXT_i386 | 0x00000010
CTX_FLAGS = CONTEXT_CONTROL | CONTEXT_INTEGER | CONTEXT_DEBUG_REGISTERS
EFLAGS_TF = 0x100
# Dr7: L0|G0 使能 + R/W0=01(写) + LEN0=11(4 字节)
DR7_WATCH_WRITE = 0x000F0003
DESC_GLOBAL = 0x0087240C  # 变量描述符表 DAT_0087240c[id]


# ---------------- 结构 ----------------
class EXCEPTION_RECORD(ctypes.Structure):
    # ExceptionInformation 是 ULONG_PTR[15]:64 位调试器下每项 8 字节,
    # 定义成 DWORD[15] 会让 WaitForDebugEvent 写溢出缓冲区(0xC0000005)。
    _fields_ = [("ExceptionCode", wt.DWORD),
                ("ExceptionFlags", wt.DWORD),
                ("ExceptionRecord", ctypes.c_void_p),
                ("ExceptionAddress", ctypes.c_void_p),
                ("NumberParameters", wt.DWORD),
                ("ExceptionInformation", ctypes.c_size_t * 15)]


class EXCEPTION_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("ExceptionRecord", EXCEPTION_RECORD), ("dwFirstChance", wt.DWORD)]


class CREATE_PROCESS_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("hFile", wt.HANDLE), ("hProcess", wt.HANDLE), ("hThread", wt.HANDLE),
                ("lpBaseOfImage", ctypes.c_void_p), ("lpBaseOfDll", ctypes.c_void_p),
                ("dwDebugInfoFileOffset", wt.DWORD), ("fUnicode", wt.WORD),
                ("nDebugInfoSize", wt.DWORD), ("lpThreadLocalBase", ctypes.c_void_p),
                ("lpStartAddress", ctypes.c_void_p), ("lpImageName", ctypes.c_void_p),
                ("fUnicode2", wt.WORD)]


class CREATE_THREAD_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("hThread", wt.HANDLE), ("lpThreadLocalBase", ctypes.c_void_p),
                ("lpStartAddress", ctypes.c_void_p)]


class EXIT_THREAD_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("dwExitCode", wt.DWORD)]


class EXIT_PROCESS_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("dwExitCode", wt.DWORD)]


class LOAD_DLL_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("hFile", wt.HANDLE), ("lpBaseOfDll", ctypes.c_void_p),
                ("hSection", wt.HANDLE), ("dwDebugInfoFileOffset", wt.DWORD),
                ("nDebugInfoSize", wt.DWORD), ("lpImageName", ctypes.c_void_p),
                ("fUnicode", wt.WORD)]


class UNLOAD_DLL_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("lpBaseOfDll", ctypes.c_void_p)]


class OUTPUT_DEBUG_STRING_INFO(ctypes.Structure):
    _fields_ = [("lpDebugStringData", ctypes.c_void_p), ("fUnicode", wt.WORD),
                ("nDebugStringLength", wt.WORD)]


class RIP_INFO(ctypes.Structure):
    _fields_ = [("dwError", wt.DWORD), ("dwType", wt.DWORD)]


class _U(ctypes.Union):
    _fields_ = [("Exception", EXCEPTION_DEBUG_INFO),
                ("CreateThread", CREATE_THREAD_DEBUG_INFO),
                ("CreateProcessInfo", CREATE_PROCESS_DEBUG_INFO),
                ("ExitThread", EXIT_THREAD_DEBUG_INFO),
                ("ExitProcess", EXIT_PROCESS_DEBUG_INFO),
                ("LoadDll", LOAD_DLL_DEBUG_INFO),
                ("UnloadDll", UNLOAD_DLL_DEBUG_INFO),
                ("DebugString", OUTPUT_DEBUG_STRING_INFO),
                ("RipInfo", RIP_INFO)]


class DEBUG_EVENT(ctypes.Structure):
    _fields_ = [("dwDebugEventCode", wt.DWORD), ("dwProcessId", wt.DWORD),
                ("dwThreadId", wt.DWORD), ("u", _U)]


class FLOATING_SAVE_AREA(ctypes.Structure):
    _fields_ = [(n, wt.DWORD) for n in ("ControlWord", "StatusWord", "TagWord",
                                        "ErrorOffset", "ErrorSelector",
                                        "DataOffset", "DataSelector")] + \
               [("RegisterArea", ctypes.c_ubyte * 80), ("Cr0NpxState", wt.DWORD)]


class CONTEXT(ctypes.Structure):
    """x86 CONTEXT(WOW64 进程的 32 位上下文;配 Wow64Get/SetThreadContext)。"""
    _fields_ = [("ContextFlags", wt.DWORD),
                ("Dr0", wt.DWORD), ("Dr1", wt.DWORD), ("Dr2", wt.DWORD),
                ("Dr3", wt.DWORD), ("Dr6", wt.DWORD), ("Dr7", wt.DWORD),
                ("FloatSave", FLOATING_SAVE_AREA),
                ("SegGs", wt.DWORD), ("SegFs", wt.DWORD), ("SegEs", wt.DWORD),
                ("SegDs", wt.DWORD),
                ("Edi", wt.DWORD), ("Esi", wt.DWORD), ("Ebx", wt.DWORD),
                ("Edx", wt.DWORD), ("Ecx", wt.DWORD), ("Eax", wt.DWORD),
                ("Ebp", wt.DWORD), ("Eip", wt.DWORD), ("SegCs", wt.DWORD),
                ("EFlags", wt.DWORD), ("Esp", wt.DWORD), ("SegSs", wt.DWORD),
                ("ExtendedRegisters", ctypes.c_ubyte * 512)]


class STARTUPINFOW(ctypes.Structure):
    _fields_ = [("cb", wt.DWORD), ("lpReserved", wt.LPWSTR), ("lpDesktop", wt.LPWSTR),
                ("lpTitle", wt.LPWSTR), ("dwX", wt.DWORD), ("dwY", wt.DWORD),
                ("dwXSize", wt.DWORD), ("dwYSize", wt.DWORD), ("dwXCountChars", wt.DWORD),
                ("dwYCountChars", wt.DWORD), ("dwFillAttribute", wt.DWORD),
                ("dwFlags", wt.DWORD), ("wShowWindow", wt.WORD), ("cbReserved2", wt.WORD),
                ("lpReserved2", ctypes.c_void_p), ("hStdInput", wt.HANDLE),
                ("hStdOutput", wt.HANDLE), ("hStdError", wt.HANDLE)]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("hProcess", wt.HANDLE), ("hThread", wt.HANDLE),
                ("dwProcessId", wt.DWORD), ("dwThreadId", wt.DWORD)]


# ---------------- 显式原型(默认转换在 64 位下不可靠) ----------------
def _setup_prototypes():
    _k.WaitForDebugEvent.argtypes = [ctypes.POINTER(DEBUG_EVENT), wt.DWORD]
    _k.WaitForDebugEvent.restype = wt.BOOL
    _k.ContinueDebugEvent.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD]
    _k.ContinueDebugEvent.restype = wt.BOOL
    _k.TerminateProcess.argtypes = [wt.HANDLE, wt.UINT]
    _k.TerminateProcess.restype = wt.BOOL
    _k.CreateProcessW.argtypes = [wt.LPCWSTR, wt.LPWSTR, ctypes.c_void_p,
                                  ctypes.c_void_p, wt.BOOL, wt.DWORD,
                                  ctypes.c_void_p, wt.LPCWSTR,
                                  ctypes.POINTER(STARTUPINFOW),
                                  ctypes.POINTER(PROCESS_INFORMATION)]
    _k.CreateProcessW.restype = wt.BOOL
    _k.IsWow64Process.argtypes = [wt.HANDLE, ctypes.POINTER(wt.BOOL)]
    _k.IsWow64Process.restype = wt.BOOL
    for name in ("ReadProcessMemory", "WriteProcessMemory"):
        fn = getattr(_k, name)
        fn.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                       ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
        fn.restype = wt.BOOL
    for name in ("GetThreadContext", "SetThreadContext",
                 "Wow64GetThreadContext", "Wow64SetThreadContext"):
        fn = getattr(_k, name, None)
        if fn is not None:
            fn.argtypes = [wt.HANDLE, ctypes.c_void_p]
            fn.restype = wt.BOOL


_setup_prototypes()

_ntdll = ctypes.windll.ntdll
_ntdll.NtQueryInformationProcess.argtypes = [wt.HANDLE, ctypes.c_uint, ctypes.c_void_p,
                                             ctypes.c_uint, ctypes.POINTER(ctypes.c_uint)]
_ntdll.NtQueryInformationProcess.restype = ctypes.c_long
PROCESS_WOW64_INFORMATION = 26  # → 输出 WOW64(32 位)PEB 地址


def hide_debugger(hproc):
    """抹掉 PEB.BeingDebugged / NtGlobalFlag(启动期反调试第一道检查)。"""
    peb = ctypes.c_uint64(0)
    st = _ntdll.NtQueryInformationProcess(hproc, PROCESS_WOW64_INFORMATION,
                                          ctypes.byref(peb), ctypes.sizeof(peb), None)
    if st != 0 or not peb.value:
        print(f"[trace] PEB32 获取失败 ntstatus={st:#x}")
        return False
    p = peb.value
    got = ctypes.c_size_t(0)
    _k.WriteProcessMemory(hproc, ctypes.c_void_p(p + 0x02), b"\x00", 1,
                          ctypes.byref(got))  # BeingDebugged
    _k.WriteProcessMemory(hproc, ctypes.c_void_p(p + 0x68), b"\x00\x00\x00\x00", 4,
                          ctypes.byref(got))  # NtGlobalFlag
    print(f"[trace] PEB32={p:#x} BeingDebugged/NtGlobalFlag 已清")
    return True


class Tracer:
    def __init__(self, cfg, out_path, max_events, timeout_s):
        self.cfg = cfg
        self.out = open(out_path, "w", encoding="utf-8")
        self.max_events = max_events
        self.timeout_s = timeout_s
        self.procs = {}            # pid -> {"hproc","base","reloc","bps":{addr:byte}}
        self.threads = {}          # tid -> hThread
        self.pending_ss = {}       # (pid,tid) -> bp addr awaiting re-arm
        self.seq = 0
        self.n_events = 0
        self.t0 = None
        self.stop_reason = None
        self.excs = {}             # (code, first_chance) -> count(非断点异常)
        self.wow64 = False
        self.watch_spec = None     # (var_id, idx) 硬件写监视
        self.watch_addr = 0
        self.watch_arm_addr = None # 提前布防点(启动链内)
        self.watch_arm_ev = 0      # 延迟布防:事件序号门槛(n_events >= 才尝试)
        self.no_rearm = set()      # 一次性断点(布防点)
        self.bp_log_addr = None    # 任意执行断点(寄存器日志)
        self.bp_log_ev = None      # 仅在该事件序号区间记录

    # ---- 进程内存 ----
    def read_mem(self, pid, addr, n):
        buf = ctypes.create_string_buffer(n)
        got = ctypes.c_size_t(0)
        h = self.procs[pid]["hproc"]
        if not _k.ReadProcessMemory(h, ctypes.c_void_p(addr), buf, n,
                                    ctypes.byref(got)):
            return None
        return buf.raw[:got.value]

    def write_mem(self, pid, addr, data):
        got = ctypes.c_size_t(0)
        h = self.procs[pid]["hproc"]
        return bool(_k.WriteProcessMemory(h, ctypes.c_void_p(addr), data,
                                          len(data), ctypes.byref(got)))

    # ---- 线程上下文(WOW64 进程用 Wow64 变体,操作 x86 CONTEXT)----
    def get_ctx(self, hthread):
        ctx = CONTEXT()
        ctx.ContextFlags = CTX_FLAGS
        fn = _k.Wow64GetThreadContext if self.wow64 else _k.GetThreadContext
        if not fn(hthread, ctypes.byref(ctx)):
            return None
        return ctx

    def set_ctx(self, hthread, ctx):
        fn = _k.Wow64SetThreadContext if self.wow64 else _k.SetThreadContext
        return bool(fn(hthread, ctypes.byref(ctx)))

    def detect_wow64(self, hproc):
        py64 = ctypes.sizeof(ctypes.c_void_p) == 8
        if not py64 or hproc is None:
            self.wow64 = False
            return
        wow = wt.BOOL()
        if _k.IsWow64Process(hproc, ctypes.byref(wow)):
            self.wow64 = bool(wow.value)
        print(f"[trace] wow64={self.wow64}")

    # ---- 断点 ----
    def arm_proc(self, pid):
        base = self.procs[pid]["base"]
        self.procs[pid]["reloc"] = base - self.cfg["image_base"]
        reloc = self.procs[pid]["reloc"]
        bps = {}
        handlers = {int(h, 16) + reloc for h in self.cfg["cmd_map"].values()}
        for a in sorted(handlers):
            orig = self.read_mem(pid, a, 1)
            if orig is None:
                continue
            if self.write_mem(pid, a, b"\xCC"):
                bps[a] = orig[0]
        self.procs[pid]["bps"] = bps
        print(f"[trace] pid={pid} 断点 {len(bps)}/{len(handlers)} "
              f"(base {base:#x} reloc {reloc:#x})")

    # ---- 事件记录 ----
    def emit(self, obj):
        self.seq += 1
        self.out.write(json.dumps(obj, ensure_ascii=False, separators=(",", ":")) + "\n")
        self.out.flush()  # 调试器进程若异常退出,已采集数据不丢

    def selfcheck(self):
        print("[trace] sizeof DEBUG_EVENT=%d EXCEPTION_RECORD=%d CONTEXT=%d"
              % (ctypes.sizeof(DEBUG_EVENT), ctypes.sizeof(EXCEPTION_RECORD),
                 ctypes.sizeof(CONTEXT)))
        self.out.write('{"ev":"meta","side":"selfcheck",'
                       '"sizeof_debug_event":%d}\n' % ctypes.sizeof(DEBUG_EVENT))
        self.out.flush()

    def snapshot_state(self, pid):
        """读运行时对象 {pc, script};不可读则全 None。"""
        reloc = self.procs[pid]["reloc"]
        raw = self.read_mem(pid, self.cfg["obj_global"] + reloc, 4)
        if not raw:
            return None, None
        r = int.from_bytes(raw, "little")
        if not r:
            return None, None
        pc_raw = self.read_mem(pid, r + self.cfg["pc_off"] + reloc, 4)
        sc_raw = self.read_mem(pid, r + self.cfg["script_id_off"] + reloc, 2)
        pc = int.from_bytes(pc_raw, "little") - 1 if pc_raw else None
        sc = int.from_bytes(sc_raw, "little") if sc_raw else None
        return pc, sc

    def handler_cmds(self, pid, addr):
        key = "0x%08x" % (addr - self.procs[pid]["reloc"])
        return self.cfg["handler_cmds"].get(key)

    # ---- 断点/单步处理 ----
    def on_breakpoint(self, pid, tid):
        h = self.threads.get(tid)
        bps = self.procs[pid]["bps"]
        if h is None:
            print(f"[trace] bp:无线程句柄 tid={tid}")
            return None
        if not bps:
            return None
        ctx = self.get_ctx(h)
        if ctx is None:
            print(f"[trace] bp:GetContext 失败 err={ctypes.GetLastError()}")
            return None
        # int3 为 trap:EIP 已越过断点字节;兼容两种报告
        for cand in (ctx.Eip - 1, ctx.Eip):
            if cand in bps:
                break
        else:
            print(f"[trace] bp:非本方断点 Eip={ctx.Eip:#x} 初始断点放行")
            return None
        addr = cand
        reloc = self.procs[pid]["reloc"]
        kind = ("default_stub" if addr - reloc == self.cfg["default_stub"]
                else addr)
        # 恢复原字节 → 回退 EIP → 置 TF 单步执行原指令
        self.write_mem(pid, addr, bytes([bps.pop(addr)]))
        ctx.Eip = addr
        ctx.EFlags |= EFLAGS_TF
        if not self.set_ctx(h, ctx):
            print(f"[trace] bp:SetContext 失败 err={ctypes.GetLastError()} "
                  f"addr={addr:#x}(关键!)")
        self.pending_ss[(pid, tid)] = addr
        return kind

    def on_single_step(self, pid, tid):
        addr = self.pending_ss.pop((pid, tid), None)
        if addr is None:
            # 无 pending 单步 → 可能是硬件监视点命中(DR6.B0)
            if self.watch_addr:
                h = self.threads.get(tid)
                ctx = self.get_ctx(h) if h else None
                if ctx is not None and (ctx.Dr6 & 1):
                    ea = ctx.Eip
                    base = self.procs.get(pid, {}).get("base") or 0
                    stack = self.read_mem(pid, ctx.Esp, 32)
                    ret = int.from_bytes(stack[0:4], "little") if stack else 0
                    val_raw = self.read_mem(pid, self.watch_addr, 8)
                    val_i = (int.from_bytes(val_raw, "little")
                             if val_raw else None)
                    val_f = (struct.unpack_from("<d", val_raw)[0]
                             if val_raw else None)
                    wpc, wsc = self.snapshot_state(pid)
                    print(f"[watch] 写命中 @ {self.watch_addr:#010x} "
                          f"writer Eip={ea:#010x} (base+{ea - base:#x}) "
                          f"ret={ret:#010x} (base+{ret - base:#x}) "
                          f"i64={val_i} f64={val_f} script={wsc} pc={wpc} "
                          f"ev={self.n_events}")
                    # 清 DR6 后继续(硬件断点自动保持)
                    ctx.Dr6 = 0
                    self.set_ctx(h, ctx)
                    return True
            return False
        bps = self.procs[pid]["bps"]
        if addr in self.no_rearm:
            return True  # 一次性布防点(监视点布防),不再回填 int3
        if addr not in bps:  # 重新布防
            orig = self.read_mem(pid, addr, 1)
            if orig is not None and self.write_mem(pid, addr, b"\xCC"):
                bps[addr] = orig[0]
        return True

    # ---- 硬件监视点(@53[2] 等系统数组写入者定位) ----
    def arm_watch(self, pid, tid):
        """首次命中时调用:dump 描述符 + 布防 Dr0 写监视点。

        DAT_0087240c 是表指针(boot_init_a: malloc(count*4+4)),
        条目 = *(table + id*4) = 各自 0x40 字节描述符。
        """
        vid, idx = self.watch_spec
        reloc = self.procs[pid]["reloc"]
        tbl_raw = self.read_mem(pid, DESC_GLOBAL + reloc, 4)
        if not tbl_raw:
            return
        tbl = int.from_bytes(tbl_raw, "little")
        if not tbl:
            return  # 描述符表未分配(启动初始化前),等下次命中再试
        raw = self.read_mem(pid, tbl + vid * 4, 4)
        if not raw:
            print(f"[watch] desc[{vid}] 读取失败")
            return
        desc = int.from_bytes(raw, "little")
        if not desc:
            return
        d = self.read_mem(pid, desc, 0x40) or b""
        if len(d) < 0x28:
            return
        cat, ty, dim = d[0], d[1], d[2]
        bounds = [int.from_bytes(d[4 + i * 4:8 + i * 4], "little")
                  for i in range(min(dim, 7))]
        # 启发式找数据指针:描述符内第一个堆指针(非代码/非描述符自身)
        data_ptr = 0
        for i in range(0x08, 0x40, 4):
            v = int.from_bytes(d[i:i + 4], "little")
            if 0x00450000 < v < 0x7F000000:
                data_ptr = v
                print(f"[watch] desc[{vid}] data_ptr 候选 +{i:#04x} = {v:#010x}")
        print(f"[watch] desc[{vid}] @ {desc:#010x}: cat={cat} type={ty} "
              f"dim={dim} bounds={bounds}")
        hexs = " ".join("%02x" % x for x in d[:0x28])
        print(f"[watch] desc dump: {hexs}")
        if not data_ptr:
            print("[watch] 未找到数据指针,放弃布防")
            return
        self.watch_addr = data_ptr + idx * 8  # 元素 8B 步进
        cur = self.read_mem(pid, self.watch_addr, 8)
        cur_v = int.from_bytes(cur, "little") if cur else None
        print(f"[watch] 当前值 data[{idx}] = {cur_v}")
        h = self.threads.get(tid)
        ctx = self.get_ctx(h)
        if ctx is None:
            print("[watch] GetContext 失败,放弃布防")
            return
        ctx.Dr0 = self.watch_addr
        ctx.Dr7 = DR7_WATCH_WRITE
        ctx.Dr6 = 0
        if self.set_ctx(h, ctx):
            print(f"[watch] Dr0 = {self.watch_addr:#010x} "
                  f"(desc[{vid}][{idx}], 写监视)已布防")
        else:
            print(f"[watch] SetContext 失败 err={ctypes.GetLastError()}")

    def recheck_watch(self, pid):
        """周期重校验:desc 数据指针/元素值是否变化(重分配 → 重新布防)。"""
        vid, idx = self.watch_spec
        reloc = self.procs[pid]["reloc"]
        tbl = int.from_bytes(self.read_mem(pid, DESC_GLOBAL + reloc, 4) or b"\0\0\0\0",
                             "little")
        if not tbl:
            return
        desc = int.from_bytes(self.read_mem(pid, tbl + vid * 4, 4) or b"\0\0\0\0",
                              "little")
        if not desc:
            return
        d = self.read_mem(pid, desc, 0x40) or b""
        if len(d) < 0x34:
            return
        data_ptr = int.from_bytes(d[0x30:0x34], "little")
        val = int.from_bytes(self.read_mem(pid, data_ptr + idx * 8, 8)
                             or b"\0" * 8, "little")
        print(f"[watch] recheck ev={self.n_events}: data_ptr={data_ptr:#010x} "
              f"data[{idx}]={val}")
        if data_ptr + idx * 8 != self.watch_addr:
            print("[watch] 数据指针已变 → 重新布防")
            self.watch_addr = data_ptr + idx * 8
            h = next(iter(self.threads.values()), None)
            ctx = self.get_ctx(h) if h else None
            if ctx is not None:
                ctx.Dr0 = self.watch_addr
                ctx.Dr6 = 0
                self.set_ctx(h, ctx)

    # ---- 主循环 ----
    def run(self, exe, cwd):
        si = STARTUPINFOW()
        si.cb = ctypes.sizeof(si)
        pi = PROCESS_INFORMATION()
        if not _k.CreateProcessW(exe, None, None, None, False,
                                 DEBUG_ONLY_THIS_PROCESS, None, cwd,
                                 ctypes.byref(si), ctypes.byref(pi)):
            print(f"[trace] CreateProcessW 失败: {ctypes.GetLastError()}")
            return False
        self.threads[pi.dwThreadId] = pi.hThread
        self.procs[pi.dwProcessId] = {"hproc": pi.hProcess, "base": 0, "reloc": 0,
                                      "bps": {}}
        self.detect_wow64(pi.hProcess)
        self.t0 = time.time()
        self.selfcheck()
        print(f"[trace] 根进程 pid={pi.dwProcessId} 启动")

        ev = DEBUG_EVENT()
        while True:
            remain = int((self.timeout_s - (time.time() - self.t0)) * 1000)
            if remain <= 0:
                self.stop_reason = "timeout"
                break
            if self.n_events >= self.max_events:
                self.stop_reason = "cap"
                break
            if not _k.WaitForDebugEvent(ctypes.byref(ev), min(remain, 1000)):
                if time.time() - self.t0 >= self.timeout_s:
                    self.stop_reason = "timeout"
                    break
                continue
            code = ev.dwDebugEventCode
            pid = ev.dwProcessId
            status = DBG_CONTINUE

            if code == CREATE_PROCESS_DEBUG_EVENT:
                info = ev.u.CreateProcessInfo
                base = info.lpBaseOfImage
                self.threads[ev.dwThreadId] = info.hThread
                is_child = pid in self.procs
                self.procs[pid] = {"hproc": info.hProcess, "base": base,
                                   "reloc": 0, "bps": {}}
                hide_debugger(info.hProcess)
                if getattr(self, "no_arm", False):
                    self.procs[pid]["reloc"] = base - self.cfg["image_base"]
                    self.procs[pid]["bps"] = {}
                    print(f"[trace] pid={pid} --no-arm:跳过下断 (base {base:#x})")
                else:
                    self.arm_proc(pid)
                if self.watch_spec and self.watch_arm_addr:
                    arm_va = self.watch_arm_addr + (base - self.cfg["image_base"])
                    orig = self.read_mem(pid, arm_va, 1)
                    if orig is not None and self.write_mem(pid, arm_va, b"\xCC"):
                        self.procs[pid]["bps"][arm_va] = orig[0]
                        self.no_rearm.add(arm_va)
                        print(f"[trace] 布防点断点 @ {arm_va:#010x}")
                if self.bp_log_addr:
                    bp_va = self.bp_log_addr + (base - self.cfg["image_base"])
                    orig = self.read_mem(pid, bp_va, 1)
                    if orig is not None and self.write_mem(pid, bp_va, b"\xCC"):
                        self.procs[pid]["bps"][bp_va] = orig[0]
                        print(f"[trace] 日志断点 @ {bp_va:#010x}")
                self.emit({"ev": "meta", "side": "engine",
                           "image_base": base, "pid": pid,
                           "child": is_child})
            elif code == CREATE_THREAD_DEBUG_EVENT:
                self.threads[ev.dwThreadId] = ev.u.CreateThread.hThread
            elif code == EXIT_THREAD_DEBUG_EVENT:
                self.threads.pop(ev.dwThreadId, None)
            elif code == EXCEPTION_DEBUG_EVENT:
                rec = ev.u.Exception.ExceptionRecord
                code_e = rec.ExceptionCode
                if code_e in (EXCEPTION_BREAKPOINT, EXCEPTION_WX86_BREAKPOINT):
                    hit = self.on_breakpoint(pid, ev.dwThreadId)
                    if hit is not None:
                        reloc = self.procs[pid]["reloc"]
                        if self.watch_spec and not self.watch_addr:
                            if self.n_events >= self.watch_arm_ev:
                                if self.watch_arm_addr is not None:
                                    if hit == self.watch_arm_addr + reloc:
                                        self.arm_watch(pid, ev.dwThreadId)
                                else:
                                    self.arm_watch(pid, ev.dwThreadId)
                        pc, sc = self.snapshot_state(pid)
                        self.n_events += 1
                        if (self.bp_log_addr is not None
                                and (self.bp_log_ev is None
                                     or self.n_events == self.bp_log_ev)):
                            hit_va = hit if isinstance(hit, int) else 0
                            if hit_va == self.bp_log_addr + reloc:
                                h = self.threads.get(ev.dwThreadId)
                                ctx = self.get_ctx(h) if h else None
                                if ctx is not None:
                                    print(f"[bplog] ev={self.n_events} "
                                          f"Eax={ctx.Eax:#010x} Ecx={ctx.Ecx:#010x} "
                                          f"Edx={ctx.Edx:#010x} Ebx={ctx.Ebx:#010x} "
                                          f"Esi={ctx.Esi:#010x} Edi={ctx.Edi:#010x} "
                                          f"Ebp={ctx.Ebp:#010x}")
                        if (self.watch_spec and self.watch_addr
                                and self.n_events % 500 == 0):
                            self.recheck_watch(pid)
                        if (self.watch_spec and self.watch_addr
                                and self.n_events <= 30):
                            val = self.read_mem(
                                pid, self.watch_addr, 8)
                            v = int.from_bytes(val, "little") if val else None
                            print(f"[watch] ev={self.n_events} "
                                  f"data[{self.watch_spec[1]}]={v}")
                        if (self.watch_spec and 20 <= self.n_events <= 30):
                            vid0 = self.watch_spec[0]
                            tbl0 = int.from_bytes(
                                self.read_mem(pid, DESC_GLOBAL + reloc, 4)
                                or b"\0\0\0\0", "little")
                            desc0 = int.from_bytes(
                                self.read_mem(pid, tbl0 + vid0 * 4, 4)
                                or b"\0\0\0\0", "little")
                            d0 = self.read_mem(pid, desc0, 0x34) or b""
                            dp = int.from_bytes(d0[0x30:0x34], "little")
                            buf = self.read_mem(pid, dp, 8) or b"\0" * 8
                            print(f"[watch] ev={self.n_events} desc[{vid0}] "
                                  f"cat={d0[0]} type={d0[1]} dim={d0[2]} "
                                  f"bounds={int.from_bytes(d0[4:8], 'little')} "
                                  f"data[0]={int.from_bytes(buf, 'little')}")
                        if self.watch_spec and self.n_events in (21, 22, 23):
                            # 帧指针数组 + 各帧 STR 局部区 hexdump
                            r0 = self.procs[pid]["reloc"]
                            r = self.read_mem(pid, self.cfg["obj_global"] + r0, 4)
                            task = int.from_bytes(r or b"\0\0\0\0", "little")
                            depth = int.from_bytes(
                                self.read_mem(pid, task + 0x140, 1) or b"\0",
                                "little")
                            ptrs_raw = self.read_mem(
                                pid, task + 0x144, (depth + 1) * 4) or b""
                            for fi in range(depth + 1):
                                fp = int.from_bytes(
                                    ptrs_raw[fi * 4:fi * 4 + 4], "little")
                                if not fp:
                                    continue
                                seg = self.read_mem(pid, fp + 0x120, 0x60) or b""
                                nz = any(seg)
                                txt = "".join(
                                    chr(c) if 32 <= c < 127 else "."
                                    for c in seg[:48])
                                sj = seg[:48].decode("cp932", "replace")
                                print(f"[watch] ev={self.n_events} frame[{fi}]"
                                      f"={fp:#010x} str区非空={nz} "
                                      f"head48: {txt} | {sj}")
                        if hit == "default_stub":
                            # 命中「报错 stub」= 引擎执行了无实现命令(异常路径)
                            self.emit({"ev": "default_stub", "seq": self.seq,
                                       "script": sc, "pc": pc, "pid": pid})
                        else:
                            cmds = self.handler_cmds(pid, hit)
                            cmd_field = cmds[0] if cmds and len(cmds) == 1 else cmds
                            self.emit({"ev": "group", "seq": self.seq,
                                       "script": sc, "pc": pc, "cmd": cmd_field,
                                       "pid": pid,
                                       "handler": "0x%08x"
                                                  % (hit - self.procs[pid]["reloc"])})
                        if self.n_events % 5000 == 0:
                            print(f"[trace] {self.n_events} 事件 "
                                  f"({time.time()-self.t0:.1f}s)")
                elif code_e in (EXCEPTION_SINGLE_STEP, EXCEPTION_WX86_SINGLE_STEP):
                    if not self.on_single_step(pid, ev.dwThreadId):
                        status = DBG_EXCEPTION_NOT_HANDLED
                else:
                    key = (code_e, bool(ev.u.Exception.dwFirstChance))
                    self.excs[key] = self.excs.get(key, 0) + 1
                    if sum(self.excs.values()) <= 30:
                        ea = rec.ExceptionAddress or 0
                        base = self.procs.get(pid, {}).get("base") or 0
                        print(f"[trace] 异常 code={code_e:#010x} pid={pid} "
                              f"({'1st' if ev.u.Exception.dwFirstChance else '2nd'}) "
                              f"addr={ea:#010x} (base+{ea - base:#x})")
                        h = self.threads.get(ev.dwThreadId)
                        if h is not None:
                            ctx = self.get_ctx(h)
                            if ctx is not None:
                                print(f"[trace]  寄存器 Eip={ctx.Eip:#010x} "
                                      f"Eax={ctx.Eax:#010x} Ebx={ctx.Ebx:#010x} "
                                      f"Ecx={ctx.Ecx:#010x} Edx={ctx.Edx:#010x} "
                                      f"Esi={ctx.Esi:#010x} Esp={ctx.Esp:#010x}")
                                stack = self.read_mem(pid, ctx.Esp, 256)
                                if stack:
                                    vals = [int.from_bytes(stack[i:i + 4], "little")
                                            for i in range(0, 256, 4)]
                                    marks = [("R" if 0x401000 <= v < 0x477000
                                              else " ") for v in vals]
                                    print("[trace]  栈 " +
                                          " ".join(f"{v:08x}{m}"
                                                   for v, m in zip(vals, marks)))
                    status = DBG_EXCEPTION_NOT_HANDLED
            elif code == EXIT_PROCESS_DEBUG_EVENT:
                code_exit = ev.u.ExitProcess.dwExitCode
                print(f"[trace] 进程退出 pid={pid} code={code_exit:#010x}")
                self.procs.pop(pid, None)
                if not self.procs:
                    self.stop_reason = f"exit(code={code_exit:#010x})"
                    _k.ContinueDebugEvent(pid, ev.dwThreadId, status)
                    break
            elif code == RIP_EVENT:
                self.stop_reason = "rip"
                _k.ContinueDebugEvent(pid, ev.dwThreadId, status)
                break

            if self.stop_reason:
                break
            _k.ContinueDebugEvent(pid, ev.dwThreadId, status)

        # 收尾
        if self.stop_reason in ("timeout", "cap", "rip"):
            for p in self.procs.values():
                _k.TerminateProcess(p["hproc"], 1)
        self.emit({"ev": "done", "reason": self.stop_reason,
                   "events": self.n_events,
                   "elapsed": round(time.time() - self.t0, 2),
                   "exceptions": {"0x%08x/%s" % (k[0], "1st" if k[1] else "2nd"): v
                                  for k, v in self.excs.items()}})
        self.out.close()
        return True


def main():
    ap = argparse.ArgumentParser()
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument("--config", default=os.path.join(here, "engine_trace_config.json"))
    ap.add_argument("--exe",
                    default=r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe")
    ap.add_argument("--out",
                    default=r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine"
                            r"\engine_trace_boot.jsonl")
    ap.add_argument("--max-events", type=int, default=200_000)
    ap.add_argument("--timeout", type=float, default=90.0)
    ap.add_argument("--no-arm", action="store_true",
                    help="不下断点(对照实验:验证引擎是否校验代码段)")
    ap.add_argument("--watch", metavar="ID:IDX",
                    help="硬件写监视 desc[ID] 数组第 IDX 元素(找系统变量写入者)")
    ap.add_argument("--watch-arm-addr", metavar="ADDR",
                    help="提前布防点 VA(启动链内;缺省=首次处理器命中)")
    ap.add_argument("--watch-arm-ev", metavar="N", type=int, default=0,
                    help="延迟布防:第 N 个事件后才尝试 arm_watch"
                         "(用于观察后载脚本消费后的描述符;缺省=0 立即)")
    ap.add_argument("--bp-log", metavar="ADDR",
                    help="任意执行断点:记录命中时寄存器")
    ap.add_argument("--bp-log-ev", metavar="N", type=int,
                    help="仅在第 N 个事件时记录(缺省=全部)")
    a = ap.parse_args()

    with open(a.config, "r", encoding="utf-8") as f:
        cfg = json.load(f)
    cwd = os.path.dirname(a.exe)
    t = Tracer(cfg, a.out, a.max_events, a.timeout)
    t.no_arm = a.no_arm
    if a.watch:
        vid, idx = a.watch.split(":")
        t.watch_spec = (int(vid), int(idx))
    if a.watch_arm_addr:
        t.watch_arm_addr = int(a.watch_arm_addr, 16)
    t.watch_arm_ev = a.watch_arm_ev
    if a.bp_log:
        t.bp_log_addr = int(a.bp_log, 16)
        t.bp_log_ev = a.bp_log_ev
    t.run(a.exe, cwd)
    print(f"[trace] 完成 reason={t.stop_reason} events={t.n_events} -> {a.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
