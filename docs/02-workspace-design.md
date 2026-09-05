# 02 · Rust Workspace 设计（第一版）

> 对应你的要求：**目录结构 / 模块职责 / 模块依赖关系 / 核心 struct / 核心 trait**
> 本版只做设计 + 骨架，**不实现业务逻辑**。
> 实现顺序见 `docs/03-phase1-plan.md`。

---

## 1. 目录结构

```
yuris-kernel/
├── Cargo.toml                  # workspace 根
├── PROGRESS.md                 # 进度日志（唯一真相来源）
├── README.md
│
├── crates/
│   ├── yuris-core/             # 错误、版本 Profile、日志、字节工具
│   ├── yuris-format/           # L0+L1：YPF / YSTB / YSCF / YSCM / XOR / zlib
│   ├── yuris-value/            # Value 类型 + 变量存储
│   ├── yuris-script/           # L2：变长字节码 → Instruction IR
│   ├── yuris-expr/             # 表达式：Token → AST → Evaluator
│   ├── yuris-vm/               # L3：PC / 栈 / 调用栈 / 挂起恢复
│   ├── yuris-scene/            # L5：Scene / Layer / 动画状态（纯数据）
│   ├── yuris-resource/         # 资源管理器：懒加载 + LRU
│   ├── yuris-runtime/          # L4：Backend trait 定义 + 事件循环 + 系统总线
│   ├── yuris-render/           # L6：GraphicsBackend 实现（软渲染 / wgpu）
│   ├── yuris-audio/            # L6：AudioBackend 实现
│   ├── yuris-input/            # L6：InputBackend 实现
│   ├── yuris-save/             # 存档快照序列化
│   ├── yuris-tools/            # CLI：dump / trace / verify / opcode-scan
│   └── yuris-cli/              # 播放器入口
│
├── docs/
│   ├── 01-runtime-architecture.md
│   ├── 02-workspace-design.md      ← 本文
│   ├── 03-phase1-plan.md
│   ├── formats/                    # 每格式一份规格
│   │   ├── ypf.md
│   │   ├── ystb.md
│   │   ├── yscf.md
│   │   ├── yscm.md
│   │   └── pending.md              # YSTL/YSLB/YSVR/YSER/YSTD
│   ├── opcode/                     # Opcode 规格（Phase 1 产出）
│   │   ├── opcode-table.md
│   │   └── semantics/
│   ├── vm/                         # VM 行为规格
│   ├── versions/                   # 版本 Profile
│   └── reverse/                    # 原始逆向记录（不乱改）
│
└── tests/
    ├── data/                       # 小型测试样本（不放整包！）
    ├── golden/                     # Golden Test 期望值
    └── integration/
```

### 相对你的原提案的调整

| 你的提案 | 本设计 | 理由 |
|---|---|---|
| — | **新增 `yuris-value`** | Value 系统 + 变量存储被 VM / Expr / Scene / Save 四处依赖，独立成 crate 避免循环依赖 |
| — | **新增 `yuris-expr`** | 你的第 9 条要求表达式引擎独立。它是纯函数式求值器，无 VM 依赖，可单独测试 |
| `yuris-core` | 收窄为「错误 + 版本 Profile + 字节工具」 | 不承载业务逻辑，保持稳定 |
| `yuris-render/audio/input` | 定位为 **Backend 实现**，trait 放 `yuris-runtime` | 保证依赖单向：`render → runtime`，而不是互相依赖 |
| `yuris-tools` 与 `yuris-cli` 并列 | 保留 | tools 面向开发（无 GUI），cli 面向运行 |

---

## 2. 模块依赖关系

```
                        yuris-core
                            ↑
        ┌───────────────────┼───────────────────┐
        │                   │                   │
   yuris-format       yuris-value          （无依赖）
        │                   ↑
        │          ┌────────┴────────┐
        │          │                 │
        │     yuris-script      yuris-expr
        │          │                 │
        │          └────────┬────────┘
        │                   ↓
        │              yuris-vm
        │                   │
        ├───────────────────┼──────────────┐
        ↓                   ↓              ↓
  yuris-resource       yuris-scene    yuris-runtime  ← 定义 Backend trait
        │                   ↑              ↑
        └───────────────────┴──────────────┤
                            ↑              │
                    ┌───────┴──────┬───────┴───────┐
                    │              │               │
              yuris-render    yuris-audio     yuris-input
                    └──────────────┴───────────────┘
                                   ↑
                             yuris-save
                                   ↑
                    ┌──────────────┴──────────────┐
                    │                             │
              yuris-tools                    yuris-cli
```

**规则**：
- 依赖严格向下，无环
- `yuris-vm` **不依赖** `yuris-render` / `yuris-audio` / `yuris-input`
- `yuris-runtime` 定义 trait，`yuris-render` 等实现 trait
- `yuris-scene` 是纯数据结构，不依赖 runtime

**用 `cargo` 强制检查**（加到 CI）：
```bash
cargo deny check bans   # 或 cargo tree 人工审查
```

---

## 3. 核心 struct / trait

### 3.1 `yuris-core`

```rust
// ===== 错误 =====
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("format: {0}")]
    Format(String),
    #[error("unsupported version: engine={engine} ypf={ypf}")]
    UnsupportedVersion { engine: u32, ypf: u32 },
    #[error("decompress: {0}")]
    Decompress(String),
    #[error("xor key not found for {0}")]
    KeyNotFound(String),
    #[error("opcode unresolved: {0:#x} at pc={1:#x}")]
    UnresolvedOpcode(u32, usize),
    #[error("unimplemented: {0}")]
    Unimplemented(&'static str),   // ← U 级 Unknown 一律走这里，不猜
}
pub type Result<T> = std::result::Result<T, Error>;

// ===== 版本 Profile =====
#[derive(Debug, Clone)]
pub struct VersionProfile {
    pub engine_version: u32,          // 555
    pub ypf_version: u32,             // 500
    pub ypf_name_xor_key: u8,         // 0xC9   (Confirmed 本样本)
    pub ystb_xor_key: Option<[u8; 4]>,// 逐游戏；None = 需猜测
    pub char_encoding: CharEncoding,  // Sjis | Gbk
    pub opcode_table: OpcodeTableId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharEncoding { Sjis, Gbk, Utf8 }
```

### 3.2 `yuris-format`

```rust
// ===== YPF =====
pub struct YpfArchive {
    header: YpfHeader,
    entries: Vec<YpfEntry>,
    name_index: HashMap<String, usize>,   // 内部路径 → entry 下标
    reader: Box<dyn RandomAccess>,        // 便于内存/文件两种后端
}

pub struct YpfHeader {
    pub magic: [u8; 4],       // b"YPF\0"
    pub version: u32,         // 500
    pub file_count: u32,      // 309
    pub first_data_off: u32,  // 0x3655  ← 同时是索引区结束位置
}

pub struct YpfEntry {
    pub name: String,          // 已解码，如 "$ysbin\yst00034.ybn"
    pub name_raw_prefix: u8,   // 0xED / 0xEC / 0xF0 —— 保留原始字节，不丢信息
    pub flags: u8,             // 1 = zlib, 0 = stored
    pub uncompressed_len: u32,
    pub compressed_len: u32,
    pub offset: u32,
    pub reserved: u32,         // 实测恒 0，保留字段
    pub tail: [u8; 8],         // 用途 Unknown，原样保留
}

impl YpfArchive {
    /// 按内部路径读取，解压 + 返回原始字节
    pub fn read(&mut self, path: &str) -> Result<Vec<u8>>;
    pub fn entries(&self) -> &[YpfEntry];
}

// ===== YSTB =====
pub struct YstbFile {
    pub header: YstbHeader,
    pub part1: Vec<u8>,      // 808 字节，语义 Unknown —— 原样保留
    pub commands: Vec<u8>,   // command_len 字节，定长 12 字节记录
    pub content: Vec<u8>,    // 原命名 strs，实测为 VM 字节码
    pub part4: Vec<u8>,      // 808 字节，语义 Unknown —— 原样保留
}

pub struct YstbHeader {
    pub magic: [u8; 4],    // b"YSTB"
    pub version: u32,      // 555
    pub unknown1: u32,     // 202 —— 保留，不猜
    pub part1_len: u32,
    pub command_len: u32,
    pub content_len: u32,
    pub part4_len: u32,
    pub unknown2: u32,     // 实测 0
}

/// 12 字节定长槽位描述符
#[derive(Debug, Clone, Copy)]
pub struct CommandSlot {
    pub tag: u32,       // 0x00000000 = text, 0x00030000 = opt, …
    pub len: u32,
    pub offset: u32,    // 相对 content 区起始
}

impl YstbFile {
    pub fn slots(&self) -> impl Iterator<Item = CommandSlot> + '_;
    pub fn slot_content(&self, slot: CommandSlot) -> &[u8];
}

// ===== XOR =====
/// 4 字节循环 XOR，跳过前 skip 字节
pub fn xor_cyclic(data: &mut [u8], key: &[u8; 4], skip: usize);

/// 单字节 XOR（YPF 文件名用，0x00 不参与）
pub fn xor_name(data: &[u8], key: u8) -> Vec<u8>;

// ===== YSCM =====
pub struct YscmTable {
    strings: Vec<String>,   // 按索引编号
    index: HashMap<String, usize>,
}
impl YscmTable {
    pub fn by_index(&self, i: usize) -> Option<&str>;
    pub fn by_name(&self, s: &str) -> Option<usize>;
}
```

### 3.3 `yuris-value`

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Array(ArrayRef),
    Ref(VariableRef),
    Null,
}

/// 变量引用。具体编码待逆向（U2），先用 (space, index) 抽象
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariableRef {
    pub space: VarSpace,   // Global | Local | System | Thread
    pub index: u32,
}

pub struct VariableStore {
    // 分空间存储；内部用 Vec 而非 HashMap，保序且 Golden Test 可比对
    spaces: EnumMap<VarSpace, Vec<Value>>,
}

impl Value {
    /// YU-RIS 的类型转换规则 —— **以逆向结果为准**，未确认处返回 Error 而非猜测
    pub fn to_int(&self) -> Result<i64>;
    pub fn to_float(&self) -> Result<f64>;
    pub fn to_bool(&self) -> Result<bool>;
    pub fn to_string(&self, enc: CharEncoding) -> Result<String>;
}
```

### 3.4 `yuris-script`

```rust
/// 解码后的统一内部表示
#[derive(Debug, Clone)]
pub struct Instruction {
    pub opcode: Opcode,        // 已解析（或 Unresolved）
    pub operands: Vec<Value>,
    pub offset: u32,           // 在字节码流中的字节偏移
    pub raw: SmallVec<[u8; 8]>,// 原始字节，永不丢弃（便于回溯与调试）
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Resolved(ResolvedOp),
    /// 未识别 —— **保留原始字节，不猜**
    Unresolved(u32),
}

/// 解码器：把变长字节码流切成 Instruction 序列
pub struct Decoder<'a> {
    data: &'a [u8],
    pc: usize,
    table: &'a OpcodeTable,
}

impl<'a> Decoder<'a> {
    pub fn decode_all(&mut self) -> Result<Vec<Instruction>>;
    fn decode_one(&mut self) -> Result<Instruction>;
}

/// Opcode 规格表：由 YSCM 交叉比对产出，数据驱动
pub struct OpcodeTable {
    entries: HashMap<u32, OpcodeSpec>,
}

pub struct OpcodeSpec {
    pub code: u32,
    pub name: Option<String>,        // 来自 YSCM；None = 未比对上
    pub operand_kinds: Vec<Operand>, // Int8 / Int16 / Int32 / VarRef / StrRef …
    pub stack_in: u32,
    pub stack_out: u32,
    pub control_flow: ControlFlow,   // Continue | Jump | Call | Return
    pub side_effect: SideEffect,     // bitflags: Graphics/Audio/Variable/Input
    pub blocking: bool,
    pub confidence: Confidence,      // Confirmed | Likely | Hypothesis | Unknown
}
```

### 3.5 `yuris-expr`

```rust
pub enum Expr {
    Literal(Value),
    Variable(VariableRef),
    Unary { op: UnOp, operand: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
    /// 未知结构 —— 保留原始 token 序列
    Raw(Vec<Token>),
}

pub fn tokenize(src: &[u8], enc: CharEncoding) -> Result<Vec<Token>>;
pub fn parse(tokens: &[Token]) -> Result<Expr>;
pub fn evaluate(expr: &Expr, ctx: &mut EvalContext) -> Result<Value>;

pub struct EvalContext<'a> {
    pub vars: &'a mut VariableStore,
    pub profile: &'a VersionProfile,
}
```

**原则**：不转译成 Rust / Lua / JS / Kotlin 再执行。
所有类型提升、比较、溢出行为必须自己实现，才能复现原版怪异行为。

### 3.6 `yuris-vm`

```rust
pub struct YurisVm {
    pub pc: usize,
    pub stack: ValueStack,
    pub call_stack: CallStack,
    pub globals: VariableStore,
    pub locals: VariableStore,
    pub state: VmState,
    pub scene: SceneHandle,        // 不直接持有 Scene，通过句柄写
    pub trace: Option<TraceSink>,  // None = 关闭；生产环境可关
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    Running, WaitingInput, WaitingChoice, WaitingTimer,
    WaitingAudio, WaitingVideo, Paused, Finished, Error,
}

/// 挂起原因 —— 交给上层 Runtime 处理，处理完 resume()
#[derive(Debug, Clone)]
pub enum VmSuspend {
    None,                          // 本帧配额跑完，下一帧继续
    Input,
    Choice(Vec<ChoiceOption>),
    Timer(Duration),
    Audio(AudioWait),
    Video(VideoWait),
    Error(Error),
}

#[derive(Debug, Clone)]
pub struct ResumeResponse {
    pub kind: ResumeKind,   // Click | Selected(usize) | Timeout | MediaEnded
}

impl YurisVm {
    /// 执行到下一个挂起点或配额耗尽。**不阻塞。**
    pub fn run(&mut self, sys: &mut dyn RuntimeApi, budget: usize) -> Result<VmSuspend>;
    pub fn resume(&mut self, resp: ResumeResponse) -> Result<()>;
    /// 单步（Golden Test / 调试用）
    pub fn step(&mut self, sys: &mut dyn RuntimeApi) -> Result<VmSuspend>;
}
```

### 3.7 `yuris-runtime` —— Backend trait（唯一平台边界）

```rust
/// VM 可见的系统能力总线。**所有**对外部世界的访问都要经过它。
pub trait RuntimeApi {
    fn graphics(&mut self) -> &mut dyn GraphicsBackend;
    fn audio(&mut self) -> &mut dyn AudioBackend;
    fn input(&mut self) -> &mut dyn InputBackend;
    fn storage(&mut self) -> &mut dyn StorageBackend;
    fn clock(&self) -> Instant;
    fn emit(&mut self, ev: RuntimeEvent);   // Trace / Golden Test 的数据源
}

pub trait GraphicsBackend {
    fn load_image(&mut self, id: ResourceId, data: &[u8]) -> Result<()>;
    fn begin_frame(&mut self) -> Result<()>;
    fn draw_layer(&mut self, layer: &LayerView<'_>) -> Result<()>;
    fn draw_text(&mut self, text: &TextLayout) -> Result<()>;
    fn end_frame(&mut self) -> Result<()>;
}

pub trait AudioBackend {
    fn play_bgm(&mut self, id: ResourceId, opt: &PlayOptions) -> Result<ChannelId>;
    fn play_se(&mut self, id: ResourceId, opt: &PlayOptions) -> Result<ChannelId>;
    fn play_voice(&mut self, id: ResourceId, opt: &PlayOptions) -> Result<ChannelId>;
    fn stop(&mut self, ch: ChannelId, fade: Duration) -> Result<()>;
    fn set_volume(&mut self, ch: ChannelId, v: f32) -> Result<()>;
    /// 阻塞型等待（等播放结束）——由 VM 挂起后轮询
    fn is_finished(&self, ch: ChannelId) -> bool;
}

pub trait InputBackend {
    fn poll(&mut self) -> InputEvent;   // Click / Key / Wheel / None
    fn wait_click(&mut self) -> InputEvent;
}

pub trait StorageBackend {
    /// 支持免封包覆盖：先查目录，再查封包
    fn read(&self, path: &str) -> Result<Vec<u8>>;
    fn exists(&self, path: &str) -> bool;
}
```

**Kernel 绝不出现**：Android、Windows、OpenGL、Vulkan、Media3、Canvas、SDL。
这些只存在于 `yuris-render` / `yuris-audio` / `yuris-input` 内部。

### 3.8 `yuris-scene`

```rust
pub struct Scene {
    pub layers: Vec<Layer>,
    pub text: Option<TextLayout>,
    pub anims: AnimationSet,
}

pub struct Layer {
    pub id: u32,
    pub z: i32,
    pub visible: bool,
    pub x: f32, pub y: f32,
    pub scale_x: f32, pub scale_y: f32,
    pub alpha: f32,
    pub rotation: f32,
    /// 变换中心（YSCM 有 RCX/RCY，说明原版有这个概念）
    pub origin_x: f32, pub origin_y: f32,
    pub resource: Option<ResourceId>,
    /// 未识别字段一律保留，不丢
    pub extra: Vec<(String, Value)>,
}
```

**数据流**：`VM → Scene（纯状态）→ Renderer`。VM 不直接调绘图 API。

### 3.9 `yuris-resource`

```rust
pub struct ResourceManager {
    archives: Vec<YpfArchive>,
    override_dir: Option<PathBuf>,   // 免封包覆盖目录
    cache: LruCache<ResourceId, Arc<Resource>>,
}

pub enum Resource {
    Image(ImageData),
    Audio(AudioData),
    Video(VideoData),
    Script(YstbFile),
    Binary(Vec<u8>),
}

impl ResourceManager {
    /// 懒加载：只有真正用到才解压
    pub fn get(&mut self, path: &str) -> Result<Arc<Resource>>;
    pub fn prefetch(&mut self, paths: &[&str]) -> Result<()>;
    pub fn set_memory_budget(&mut self, bytes: usize);
}
```

### 3.10 `yuris-save`

```rust
/// 存档 = VM 状态 + Scene 状态 + 必要 Runtime 状态
#[derive(Serialize, Deserialize)]
pub struct SaveData {
    pub version: u32,
    pub profile_id: String,
    pub vm: VmSnapshot,      // pc / call_stack / stack / globals / locals
    pub scene: Scene,
    pub audio: AudioSnapshot,
    pub system: SystemSnapshot,
    pub checksum: u64,
}
```

语义必须能完整恢复；格式自定（建议 bincode + 版本号 + 校验和）。

---

## 4. Trace 系统

`yuris-core` 提供统一 sink，VM / Runtime 都往里写。

```rust
pub struct TraceSink { /* 输出 txt / jsonl */ }

#[derive(Serialize)]
pub struct TraceRecord {
    pub seq: u64,
    pub pc: usize,
    pub opcode: String,        // 名称或 Unresolved(0x…)
    pub operands: Vec<String>,
    pub stack_depth: usize,
    pub vm_state: String,
    pub scene_delta: Option<String>,
    pub events: Vec<String>,
}
```

CLI：

```bash
yuris trace game/ --out trace.txt --format txt
yuris trace game/ --out trace.jsonl --format jsonl
```

输出示例：

```
seq=000042 pc=0x000012A0 opcode=TEXT operands=["こんにちは"]
        stack_depth=0 vm_state=WaitingInput
```

---

## 5. Golden Test 设计

```
原版 YU-RIS 运行  ──►  Expected（人工记录 / hook 抓取）
                                    │
                                    ├── 比较
                                    │
Rust YurisKernel  ─►  Actual（trace.jsonl）
```

比较维度：

| 维度 | 来源 |
|---|---|
| PC 序列 | Trace |
| 变量快照 | 每个挂起点 dump 全量变量 |
| Scene 状态 | Layer 数量 / 可见性 / 变换 / 资源 ID |
| Audio 事件 | BGM/SE/Voice 的播放/停止/音量序列 |
| VM State | 挂起类型与顺序 |
| 文本 | 显示文本序列 |

```rust
// tests/golden/
#[test]
fn scene_001_matches_original() {
    let expected = load_golden("scene_001");
    let actual = run_headless("scene_001");   // mock backend，无窗口无声卡
    assert_golden_eq(&expected, &actual);
}
```

**前提**：Backend 必须是 trait，这样才能用 mock backend 无头运行 —— 这是 3.7 设计的直接收益。

---

## 6. 编码规范

| 项 | 规定 |
|---|---|
| Rust | Stable（当前 1.96.0） |
| 错误 | 一律 `Result<T, yuris_core::Error>`，不用 `unwrap` / `panic` 处理可恢复错误 |
| 日志 | `tracing`，不用 `println!` |
| Unknown 处理 | 一律 `Error::Unimplemented("原因")`，**禁止猜** |
| `unsafe` | 需要时必须在 PR 说明；默认 0 |
| 全局状态 | 禁止。配置与状态显式传递 |
| Clone | 热路径（VM 循环内）避免；资源用 `Arc` |
| `Arc<Mutex<_>>` | 只在跨线程 Backend 边界使用，VM 内部不用 |
| 平台相关 | 只允许出现在 `yuris-render` / `yuris-audio` / `yuris-input` |
| 测试 | 每个格式解析器必须有「样本字节 → 期望结构」的单测 |

---

## 7. 已建骨架

```
crates/yuris-{core,format,value,script,expr,vm,scene,resource,runtime,
              render,audio,input,save,tools,cli}/src/lib.rs
Cargo.toml (workspace)
```

`cargo build` 通过。下一步按 `docs/03-phase1-plan.md` 填充实现。
