# 01 · YU-RIS Runtime 架构分析

> 基于 `/Users/weiss/Desktop/yuris` 现有资料 + 对样本游戏 `Animal Trail Girlish Square 2`
> （引擎版本 555 / YPF 版本 500）的二进制实测。
>
> 所有结论标注等级：**Confirmed / Likely / Hypothesis / Unknown**。
> 本文不猜测；没有证据的地方一律写 Unknown。

---

## 目录

1. [分层架构](#1-分层架构)
2. [各层职责与关系](#2-各层职责与关系)
3. [已确认部分 / 部分确认 / 未知](#3-已确认--部分确认--未知)
4. [优先级排序](#4-优先级排序)
5. [关键设计决策](#5-关键设计决策)
6. [与目标的距离](#6-与目标的距离)

---

## 1. 分层架构

YU-RIS 不是一个"脚本解释器"，而是一个**完整的游戏运行环境**。按数据流划分：

```
┌──────────────────────────────────────────────────────────────────┐
│  L0  存储层      YPF 封包（zlib + 条目索引 + 文件名 XOR）            │
│                  ├─ bn.ypf / ysbin.ypf  → 脚本与引擎数据            │
│                  ├─ cg.ypf / bgm.ypf / vo.ypf / se.ypf  → 资源      │
│                  └─ sc.ypf（部分作品）→ 明文剧本                     │
└────────────────────────────┬─────────────────────────────────────┘
                             ↓  解包 / 解压 / 解 XOR
┌──────────────────────────────────────────────────────────────────┐
│  L1  容器解码层   YSTB / YSCF / YSCM / YSLB / YSVR / YSER / YSTL   │
│                  每种 magic 一个容器，各自有 header + 分区           │
└────────────────────────────┬─────────────────────────────────────┘
                             ↓
┌──────────────────────────────────────────────────────────────────┐
│  L2  脚本层      YSTB.commands → 12 字节定长槽位描述符表            │
│                  YSTB.strs     → 变长 VM 字节码流                   │
│                  得出 Instruction 流（IR）                          │
└────────────────────────────┬─────────────────────────────────────┘
                             ↓
┌──────────────────────────────────────────────────────────────────┐
│  L3  VM 层       PC / 值栈 / 调用栈 / 变量区                        │
│                  取指 → 译码 → 执行                                 │
│                  **可挂起**（suspend/resume，非阻塞）                │
└────────────────────────────┬─────────────────────────────────────┘
                             ↓  产生 Runtime Event
┌──────────────────────────────────────────────────────────────────┐
│  L4  Runtime API  Kernel 与平台之间的**唯一**边界                    │
│                  Graphics / Audio / Input / Storage / Clock        │
│                  （trait，不是具体实现）                            │
└────────────────────────────┬─────────────────────────────────────┘
                             ↓
┌──────────────────────────────────────────────────────────────────┐
│  L5  状态层      Scene（Layer 树）/ Text / Animation / Save        │
│                  **纯状态**，不碰任何平台 API                        │
└────────────────────────────┬─────────────────────────────────────┘
                             ↓
┌──────────────────────────────────────────────────────────────────┐
│  L6  Backend     渲染 / 音频 / 输入 / 存储 的平台实现                │
│                  wgpu / sdl2 / cpal / rodio / … Kernel 不感知       │
└──────────────────────────────────────────────────────────────────┘
```

### 依赖方向（严格单向，无环）

```
L0 存储  ←  L1 容器  ←  L2 脚本  ←  L3 VM  →  L4 Runtime API（trait）
                                              ↑
                                     L5 状态层 ┘
                                              ↑
                                     L6 Backend（实现 trait）
```

**关键约束**：L3（VM）**只能**通过 L4 的 trait 影响外部世界。VM 永远不直接调用
`wgpu` / `SDL` / 文件 API / 系统时钟。这是可测试性和可移植性的基础，也是
Golden Test 能成立的前提。

---

## 2. 各层职责与关系

### L0 · 存储层

| 项 | 内容 |
|---|---|
| 职责 | 把 `.ypf` 封包里的条目按需取出来，还原成原始字节 |
| 输入 | 封包文件路径 + 内部路径（如 `$ysbin\yst00034.ybn`） |
| 输出 | `Vec<u8>`（解压 + 解 XOR 后的原始容器数据） |
| 关键性质 | **懒加载**。不把 1.6 GB 全读进内存，按路径索引 seek + 读 + 解压 |

样本实测的封包构成（16 个 ypf，约 1.6 GB）：

```
bn.ypf      1.4 MB   ★ 引擎与脚本（309 条目）
sc.ypf    327 KB     剧本（本样本是否明文待验）
cg.ypf    827 MB     立绘 / CG
bgm.ypf    62 MB
se.ypf     32 MB
vo.ypf    107 MB     语音
sysvo.ypf 5.5 MB
sysse.ypf  41 KB
op.ypf    125 MB
op_c.ypf  125 MB
cgsys_ec.ypf 83 MB
update1.ypf 491 MB
mv001~004.ymv         影片
```

`bn.ypf` 内部 309 条目的类型分布（实测）：

| magic | 数量 | 说明 |
|---|---|---|
| `YSTB` | 302 | 脚本正文 |
| `YSCM` | 1 | **Opcode / 参数名表**（`ysc.ybn`） |
| `YSCF` | 1 | 工程配置（`yscfg.ybn`） |
| `YSER` | 1 | 疑似错误/消息表（`yse.ybn`） |
| `YSLB` | 1 | （`ysl.ybn`，139 KB） |
| `YSVR` | 1 | （`ysv.ybn`，52 KB） |
| 非 zlib | 2 | stored |
| `comp_len = 0` | 1 | 空条目 |

### L1 · 容器解码层

每种 magic 一个容器。**已确认的三种**：

| magic | 文件 | 结构 |
|---|---|---|
| `YSTB` | `yst%05d.ybn` | 见 `docs/formats/ystb.md`，8 字段 header + 4 个分区 |
| `YSCF` | `yscfg.ybn` | magic + version + 配置字段 + caption |
| `YSCM` | `ysc.ybn` | **Opcode/参数名字符串表** |

**待确认**：`YSER` / `YSLB` / `YSVR` / `YSTL` / `YSTD`

关于 `YSTD`：现有资料中未出现，样本中也未发现 —— **Unknown**（可能属于其他版本或其他用途）。

### L2 · 脚本层

**这是本次实测最重要的发现，直接决定 VM 的设计。**

YSTB 的 `commands` 区是一个**定长 12 字节的记录表**：

```rust
pub struct CommandSlot {
    pub tag:    u32,   // 内容类型标签（YSTB_FILE.py 称之为 opcode）
    pub len:    u32,   // 内容长度
    pub offset: u32,   // 相对 strs 区起始的偏移
}
```

而这些槽位**指向的 `strs` 区，存放的是变长 VM 字节码**，不是明文文本：

实测（样本 `yst00000.ybn`，密钥 `2b904f93` 解密后）：

```
槽位 #0:  tag=0x00000000  len=22  offset=0
  → strs[0..22] = 56 03 00 24 ca 04 | 57 02 00 90 01 | 42 01 00 01 | 2b 00 00 | 29 01 00 00 | 4d 02 00 22 22
槽位 #1:  tag=0x00030000  len=5   offset=22
槽位 #2:  tag=0x00000000  len=22  offset=27
...（402 条，offset 严格递增且无缝衔接，零越界）
```

其中 `57 02 00` / `42 01 00` / `56 03 00` 的编码，与
`[YU-RIS] Whirlpool社的一些观察` 一文记录的 VM 字节码**同一套**：

```
48030040 FB0A  pushscalarvar FB0A     ← 6 字节（4 字节 opcode + 2 字节操作数）
420100   5F    pushint8 0x5F          ← 4 字节（3 字节 opcode + 1 字节操作数）
570200   8100  pushint16 0x8100       ← 5 字节（3 字节 opcode + 2 字节操作数）
3D0000         equal                  ← 3 字节（无操作数）
5A0000         ge
530000         le
260000         logand
7C0000         logor
```

**推论（Likely）**：opcode 是变长编码，形如 `[主码][子码][标志]` + 可选操作数。
`57 02 00` 中的 `02` 疑似操作数宽度标记。这需要 YSCM 交叉比对来确认。

**这个结构对项目的意义**：

按 `Methodology/基于虚拟机字节码的文本修改思路` 的分类，YU-RIS 属于
**「指令与数据分离」** 而非「指令与数据混合」。意味着：

- 修改文本不需要重排指令、不需要修跳转
- 追加内容不影响既有偏移
- 但**要真正"运行"而不是"改文本"，我们必须实现完整 VM** —— 这正是本项目要做的事

### L3 · VM 层

**目前 Unknown 最多的一层。** 已知线索：

| 线索 | 来源 | 等级 |
|---|---|---|
| 存在 `pushint8` / `pushint16` / `pushscalarvar` | Whirlpool 篇反汇编 | Confirmed（该版本） |
| 存在 `ge` / `le` / `equal` / `logand` / `logor` | 同上 | Confirmed（该版本） |
| 存在标量变量（scalar var），编号如 `FC0A` / `FB0A` / `FF0A` | 同上 | Confirmed（该版本） |
| 有值栈（push/eval 模型） | 同上 | Likely |
| 有变量区、调用栈、标签跳转 | 推断 | Hypothesis |
| 挂起/恢复点（等点击、等选择、等定时器） | GalGame 通用行为 | Hypothesis |

**设计上必须预留**，即便语义未知：

```rust
pub enum VmState {
    Running, WaitingInput, WaitingChoice, WaitingTimer,
    WaitingAudio, WaitingVideo, Paused, Finished, Error,
}
```

### L4 · Runtime API

Kernel 与外界的唯一边界。VM 不感知平台。

从 YSCM 的参数名可以**反推** Runtime 需要提供哪些能力（Likely）：

| YSCM 参数名簇 | 推断的 Runtime 能力 |
|---|---|
| `CG` `:ID` `IDNO` `ID2..ID4` `GID` | 图像资源引用与句柄 |
| `SX` `SY` `SLX` `SLY` `SCX` `SCY` | 位置 / 缩放 / 裁剪 |
| `RZ` `RLX` `RLY` `RCX` `RCY` | 旋转 / 旋转中心 |
| `SIP` `RIP` `TSX` `TSY` `MIPMAP` `TEX` | 采样 / 插值 / 纹理 |
| `DXBUF` `DXDRAW` | 离屏缓冲 / 直接绘制 |
| `FX` `FY` `FBX` `FBY` `FRX` `FRY` `FRZ` `FCX` `FCY` `FEX` `FEY` `FEZ` | 变换矩阵 / 3D 风格参数 |
| `FSD` `FMD` `FQU` `FID` | 绘制模式 / 质量 / ID |
| `SD` `MD` `MODE` | 语音 / 音乐 / 模式 |
| `TA` `TID` `TIDNO` | 文本区域 / 文本 ID |
| `CASH` `LINT` `LINT2` | 缓存 / 插值 |
| `FILE` `MFILE` `RID` `MID` | 文件与资源 ID |
| `BMP` `PNG` `JPG` `GIF` `AVI` `PSB` `WEBP` `WAV` `OGG` | 支持的媒体格式 |
| `CAPTION` `THREAD` `DEBUGMODE` `SOUND` `COMPILE` `WINDOWRESIZE` `WINDOWFRAME` `FILEPRIORITY*` | 引擎配置 |

> 这张表是 **Likely**（按名称推断），不是 Confirmed。但它为 Runtime API 的
> **接口面**提供了非常有价值的先验 —— 至少我们知道要准备哪些能力槽位。

### L5 · 状态层

Scene / Layer / Text / Animation / Save —— 纯数据，无副作用。

```rust
pub struct Scene { pub layers: Vec<Layer> }

pub struct Layer {
    pub id: u32, pub z: i32, pub visible: bool,
    pub x: f32, pub y: f32,
    pub scale_x: f32, pub scale_y: f32,
    pub alpha: f32, pub rotation: f32,
    pub resource: Option<ResourceId>,
}
```

**数据流**：`VM → Scene State → Renderer`。VM 绝不直接调用绘图 API。

### L6 · Backend

平台实现。Kernel 只依赖 L4 的 trait。

| Backend | 候选实现 |
|---|---|
| Graphics | wgpu（跨平台、现代）/ softbuffer（无 GPU 的参考实现，用于 Golden Test） |
| Audio | cpal + rodio |
| Input | winit 事件 或 自定义 |
| Storage | std::fs + 目录映射（支持免封包覆盖） |

---

## 3. 已确认 / 部分确认 / 未知

### ✅ Confirmed（可直接写实现 + 单测）

| # | 项 | 证据 |
|---|---|---|
| C1 | YPF header 4 字段（magic/version/count/first_data_off） | 样本解析闭合 |
| C2 | YPF entry 布局（name/flag/uncomp/comp/off/zero/tail8） | 尺寸算术 `303×45+5×40+1×42=13877` 精确 |
| C3 | YPF 文件名单字节 XOR 0xC9（0x00 终止符不加密） | 解出 309 条合法路径 |
| C4 | YPF 条目数据为 zlib（flag=1） | 304/309 解压成功，长度吻合 |
| C5 | YSTB header 8 字段 | `0x20+808+4824+4737+808 == 11209 == 文件大小` |
| C6 | YSTB 4 字节循环 XOR，跳前 0x20 字节 | 密钥 `2b904f93` 使 402 条指令偏移严格自洽 |
| C7 | YSTB command 定长 12 字节（tag/len/offset） | `command_len % 12 == 0`，402 条零越界 |
| C8 | opcode `0x00000000`（text）与 `0x00030000`（opt） | 与 `YSTB_FILE.py` 记录逐字节一致 |
| C9 | YSCF 存在且含 version/screen/caption | 106 字节样本，caption = "Kemonomichi Girlish Square 2" |
| C10 | YSCM 存在，含 1168 个字符串 | `ysc.ybn` 解压后 magic = `YSCM` |
| C11 | 样本引擎版本 555 / YPF 版本 500 | 实测 |
| C12 | 免封包优先级机制（yscfg 三个 FILEPRIORITY 字段） | YSCM 含三个键名 + 免封包文章逆向结论 |
| C13 | 变长字节码的存在（能观察到 pushint/pushvar 类指令） | 两处独立来源一致 |

### 🟡 Partially Confirmed / Likely

| # | 项 | 状态 | 待验证 |
|---|---|---|---|
| P1 | YSCM = Opcode/参数名表 | Likely（有原文佐证） | 需与字节码做交叉比对 |
| P2 | opcode 变长编码方案 `[主码][子码][标志]` + 操作数 | Likely | YSCM 比对后确认 |
| P3 | YSCM 参数名 → Runtime 能力映射 | Likely（按名推断） | 运行时行为验证 |
| P4 | 支持的图片/音频格式清单 | Likely | 实际解码验证 |
| P5 | YSTL / YSLB / YSVR / YSER 的用途 | 部分（存在性确认） | 结构未解析 |
| P6 | `part1` / `part4` 与 command 数的关系（808 = 202×4 = 402/2） | Hypothesis | 多文件统计 |
| P7 | 控制串前缀 `M`（0x4D）+ len + payload | Likely | 与 opt 标记结构吻合 |

### ❌ Unknown（禁止猜测，保持 unimplemented）

| # | 项 | 阻塞了什么 |
|---|---|---|
| U1 | **完整 opcode 语义表** | VM 实现（最大缺口） |
| U2 | 变量系统的类型与寻址（标量/数组/全局/局部） | 变量系统 |
| U3 | 表达式求值的完整规则与类型转换 | Expression Engine |
| U4 | 调用栈 / 标签 / 跳转的编码 | 控制流 |
| U5 | 挂起点（点击/选择/定时器）的精确触发条件 | Suspend/Resume |
| U6 | Scene / Layer 的真实数据模型 | 渲染 |
| U7 | 文本渲染细节（速度/Ruby/控制符/特效） | Text 系统 |
| U8 | 音频通道 / 淡入淡出 / 循环语义 | Audio 系统 |
| U9 | 存档格式（原版） | 兼容原版存档（自定格式可绕过） |
| U10 | `YSTD` 格式 | 未在本样本发现 |
| U11 | YPF entry tail 8 字节 | 校验（可暂时忽略） |
| U12 | 路径首字节 `$`(303) / `%`(5) / `9`(1) 的差异 | 索引解析的鲁棒性 |
| U13 | 版本差异（v247 / v255 / v29x / v4xx / v48x / v494 / v5xx） | 版本 Profile |
| U14 | 动画系统（Tween/Timeline 参数） | Animation |

---

## 4. 优先级排序

排序原则（按你的要求）：**对运行真实游戏的重要程度 × 对后续开发的依赖程度**。
**不是**按实现难度排序。

### Tier 0 —— 不做则一切免谈

| 优先级 | 任务 | 理由 |
|---|---|---|
| **P0.1** | YPF + YSTB + XOR + zlib 解码 | 所有数据的入口，且已 100% 确认，无风险 |
| **P0.2** | Trace / Dump 工具链 | 没有可观测性，后面每一步都在盲跑 |
| **P0.3** | **YSCM ↔ 字节码交叉比对，产出 Opcode 表** | 解除 U1 这个最大阻塞项；这是**唯一**能系统性拿到 opcode 语义的路径 |

### Tier 1 —— 决定"能不能跑起来"

| 优先级 | 任务 | 依赖 | 理由 |
|---|---|---|---|
| **P1.1** | Instruction 解码器（变长 → IR） | P0.3 | VM 的输入 |
| **P1.2** | Value 系统 + 变量区 | P0.3 | 几乎所有指令都碰变量 |
| **P1.3** | VM 骨架 + 值栈 + PC + Suspend/Resume | P1.1, P1.2 | 执行引擎 |
| **P1.4** | Runtime API trait 定义 | — | 可与 VM 并行，无依赖 |
| **P1.5** | 最小 Backend（软渲染 + 空音频） | P1.4 | 让 Golden Test 能跑 |

### Tier 2 —— 决定"像不像原版"

| 优先级 | 任务 | 依赖 |
|---|---|---|
| P2.1 | Scene / Layer 状态模型 | P1.3, P1.4 |
| P2.2 | Expression Engine | P1.2 |
| P2.3 | Resource Manager（懒加载 + LRU） | P0.1 |
| P2.4 | 图像解码（PNG/BMP/JPEG/WEBP…） | P2.3 |
| P2.5 | Text 系统（解析 / 布局 / 控制符） | P2.1 |
| P2.6 | Audio（BGM/SE/Voice + 通道） | P1.4 |
| P2.7 | Input（点击 / 选择 / Skip） | P1.4 |
| P2.8 | Animation（Tween/Timeline） | P2.1 |

### Tier 3 —— 完整化

| 优先级 | 任务 |
|---|---|
| P3.1 | Save / Load（VM + Scene + Runtime 状态快照） |
| P3.2 | 版本 Profile（v4xx / v5xx / v555…） |
| P3.3 | 真实游戏端到端跑通 |
| P3.4 | Golden Test 自动化对比 |

### 为什么 P0.3（Opcode 表）排在最前

它不是"看起来简单"，而是：

1. **阻塞面最大** —— U1 卡着 VM / 变量 / 表达式 / 控制流四项
2. **已有可执行的破解路径** —— YSCM 字符串表 + 字节码统计交叉比对，不需要先会跑游戏
3. **工具已现成** —— 解码器 + 统计脚本即可，不需要逆向调试
4. **产出可复用** —— 一旦拿到 opcode 表，Tier 1 全部解锁

---

## 5. 关键设计决策

### 5.1 VM 必须可挂起，不能阻塞

```rust
pub enum VmSuspend {
    Input,
    Choice(Vec<ChoiceOption>),
    Timer(Duration),
    Audio(AudioWait),
    Video(VideoWait),
    None,           // 本帧继续执行
}

impl YurisVm {
    /// 执行到下一个挂起点（或指令配额耗尽）后返回
    pub fn run(&mut self, budget: usize) -> Result<VmSuspend>;
    /// 外部 Runtime 完成异步操作后恢复
    pub fn resume(&mut self, resp: ResumeResponse) -> Result<()>;
}
```

**理由**：`TEXT → 等点击 → 继续` 是 GalGame 的基本节奏。阻塞式 VM 无法接入
事件驱动的渲染循环，也无法做 Golden Test（无法单步对比）。

### 5.2 自建 Value 类型，不用 `serde_json::Value`

```rust
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(YurisString),
    Array(ArrayRef),
    Ref(VariableRef),
    Null,
}
```

**理由**：YU-RIS 的类型转换规则（int↔float↔string 的隐式转换、字符串比较语义）
是原版行为的一部分。用通用 JSON 值会丢失这些语义，导致 Golden Test 永远对不齐。

**注意**：具体变体以逆向结果为准（U2 未解），先用最保守的集合，后续按证据增删。

### 5.3 表达式引擎独立实现，不转译成其他语言

```
Token → AST(Expr) → Evaluator
```

**理由**：转译成 Rust/Lua/JS 执行会引入宿主语言的求值语义（溢出、浮点、字符串比较），
无法复现 YU-RIS 原版的怪异行为。而"复现原版行为"是本项目的第一原则。

### 5.4 Kernel 不感知平台

VM → Runtime API（trait）→ Backend。

**理由**：
- 可移植（Windows / macOS / Android / WASM）
- 可测试（Golden Test 用 mock backend，无需窗口和声卡）
- 与你的第 10 条要求一致

### 5.5 资源懒加载 + LRU

**理由**：样本 CG 包 827 MB。全量加载不现实。且 YU-RIS 本身按路径索引 seek，
天然支持按需读取。

### 5.6 版本 Profile 集中管理

```rust
pub struct VersionProfile {
    pub engine_version: u32,        // 555
    pub ypf_version: u32,           // 500
    pub name_xor_key: u8,           // 0xC9
    pub ystb_xor_key: Option<[u8;4]>,
    pub opcode_table: OpcodeTableId,
    pub sjis_table: SjisRangeTable,
    ...
}
```

**理由**：版本差异散落各处会变成维护灾难。集中成 profile，按版本加载。

---

## 6. 与目标的距离

```
目标：Rust YurisKernel 直接运行原始 YU-RIS 游戏

当前位置：
  ✅ 能打开封包，能取出任意一个文件            (L0 完成)
  ✅ 能解开脚本容器的加密，能定位每一条指令槽位  (L1/L2 完成)
  ✅ 知道指令内容区是变长字节码                 (L2 完成)
  ❌ 不知道字节码的语义                        (L3 阻塞) ← 我们在这一步
  ❌ 没有 VM
  ❌ 没有 Runtime / Scene / 渲染 / 音频

阻塞项排名：
  1. U1  opcode 语义      ← 有明确破解路径（YSCM 比对），优先攻
  2. U2  变量系统          ← 依赖 U1
  3. U4  控制流编码        ← 依赖 U1
  4. U5  挂起点            ← 依赖 U1 + 运行时观察
```

**一句话**：数据我们已经能读出来了，卡在"读出来的字节是什么意思"。
而 `ysc.ybn`（YSCM）就是那本字典 —— 这是目前最该攻的点。

---

## 附：本文引用的实测数据来源

| 数据 | 来源 | 可复现方式 |
|---|---|---|
| YPF 结构与条目 | `AnimalTrailGirlishSquare 2/pac/bn.ypf` | 解析脚本，验证尺寸算术闭合 |
| YSTB 结构与密钥 | 同上，`$ysbin\yst00000.ybn` | 密钥 `2b904f93`，验证 402 条偏移自洽 |
| YSCF | 同上，`9ysbin\yscfg.ybn` | 106 字节，读 caption |
| YSCM | 同上，`%ysbin\ysc.ybn` | 解压后提取 1168 个字符串 |
| VM 字节码编码 | `[YU-RIS] Whirlpool社的一些观察/` | 文中反汇编片段 |
| YSCM 语义 | `[YU-RIS] SDK编译器调用/` | "那些字符串其实是按顺序的 Opcode 的名称和参数的名称" |
| 免封包机制 | `[YU-RIS] 免封包处理/` | yscfg 三字段逆向 |
