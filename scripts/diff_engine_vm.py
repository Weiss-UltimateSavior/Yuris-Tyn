#!/usr/bin/env python3
"""P1 —— 引擎真值 vs Rust VM 事件流对拍。

对拍域:两边各自重建 (script, pc, cmd) 三元组序列。
  - 引擎侧:scripts/engine_trace.py 产物,每事件直接含 script/pc/cmd
    (cmd 为共享处理器时是候选列表,任一命中即算匹配);
    default_stub 命中单列为异常。
  - VM 侧:crates/yuris-vm/examples/vm_trace.rs 产物,GroupExecuted 事件
    不含 script;按 ScriptSwitch(to) 重建当前脚本(初始 = meta.entry_script)。

判定:前缀逐元素对齐(引擎实时帧率与 VM 全速驱动的循环次数天然不同,
但只要语义一致,循环体在两边重复同一 (script,pc,cmd) 模式,前缀对齐成立)。
diff 为空 → 语义一致;否则逐项输出差异 + 上下文。

用法:
  python scripts/diff_engine_vm.py <engine.jsonl> <vm.jsonl> [--show N]
"""
import argparse
import json
import sys


def load_groups(path, side):
    # VM 语义事件 → YSCM 命令 id(与 yuris_vm::cmd 常量一致):
    # 这些命令在 VM 里只发语义事件(无 GroupExecuted),引擎侧同一派发
    # 以 group 形式出现,须归一到同一 (script,pc,cmd) 域。
    SEMANTIC_CMD = {"text": 0x62, "cg": 0x01, "sound": 0x59, "cgact": 0x02,
                    "cginfo": 0x04, "cgend": 0x03, "load": 0x36, "save": 0x56}
    groups = []       # [(script, pc, cmd 或 [候选])]
    stubs = []        # 引擎命中报错 stub 的位置
    entry = None
    switches = 0
    cur = None
    done = None
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            e = json.loads(line)
            if e.get("ev") == "meta":
                if side == "vm":
                    entry = e.get("entry_script")
                    cur = entry
                continue
            if e.get("ev") == "done":
                done = e
                continue
            if side == "vm":
                ev = e["ev"]
                if ev == "switch":
                    cur = e["to"]
                    switches += 1
                elif ev == "group":
                    groups.append((cur, e["pc"], e["cmd"]))
                elif ev in SEMANTIC_CMD:
                    groups.append((cur, e["pc"], SEMANTIC_CMD[ev]))
                elif ev in ("decl", "unsupported", "varquery", "sub"):
                    groups.append((cur, e["pc"], e["cmd"]))
            else:
                if e["ev"] == "group":
                    cmd = e["cmd"]
                    if isinstance(cmd, list):
                        # 共享 no-op 处理器多 cmd 表(0x00/0x09/0x4d/0x50/
                        # 0x52-0x54 = FUN_00423080)命中间址断点:多数是
                        # 表达式求值器 00420acc 内部 op 派发(表 007e1660)
                        # 触发的伪影(成果 55),非真实命令派发 → 剔除。
                        # 真实命令层派发时 cmd 必为标量(处理器表按 cmd 索引)。
                        continue
                    groups.append((e["script"], e["pc"], cmd))
                elif e["ev"] == "default_stub":
                    stubs.append(e)
    return groups, stubs, entry, switches, done


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("engine")
    ap.add_argument("vm")
    ap.add_argument("--show", type=int, default=10, help="显示前 N 处差异详情")
    a = ap.parse_args()

    eg, stubs, _, _, edone = load_groups(a.engine, "engine")
    vg, _, ventry, vsw, vdone = load_groups(a.vm, "vm")
    print(f"[diff] 引擎 {len(eg)} 组(default_stub 命中 {len(stubs)}),"
          f"VM {len(vg)} 组(entry script {ventry},switch {vsw} 次)")
    if edone:
        print(f"[diff] 引擎停止原因 {edone.get('reason')} elapsed={edone.get('elapsed')}s;"
              f"VM {vdone.get('reason') if vdone else '?'}")

    if stubs:
        print(f"[diff] ⚠ 引擎命中无实现处理器(default_stub):前 5 条:")
        for s in stubs[:5]:
            print(f"    script={s.get('script')} pc={s.get('pc')}")
        print("[diff]   (声明族命令本不应被运行期执行;出现即语义偏离信号)")

    n = min(len(eg), len(vg))
    # 前缀对齐 + 重同步:分歧后寻找两侧再次连续一致(RSYNC 窗)的位置,
    # 每个分歧簇计 1 个「分歧点」;无法重同步则剩余全部算 1 个尾簇。
    RSYNC = 4
    mismatches = []   # (i, engine triple, vm triple)
    i = 0
    while i < n:
        es, epc, ecmds = eg[i]
        vs, vpc, vcmd = vg[i]
        # 引擎侧 cmd 恒为标量(共享 stub list 已在装载时剔除;成果 55)
        e_set = ecmds if isinstance(ecmds, list) else [ecmds]
        v_set = vcmd if isinstance(vcmd, list) else [vcmd]
        if es == vs and epc == vpc and set(v_set) & set(e_set):
            i += 1
            continue
        # 记录分歧点,尝试重同步
        mismatches.append((i, (es, epc, ecmds), (vs, vpc, vcmd)))
        j = i + 1
        resync = None
        limit = min(n, i + 5000)
        while j < limit:
            ok = all(
                eg[j + k][0] == vg[j + k][0]
                and eg[j + k][1] == vg[j + k][1]
                and (set(vg[j + k][2] if isinstance(vg[j + k][2], list)
                         else [vg[j + k][2]])
                     & set(eg[j + k][2] if isinstance(eg[j + k][2], list)
                           else [eg[j + k][2]]))
                for k in range(RSYNC)
                if j + k < n
            ) and j + RSYNC <= n
            if ok:
                resync = j
                break
            j += 1
        if resync is None or len(mismatches) > 64:
            break
        i = resync
    print(f"[diff] 对齐范围 min={n};差异 {len(mismatches)} 处")

    def fmt(t):
        s, pc, c = t
        cs = c if isinstance(c, str) else ("0x%02x" % c if isinstance(c, int)
                                           else "/".join("0x%02x" % x for x in c))
        return f"script={s} pc={pc} cmd={cs}"

    for i, e, v in mismatches[:a.show]:
        ctx_e = " | ".join(fmt(t) for t in eg[max(0, i - 2):i + 3])
        ctx_v = " | ".join(fmt((s, pc, c if isinstance(c, int) else [c]))
                           for s, pc, c in vg[max(0, i - 2):i + 3])
        print(f"\n  [{i}] 引擎: {fmt(e)}\n      VM   : {fmt(v)}")
        print(f"      引擎上下文: {ctx_e}")
        print(f"      VM   上下文: {ctx_v}")

    if len(mismatches) == 0:
        if len(eg) != len(vg):
            longer = "引擎" if len(eg) > len(vg) else "VM"
            print(f"[diff] ✅ 对齐区间内 diff 为空;{longer}流更长"
                  f"(引擎 {len(eg)} vs VM {len(vg)})—— 截断差异,非语义分歧")
        else:
            print("[diff] ✅ 全程 diff 为空:引擎真值与 Rust VM 事件流完全一致")
        return 0
    print(f"[diff] ❌ 存在 {len(mismatches)} 处语义分歧(共对齐 {n} 组)")
    return 1


if __name__ == "__main__":
    sys.exit(main())
