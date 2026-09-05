# -*- coding: utf-8 -*-
"""P5.2 —— 重新对拍最新 trace,结果落盘。"""
import io
import sys

sys.argv = ["diff",
            r"D:\yuris-kernel\crates\yuris-vm\tests\golden\engine\engine_trace_boot.jsonl",
            r"C:\Users\weiss\AppData\Local\Temp\vm_v8.jsonl"]
sys.path.insert(0, r"D:\yuris-kernel\scripts")
buf = io.StringIO()
old = sys.stdout
sys.stdout = buf
ns = {"__name__": "__diff__"}
rc = None
try:
    exec(compile(open(r"D:\yuris-kernel\scripts\diff_engine_vm.py",
                      encoding="utf-8").read(), "diff_engine_vm.py", "exec"), ns)
    if "main" in ns:
        rc = ns["main"]()
    else:
        rc = "no-main"
except SystemExit as e:
    rc = "SystemExit:%s" % e.code
except Exception as e:
    rc = "EXC:%r" % e
finally:
    sys.stdout = old
with open(r"C:\Users\weiss\AppData\Local\Temp\diff6.txt", "w", encoding="utf-8") as f:
    f.write(buf.getvalue() + "\nrc=%s\n" % rc)
