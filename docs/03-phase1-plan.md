# 03 · 第一阶段开发任务拆分

> Phase 1 目标（对应你的第 26 条）：
>
> ```
> 读取一个 YU-RIS 游戏
>   ↓
> 读取脚本
>   ↓
> 解析 Instruction
>   ↓
> 初始化 VM
>   ↓
> 执行最基本 Opcode
>   ↓
> 产生 Runtime Event
> ```
>
> 排序原则：**对运行真实游戏的重要程度 × 后续依赖程度**。不是按难度排序。

---

## 0. 靶子选择

| 项 | 选择 | 理由 |
|---|---|---|
| 引擎版本 | **555**（YPF 500） | 本目录唯一已实测样本，有完整证据链 |
| 游戏 | `Animal Trail Girlish Square 2` | 有 `bn.ypf`（309 条目）+ 完整引擎数据 |
| 起点脚本 | `$ysbin\yst00000.ybn` | 已解出 402 条指令，密钥已知 |
| 字典 | `%ysbin\ysc.ybn`（YSCM） | 1168 个字符串，opcode 名表 |

**先不碰**：Whirlpool 魔改版（双重校验）、sc.ypf 明文剧本分支。

---

## 1. 任务清单

### P0 · 地基（必须先完成，无风险）

#### P0.1 — `yuris-core`：错误 / Profile / 字节工具

| 项 | 内容 |
|---|---|
| 产出 | `Error`、`Result`、`VersionProfile`、`xor_cyclic`、`xor_name`、字节读取工具 |
| 依赖 | 无 |
| 验收 | `cargo test -p yuris-core` 通过；XOR 往返测试通过 |
| 等级 | Confirmed |

具体：
```rust
// src/error.rs      Error / Result
// src/version.rs    VersionProfile / CharEncoding / OpcodeTableId
// src/bytes.rs      xor_cyclic(data, key, skip) / xor_name(data, key)
//                   read_u32_le / read_cstring_xor ...
```

> 注意：`xor_name` 必须实现「0x00 不参与 XOR」的规则，这是样本实测结果。

---

#### P0.2 — `yuris-format`：YPF

| 项 | 内容 |
|---|---|
| 产出 | `YpfArchive::open()` / `entries()` / `read(path)` |
| 依赖 | P0.1 |
| 验收 | 对 `bn.ypf` 解析出 **309/309** 条目；全部路径形如 `$ysbin\*.ybn`；随机抽 20 条解压后长度 == `uncompressed_len` |
| 等级 | Confirmed |

关键实现点：
- header 4 字段：`magic / version / file_count / first_data_off`
- 索引区 = `[0x20, first_data_off)`
- 每条 entry：`name(C串, XOR 0xC9, 0x00 不加密) + flags(u8) + uncomp(u32) + comp(u32) + offset(u32) + reserved(u32) + tail(8B)`
- `first_data_off` 同时是索引区结束位置 —— **解析完必须断言闭合**

**必写测试**：
```rust
#[test] fn bn_ypf_entry_count_is_309()
#[test] fn bn_ypf_index_consumes_exactly_to_first_data_off()
#[test] fn bn_ypf_roundtrip_decompress_sample_entries()
```

---

#### P0.3 — `yuris-format`：YSTB

| 项 | 内容 |
|---|---|
| 产出 | `YstbFile::parse()` / `slots()` / `slot_content()` |
| 依赖 | P0.1, P0.2 |
| 验收 | 解出 402 条 slot；**相邻 slot 的 offset 严格衔接**（`off[n]+len[n] == off[n+1]`）且零越界 |
| 等级 | Confirmed |

关键实现点：
- header 8 字段：`magic / version / unknown1 / part1_len / command_len / content_len / part4_len / unknown2`
- XOR：4 字节循环，跳过前 0x20 字节。密钥来自 `VersionProfile`（本样本 `2b904f93`）
- `commands` 区按 12 字节切分：`tag(u32) / len(u32) / offset(u32)`
- **保留** `part1` / `part4` 原始字节，不要丢弃（语义未明）

**必写测试**：
```rust
#[test] fn ystb_header_parses_v555()
#[test] fn ystb_xor_key_2b904f93_yields_coherent_slots()   // 决定性测试
#[test] fn ystb_slot_offsets_are_contiguous()
```

> 第三个测试是**密钥正确性的判定器**。如果密钥猜错，offset 一定不连续。
> 这个性质可以直接用来实现「自动猜密钥」工具。

---

#### P0.4 — `yuris-tools`：`dump` / `trace` 骨架

| 项 | 内容 |
|---|---|
| 产出 | CLI：`yuris dump <game_dir>` 输出 `trace.txt` / `trace.jsonl` |
| 依赖 | P0.2, P0.3 |
| 验收 | 能 dump 出 309 条目清单 + 指定 ybn 的 402 条 slot 及原始内容 hex |
| 等级 | Confirmed |

子命令：
```
yuris ypf   <game> [--list] [--extract <path>]
yuris ystb  <game> <script> [--slots] [--raw]
yuris trace <game> [--out FILE] [--format txt|jsonl]
yuris opcode-scan <game>            # ← P1.3 用
```

---

### P1 · 解除最大阻塞（Opcode）

> ⚠️ 这是 Phase 1 里**最重要**的任务。不做完它，VM 无从下手。

#### P1.1 — YSCM 解析

| 项 | 内容 |
|---|---|
| 产出 | `YscmTable`：1168 个字符串 + 索引 ↔ 名称双向映射 |
| 依赖 | P0.2 |
| 验收 | 解析出 ≥1168 个字符串；能按索引查出 `pushint8` 类名称（若存在） |
| 等级 | 存在性 Confirmed，语义 Likely |

要点：
- 需要先把 YSCM 的**二进制结构**搞清楚（字符串表如何组织：定长？偏移表？长度前缀？）
- 先做统计：字符串出现的偏移分布、是否有长度前缀、是否有索引表

---

#### P1.2 — 字节码流反汇编统计

| 项 | 内容 |
|---|---|
| 产出 | 对全部 300 个 YSTB 的 content 区做 opcode 频次统计 |
| 依赖 | P0.3 |
| 验收 | 得到 opcode 编号分布表（哪些出现、出现多少次、操作数长度分布） |
| 等级 | Likely |

方法：
1. 取全部 slot 的 content 字节
2. 按「变长 opcode」假说切分：先假设编码为 `[主码][子码][标志]` + 操作数
3. **用统计验证假设**：合法的 opcode 集合应该收敛（几十到几百种），
   而不是发散（成千上万种）。如果发散，说明切分假设错误，换假设重试
4. 输出：`docs/opcode/opcode-candidates.md`

> 这个「收敛性检验」是关键。切分假设对不对，看 opcode 种类数是否收敛就能判断。

---

#### P1.3 — YSCM ↔ 字节码 交叉比对

| 项 | 内容 |
|---|---|
| 产出 | `docs/opcode/opcode-table.md` —— 第一版 Opcode 规格 |
| 依赖 | P1.1, P1.2 |
| 验收 | 至少比对着确认 **20 个以上** opcode 的编号 → 名称 → 操作数宽度 |
| 等级 | 目标 Confirmed |

比对策略（按证据强度排序）：

1. **频次匹配**：YSCM 中第 N 个字符串 ↔ 字节码中出现频次第 N 高的 opcode
   （依据是 SDK 文章的「按顺序」说法）
2. **结构匹配**：参数名簇（如 `SX SY SLX SLY`）应该在 opcode 表中相邻出现
3. **已知锚点**：Whirlpool 篇已确认的 `pushint8` / `pushint16` / `pushscalarvar` /
   `equal` / `ge` / `le` / `logand` / `logor` —— 拿这些当种子，校准索引偏移
4. **控制流识别**：能改变 PC 的 opcode 应该数量极少且形态特殊

**输出格式**（每个 opcode 一页）：
```markdown
## Opcode 0x??_????  pushint16

- Code: `57 02 00`
- Name: pushint16
- Version: 555
- Operand: imm16 (LE)
- Stack: [] → [Int]
- Control Flow: Continue
- Side Effect: none
- Blocking: No
- Confidence: Confirmed   ← 必须标
- Evidence: Whirlpool 篇反汇编 + 频次统计第 N 位
```

---

### P2 · VM 雏形

#### P2.1 — `yuris-value`：Value + 变量存储

| 依赖 | P1.3（至少知道变量如何寻址） |
|---|---|
| 产出 | `Value` enum、`VariableStore`、`VariableRef` |
| 验收 | 类型转换的单测；store 的快照/恢复测试 |
| 阻塞 | **U2 变量系统未解** —— 若 P1.3 未拿到变量相关 opcode，先实现骨架并标 `Unimplemented` |

---

#### P2.2 — `yuris-script`：Instruction 解码器

| 依赖 | P1.3 |
|---|---|
| 产出 | `Decoder`：变长字节码 → `Vec<Instruction>` |
| 验收 | 对 `yst00000.ybn` 的 content 区，能完整切分且**无残留字节** |
| 关键 | 未识别 opcode 输出 `Opcode::Unresolved(code)` + 保留原始字节，**绝不猜** |

---

#### P2.3 — `yuris-runtime`：Backend trait 定义

| 依赖 | 无（可与 P1 并行） |
|---|---|
| 产出 | `RuntimeApi` / `GraphicsBackend` / `AudioBackend` / `InputBackend` / `StorageBackend` |
| 验收 | 能用 mock 实现跑通一个空循环 |

---

#### P2.4 — `yuris-vm`：VM 骨架 + Suspend/Resume

| 依赖 | P2.1, P2.2, P2.3 |
|---|---|
| 产出 | `YurisVm::run(budget) -> VmSuspend` / `resume()` / `step()` |
| 验收 | 能执行 `yst00000.ybn` 的前 N 条指令并在第一个挂起点正确返回 |
| 关键 | **绝不阻塞**；未实现的 opcode 返回 `Error::Unimplemented` 并带 opcode 码 |

---

#### P2.5 — 最小 Backend（软渲染 + 空音频 + mock 输入）

| 依赖 | P2.3 |
|---|---|
| 产出 | `NullBackend`（全空，用于测试）+ `SoftRender`（可选，输出 PPM/PNG 截图） |
| 验收 | Golden Test 能无头运行 |

---

### P3 · Phase 1 收尾

#### P3.1 — Trace 输出完善
#### P3.2 — Golden Test 框架搭好（先有框架，期望值后填）
#### P3.3 — 更新 `PROGRESS.md`，标注 U1 是否已解除

---

## 2. 执行顺序（甘特式）

```
Week 1
├─ P0.1  core  ─────────┐
├─ P0.2  YPF   ─────────┼──► 309/309 条目
└─ P0.3  YSTB  ─────────┘──► 402/402 指令

Week 2
├─ P0.4  dump/trace 工具
├─ P1.1  YSCM 解析  ─────┐
├─ P1.2  字节码统计  ────┼──► opcode 频次表
└─ P1.3  交叉比对   ─────┘──► ★ opcode-table.md（解除 U1）

Week 3
├─ P2.3  Backend trait（可提前并行）
├─ P2.1  Value / 变量
├─ P2.2  Instruction 解码
└─ P2.4  VM 骨架

Week 4
├─ P2.5  最小 Backend
└─ P3.x  Trace / Golden Test 框架
```

**并行建议**：P2.3（Backend trait）不依赖 opcode 表，可以从 Day 1 就开始，与 P0/P1 并行。

---

## 3. 每个任务的「完成」定义

不满足以下任一条，**不得**标记为完成：

| 条件 | 说明 |
|---|---|
| ✅ 有 `cargo test` 覆盖 | 至少 1 个真实样本数据的断言 |
| ✅ 数据能从样本复现 | 用了真实 `bn.ypf` / 真实 ybn，不是人造假数据 |
| ✅ `PROGRESS.md` 已更新 | 含验证方式（别人能照做复现） |
| ✅ 结论标注了等级 | Confirmed / Likely / Hypothesis / Unknown |
| ✅ 未知项显式 `Unimplemented` | 没有硬编码的猜测值 |

---

## 4. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| **YSCM 索引与 opcode 编号不是简单对应** | P1.3 失败，U1 无法解除 | 回退到运行时观察：Hook 原版 Runtime 抓 opcode 执行轨迹 |
| **opcode 变长切分假设错误** | P1.2 发散 | 用收敛性检验快速证伪，换假设；或用 SDK 编译器做对照实验（改明文脚本 → 观察编译产物变化） |
| **样本游戏的 opcode 表与其他版本不同** | 兼容性差 | 先做 v555 一个版本，跑通后再扩 |
| **`part1`/`part4` 是关键数据却被忽略** | VM 行为不对 | 已原样保留，需要时可回查 |
| **自动猜密钥失败** | 换游戏就卡住 | 用 offset 连续性做判定器（P0.3 的测试就是这个性质），可暴力搜索 |

---

## 5. Phase 1 之后（不现在做，先列着）

| Phase | 内容 |
|---|---|
| 2 | Scene / 渲染 / 文本 / 图像解码 —— 让画面出来 |
| 3 | 音频 / 输入 / 动画 —— 让游戏可玩 |
| 4 | 存档 / 分支 / 端到端跑通一个场景 |
| 5 | 版本 Profile 扩展（v4xx / v48x / v494 / 其他 v5xx） |
| 6 | Golden Test 自动化 + 与原版 Runtime 批量对比 |

---

## 6. 立即可以开始的第一行代码

按依赖最小化，第一个 PR 应该是：

```
P0.1 yuris-core   → error.rs / bytes.rs（含 xor_cyclic、xor_name）
P0.2 yuris-format → ypf.rs（含 bn_ypf 的 3 个断言测试）
P0.3 yuris-format → ystb.rs（含 offset 连续性的决定性测试）
P0.4 yuris-tools  → dump 子命令
```

这四步全部基于 **Confirmed** 证据，零猜测，可以一次做到位。
