# Windows 交接文档 —— 立绘投影取证（2026-09-12）

承接 macOS 会话：立绘布局修复推进到「投影公式取证」节点。目标是在 Windows 侧
运行原引擎 + `engine_trace` 探针取回投影真值，然后回填 `yuris-player-core` 的
`show_tachie`。本文件是唯一交接说明；进度真相仍以 `PROGRESS.md` 为准。

---

## 1. 环境与素材

| 项 | 说明 |
| --- | --- |
| Rust | stable（macOS 侧已验证 1.96.0；Windows 用 MSVC toolchain） |
| Python | 3.12（`scripts/` 下探针与 `engine_trace.py`） |
| 游戏 | 例：`D:\yuris-kernel\AnimalTrailGirlishSquare 2\`（`kemonomichi2.exe` + `pac/`） |
| 调试器 | `engine_trace.py` 必须 Windows：64 位 Python 调 32 位引擎（WOW64 上下文） |

构建与验证（Windows 首次会全量编译，几分钟）：

```bat
cargo test --workspace
:: 基线：49 套件 / 165 passed

cargo build --release -p yuris-cli
cargo run --release -p yuris-cli -- run "D:\yuris-kernel\AnimalTrailGirlishSquare 2"
```

---

## 2. 当前任务：投影真值探针（优先级最高）

### 2.1 背景（本轮已确证）

- `MATH3D`（YSCM 61，handler `0x4466bc`）是**回读**：handler 内唯一调用
  `0x446a04`（投影函数，`0x446a04–0x447d47`，4932B x87），它把结果写入全局
  `0x871f68/70/78/80`（= 屏幕 x / y / scaleX / scaleY），MATH3D 再把 4 个
  double 拷进脚本目标（`@0x16ab[31..34]`）。
- 脚本链：`es.SP.XYZCALC`（s177 pc1510）末尾 `MATH3D @0x16aa @0x16ab` →
  `es.SP.DRAW`（s180）用回读值算出
  `CG FX=out31*100, FY=out32*100, FBX/FBY/FCX/FCY…`。
- 缺的只有 `0x446a04` 的公式；已选定钩点 `0x447d3b`（4 个全局写完、函数返回前）。

### 2.2 探针命令

```bat
python scripts\engine_trace.py --exe "D:\yuris-kernel\AnimalTrailGirlishSquare 2\kemonomichi2.exe" ^
  --out math3d.jsonl --timeout 120 ^
  --bp-log 0x447d3b --bp-dump 0x871f68:64 --bp-dump esp+0x0:0x140
```

操作：等游戏窗口出现后玩到 **maho2_01** 三立绘同屏场景，停约 2 秒后关游戏或
等超时。可选再加 `--bp-dump 0x871f88:32`。

场景已知输入（`sc.ypf → maho2_01.txt`，供拟合回归）：

- `\T(M_LOP_1A0100, 800, 876, 162, 200)`
- `\T(K_PEN_1A0100, 800, -270, 114, 200)`
- `\T(L_NYA_1A0500, 250)`（未给 x/y/z，默认位）
- 相机：`\S.CLXYZ(m,2000,0,-50,0,0,0)` / `\S.CLXYZ(k,1800,0,-40,0,0,0)`

产物：JSONL 中每条 `{"ev":"bplog","script","pc","hit","eax","edx","esp","dumps":[…]}`，
`dumps` 里 `hex` = 内存原样（小端）。

### 2.3 回传后解析（示例）

```python
import json, struct
for line in open('math3d.jsonl', encoding='utf-8'):
    e = json.loads(line)
    if e.get('ev') != 'bplog':
        continue
    for d in e.get('dumps', []):
        if d.get('hex') and int(d['addr'], 16) & 0xFFFF == 0x1f68:
            x, y, sx, sy = struct.unpack('<4d', bytes.fromhex(d['hex'])[:32])
            print(e['script'], e['pc'], x, y, sx, sy)
```

- `0x871f68 = f64 x`、`0x871f70 = f64 y`、`0x871f78 = scaleX`、`0x871f80 = scaleY`。
- `esp+0x0..0x140`：x87 帧参数（r2 命名 `arg_4h..arg_124h`，见
  `esp+0x68`/`arg_68h` 等），即投影输入。
- 建议对照钩 `--bp-log 0x446a04`（投影入口）确认输入布局。

### 2.4 落地清单（拿到真值后）

1. 拟合/验证公式（必要时逐项对照 `0x446a04` 反汇编，r2 用法见 §3）。
2. `crates/yuris-player-core/src/lib.rs` 的 `show_tachie`：替换 `960+x` 占位与
   `TACHIE_SCALE=0.93`，改用投影结果；保留浮动动画（幅度 18px / 周期 2.8s）。
3. 补单测（合成输入 → 期望屏幕坐标）；更新 `docs/layout-fix-plan.md`（P3 前）
   与 `PROGRESS.md`（成果/勘误留痕）。
4. 回归：用参考图实测 left = 232/794/1254（三角色）校验。

---

## 3. 本轮已确证结论与速查（别丢）

- **处理器表映射勘误**：表槽 = YSCM 命令 id，**无偏移**。`0x423080` 是 3 字节
  `xor eax,eax; ret` 空桩，被 id 0/9/84/97/102/103/105（ALIAS / DEBUGLIST /
  VARACT / VARINFO / WINDOW / TASKINFO）共用。旧结论「CG=0x423080」错。
  正确：CG(1)=`0x423864`、CGACT(2)=`0x42607c`、MATH3D(61)=`0x4466bc`、
  LET(53)=`0x443808`。`scripts/engine_trace_config.json` 的 `cmd_map` 已是正确映射。
- **CG 参数窗口 B0 = YSCM 参数下标**：`0x1c` FX、`0x1d` FY、`0x1e` FBX、
  `0x1f` FBY、`0x20` FRX、`0x21` FRY、`0x22` FRZ、`0x23` FCX、`0x24` FCY、
  `0x28` FSD、`0x29` FMD、`0x2b` FID、`0x2c` F、`0x36` MODE。
- 关键地址：`0x446a04` 投影函数（写 `0x871f68/70/78/80`，仅在 MATH3D 内被调）；
  `0x447d3b` 写完全局的返回前钩点；`0x4466bc` MATH3D 入口。
- r2 用法：`r2 -q -e scr.color=0 -e bin.relocs.apply=true -c 's <VA>; af; pdf' kemonomichi2.exe`；
  `.text` 段 VA = 文件偏移 + `0x400C00`。
- 常量：YSTB XOR key `2b904f93`；YPF name key `0xC9`。
- 取证工具（已随包）：
  - `scripts/tmp_disasm.py`（YSTB 反汇编；该文件被 `.gitignore` 的 `tmp_*.py`
    规则忽略，包里已附带）：
    `python scripts\tmp_disasm.py <游戏目录> <script> <pc_start> [pc_end]`
  - `crates/yuris-vm/examples/tmp_st_layout.rs`、`tmp_xyzcalc.rs`（VM 侧取证）。
  - `scripts/engine_trace.py`：本轮新增 `--bp-dump ADDR:LEN`（可多次，支持
    `esp+OFF`），命中日志断点（`--bp-log`）时随 `bplog` 事件写 JSONL。
- 已知待办（非本任务）：VARINFO `STRFIRST(15)`/`SJISCODE(16)` 未实现；
  P3 `\FACE`/`\EV` 未实装；UI 状态机（AUTO/SKIP 等）仅做了可见性过滤。

---

## 4. 归档说明

- zip 排除了 `target/`（macOS 构建产物，Windows 无用）、`.DS_Store`、
  `__pycache__`、`ui.log`。
- 包含：`.git` 全历史、**未提交的改动**、`docs/reverse/decompiled/`（152K
  反编译参考，已被 `.gitignore`）、全部取证脚本与临时工具。
- 游戏本体不在包内：Windows 侧用现有安装目录（见 §1）。

## 5. 纪律提醒

- 证据分级（Confirmed / Likely / Hypothesis / Unknown）；新结论写 `PROGRESS.md`
  （成果 / 勘误必须留痕，不静默改）。
- git 提交由用户决定；游戏资源、trace/JSONL 数据不入库。
