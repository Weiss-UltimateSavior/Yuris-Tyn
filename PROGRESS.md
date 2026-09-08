# YurisKernel 开发进度日志

> 目标：**用 Rust 实现一个 YU-RIS 兼容运行内核，直接读取原始游戏数据并运行原游戏。**
> 不是提取器，不是反编译器，不是转换器。
>
> 本文件是**唯一进度真相来源**。每获得一个成果、每完成一个阶段，立即更新。
> 更新格式：日期 + 阶段 + 成果 + 证据 + 结论等级（Confirmed / Likely / Hypothesis / Unknown）。

***

## 证据等级定义

| 等级             | 含义                        | 可以拿来做什么                           |
| -------------- | ------------------------- | --------------------------------- |
| **Confirmed**  | 有本仓库样本实测数据 + 可复现的验证步骤     | 直接写实现 + 写单元测试                     |
| **Likely**     | 有间接证据或跨版本一致性支持，但未在本样本直接验证 | 可以实现，但必须加 `// UNVERIFIED` 与对照测试   |
| **Hypothesis** | 基于结构/模式推断，无直接证据           | **禁止**写进核心路径，只写进实验分支或文档           |
| **Unknown**    | 没有任何证据                    | 保持 `unimplemented!()` 或显式错误返回，不要猜 |

***

## 2026-09-02 — Phase 0：架构分析 + 样本实测

### 阶段状态

| 阶段                    | 状态   | 说明                              |
| --------------------- | ---- | ------------------------------- |
| 0.1 资料清点              | ✅ 完成 | 见下                              |
| 0.2 YPF 实测            | ✅ 完成 | 结构 **Confirmed**                |
| 0.3 YSTB 实测           | ✅ 完成 | 结构 + 密钥 **Confirmed**           |
| 0.4 YSCF 实测           | ✅ 完成 | **Confirmed**                   |
| 0.5 YSCM 发现           | ✅ 完成 | 存在性 **Confirmed**，语义 **Likely** |
| 0.6 Rust Workspace 设计 | ✅ 完成 | 见 `docs/02-workspace-design.md` |
| 0.7 Phase 1 任务拆分      | ✅ 完成 | 见 `docs/03-phase1-plan.md`      |
| 0.8 Cargo 骨架          | ✅ 完成 | 15 crates，编译通过                  |

### 成果 1：YPF 封包格式 —— **Confirmed**

样本：`AnimalTrailGirlishSquare 2/pac/bn.ypf`（1,465,312 字节，309 条目）

```
Header (0x20 字节)
  0x00  magic          b"YPF\0"
  0x04  version        u32 LE = 500
  0x08  file_count     u32 LE = 309
  0x0C  first_data_off u32 LE = 0x3655   ← 首个文件数据的绝对偏移
  0x10..0x20  全 0

Index 位于 [0x20, first_data_off)
  每条 Entry：
    name       C 字符串，每个非 0 字节 XOR 0xC9，终止符 0x00 不加密
    flag       u8     1 = zlib 压缩，0 = 原样存储
    uncomp_len u32 LE
    comp_len   u32 LE
    offset     u32 LE  （绝对文件偏移）
    zero       u32 LE  恒为 0
    tail       8 字节  用途未知（疑似校验和/哈希）

  Entry 大小 = len(name) + 1 + 1 + 4*4 + 8 = len(name) + 26
```

**验证方式（可复现）**：`python3 scripts/probe_format.py <bn.ypf>`

1. 解析出 **309 条**，全部字段名解密为合法路径（`$ysbin\yst00034.ybn` 等）
2. **相邻名字起始间距**实测：`45 × 302`、`40 × 5`、`42 × 1`（308 个间距），
   与模型 `len(name) + 26` **逐条吻合，零不符** → 模型正确
3. `offset` 指向的 zlib 流解压后长度 == `uncomp_len`（304/309 通过）
4. 跨度闭合：`0x3655 − 0x24 = 13873 = 13832（308 条实测）+ 41（末条）` ✓

**已知异常（2026-09-02 修正）**：

> ⚠️ 早期笔记曾写「索引精确闭合于 `first_data_off`」——**该结论错误，已更正**。

| 现象                              | 实测                                              |
| ------------------------------- | ----------------------------------------------- |
| 索引区头部 4 字节                      | `0x20..0x23 = 52 ae 33 00`，用途 **Unknown**       |
| 首个名字起始                          | `0x24`（不是 `0x20`）                               |
| 末条 `$ysbin\yst00270.ybn`（19 字符） | 按模型应 45 字节，实际仅 `41` 字节，**tail 只有 4 字节**         |
| 末条"tail 8 字节"的后 4 字节            | 实为 `78 da c5 5a` —— `78 da` 正是首个数据块的 **zlib 头** |
| 净结果                             | 头部多出的 4 字节 与 末条缺失的 4 字节 **恰好相抵**                |

**对实现无阻塞**：改用鲁棒解析（从 `0x24` 起，循环 `file_count` 次，
越界即停，残差用容差断言 `|p − first_data_off| ≤ 8`）。已写进 `docs/formats/ypf.md` §2.4。

**遗留未知**：

- 索引区头部 4 字节 `52 ae 33 00` 语义 —— **Unknown**

- 末条 tail 为何只有 4 字节 —— **Unknown**（现象已确认，原因未明）

- `tail` 8 字节语义 —— **Unknown**

- 路径首字节：303×`0xED`（→`$`）、5×`0xEC`（→`%`）、1×`0xF0`（→`9`）—— **Unknown**

- 1 个条目非 zlib（`flag=0`）—— **Likely**（stored）

- XOR key `0xC9` 是否随版本/游戏变化 —— **Unknown**（仅本样本验证）

### 成果 2：YSTB 脚本容器 —— **Confirmed**

样本：`bn.ypf` 内的 `$ysbin\ysc.ybn`（解压后 11,102 字节）与 `yst00000.ybn`（11,209 字节）

```
Header (0x20 字节，不加密)
  0x00  magic       b"YSTB"
  0x04  version     u32 LE = 555
  0x08  unknown1    u32 LE = 202
  0x0C  part1_len   u32 LE = 808
  0x10  command_len u32 LE = 4824
  0x14  str_len     u32 LE = 4737
  0x18  part4_len   u32 LE = 808
  0x1C  unknown2    u32 LE = 0

Body（从 0x20 起全部参与 XOR）
  part1[part1_len]
  commands[command_len]     ← 定长记录，每条 12 字节
  strs[str_len]
  part4[part4_len]

加密：4 字节循环 XOR，跳过前 0x20 字节
  plain[i] = cipher[i] XOR key[i % 4]        (i >= 0x20)
```

**本样本密钥 =** **`2b904f93`**（Confirmed）

**Command 记录（定长 12 字节）**：

```rust
struct Command {
    opcode:         u32,   // 语义标签
    content_len:    u32,   // 内容字节数
    content_offset: u32,   // 相对 strs 区起始的偏移
}
```

**验证方式（决定性）**：

1. `command_len % 12 == 0`（4824 / 12 = 402 条）
2. 解密后 opcode 出现 `0x00000000` 与 `0x00030000`，与已有工具 `YSTB_FILE.py` 中记录的
   `b"\x00\x00\x00\x00"`（text）与 `b"\x00\x00\x03\x00"`（may\_be\_opt）**完全一致**
3. `content_offset` 与 `content_len` 严格自洽：
   `off[0]=0,len=22 → off[1]=22,len=5 → off[2]=27,len=22 → off[3]=49...`
   402 条指令**零越界**，完全连续 —— 这是密钥正确性的强证明
4. `0x20 + 808 + 4824 + 4737 + 808 = 11209 == 文件大小`

**关键发现（影响 VM 设计）**：

`strs` 区存放的**不是明文日文**，而是**变长 VM 字节码**：

```
56 03 00 24 ca 04 | 57 02 00 90 01 | 42 01 00 01 | 2b 00 00 | 29 01 00 00 | 4d 02 00 22 22
```

其中 `57 02 00`、`42 01 00`、`56 03 00`、`4d 02 00` 的编码形式与
`[YU-RIS] Whirlpool社的一些观察` 一文中记录的 VM 字节码**同一套编码**：

```
48030040 FB0A  pushscalarvar FB0A
420100   5F    pushint8 0x5F
570200   8100  pushint16 0x8100
3D0000         equal
5A0000         ge
530000         le
260000         logand
7C0000         logor
```

另外 `4d 02 00 22 22` 中的 `4d` 是控制串前缀 `M`，与 `YSTB_FILE.py` 里
opt 起始标记 `4D 0C 00 "ES.SEL.SET"` 的 `M + len + payload` 结构吻合。

**结论（架构级）**：

> 12 字节 Command 表 = **内容槽位描述符表**（tag + len + offset）
> `strs` 区 = **实际可执行内容**，里面是变长 VM 字节码流
>
> 即 YU-RIS 属于「指令与数据分离」结构（见 Methodology/基于虚拟机字节码的文本修改思路），
> 这对我们极度有利：改文本不需要解析完整 VM。

**遗留未知**：

- `part1` / `part4` 各 808 字节的语义（808 = 202 × 4，恰好是 402 的一半）—— **Unknown**

- `unknown1 = 202` 的含义 —— **Unknown**（疑与 part1 条目数有关）

- opcode `0x00010000`（本样本出现 169 次）语义 —— **Unknown**

- 字节码的完整 opcode 表 —— **Unknown**（但已有破解路径，见成果 4）

### 成果 3：YSCF 工程配置 —— **Confirmed**

样本：`$ysbin\yscfg.ybn`（106 字节）

```
magic   b"YSCF"
version u32 LE = 555
screen  u32 LE 1920 × 1080
tail    u16 caption_len = 0x1C = 28
        "Kemonomichi Girlish Square 2"
```

与既有资料中 `yscfg.ybn` 结构（含 `filePriorityRelease` 等字段）一致。
**下一步**：按 Notes.txt 的字段布局把 106 字节逐字段对齐，读出真实 `filePriority*`。

### 成果 4：YSCM —— Opcode 名称表（**存在性 Confirmed，语义 Likely**）

样本：`$ysbin\ysc.ybn` 解压后 magic = **`YSCM`**，11,102 字节，含 **1168 个可打印字符串**。

内容包含两类：

**(a) 参数名 / 命令名（前段）**

```
ALIAS CG :ID IDNO SD MD SX SY SLX SLY SCX SCY RZ RLX RLY RCX RCY
SIP RIP TSX TSY MIPMAP TEX DXBUF DXDRAW FX FY FBX FBY FRX FRY FRZ
FCX FCY FEX FEY FEZ FSD FMD FQU FID REMALLOC FILE RID MID MFILE
TID TIDNO TA MODE CASH LINT LINT2 CGACT GID ID2 ID3 ID4 IDNO ...
```

**(b) 配置键名（后段）**

```
CAPTION THREAD SCRIPTFILEEXT DEBUGMODE SOUND COMPILE
WINDOWRESIZE WINDOWFRAME
FILEPRIORITYDEVELOP  FILEPRIORITYDEBUG  FILEPRIORITYRELEASE   ← 印证免封包文章
DEFSTR DEFSTR2 ... DEFSTR10
COMPILEMODE RELEASEMODE PROJECTFOLDER
```

还发现疑似编码/转义表：

```
ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789pqrstuvwxyz{|}~
`abcdefghikmjon
%'&(-./
!"$#
```

**为什么重要**（来自 `[YU-RIS] SDK编译器调用` 原文）：

> "还有个 YSCom.ycd 的文件，这个 Com 应该是 command 的缩写，里面的结构和编译后 ysbin 里的
> ysc.ybn 文件很像，那些字符串其实是按顺序的 **Opcode 的名称和参数的名称**。"

**破解路径（已确认可行、尚未执行）**：

```
YSCM 字符串表（按索引编号）
        ↓  与 YSTB 字节码中出现的 opcode 编号做交叉比对
Opcode 编号 → 名称 → 参数个数/类型
        ↓
Opcode Specification
```

这是本项目目前**最有价值的一条线索**，应作为 Phase 1 的高优先级任务。

### 成果 5：Rust Workspace 设计

见 `docs/02-workspace-design.md`。15 个 crate，依赖单向无环，已建骨架并编译通过。

### 成果 6：Phase 1 任务拆分

见 `docs/03-phase1-plan.md`。按「对运行真实游戏的重要程度 × 后续依赖程度」排序，
不是按难度排序。

### 成果 7：可复现探测脚本

`scripts/probe_format.py` —— 把本阶段全部格式结论固化为可执行验证。

```bash
python3 scripts/probe_format.py "path/to/bn.ypf"
```

样本实际输出：

```
[YPF] version=500 count=309 first_data_off=0x3655
[YPF] index prefix [0x20,0x24) = 52ae3300  (purpose: Unknown)
[YPF] parsed 309 entries -> end=0x3659; first_data_off=0x3655; residual=-4 bytes
[YPF] entry sizes: {45: 303, 42: 1, 40: 5}
[YPF] payload magics: {'YSTB': 302, 'YSCF': 1, '<non-zlib>': 1, 'YSER': 1,
                       'YSTL': 1, 'YSCM': 1, 'YSVR': 1, 'YSLB': 1}
[YPF] name prefix bytes: {'0xed': 303, '0xf0': 1, '0xec': 5}

[YSTB] $ysbin\yst00000.ybn  (11209 bytes)
[YSTB] key = 2b904f93
[YSTB] version=555 unknown1=202 unknown2=0
[YSTB] part1=808 command=4824 content=4737 part4=808
[YSTB] slots=402 contiguous=402 out_of_bounds=0
[YSTB] slots are fully contiguous  -> key verified  ✓
[YSTB] tag histogram: {'0x0': 201, '0x30000': 32, '0x10000': 169}
[YSTB] content[0:32] = 56 03 00 24 ca 04 57 02 00 90 01 42 01 00 01 2b 00 00 ...
```

### 成果 8：Cargo Workspace 骨架

15 个 crate，`cargo build` 通过（Rust 1.96.0 stable）。
依赖单向无环；`yuris-vm` 不依赖任何 backend crate（由 `yuris-runtime` 定义 trait）。

***

## 2026-09-02 — Phase 1：P0.1 \~ P0.4 完成

### 阶段状态

| 任务                        | 状态   | 产出                                                    |
| ------------------------- | ---- | ----------------------------------------------------- |
| P0.1 `yuris-core`         | ✅ 完成 | `error.rs` / `bytes.rs` / `version.rs`，6 个单测          |
| P0.2 `yuris-format::ypf`  | ✅ 完成 | 309/309 条目，索引**精确闭合**                                 |
| P0.3 `yuris-format::ystb` | ✅ 完成 | 402/402 槽位，`contiguity_score() == 1.0`                |
| P0.3 `yuris-format::yscf` | ✅ 完成 | 字段级解析，含 `filePriority` 三值                             |
| P0.4 `yuris-tools`        | ✅ 完成 | `ypf list/extract`、`ystb guess-key/info/slots`、`yscf` |

**测试结果：11 passed / 0 failed**（含 5 个对真实样本 `bn.ypf` 的集成断言）。

### 成果 9：Rust 实现 + 自动密钥猜测（**Confirmed**）

`yuris-tools` 的 `ystb guess-key` 在**不提供任何先验**的情况下，
从密文中自动还原出密钥 `2b904f93`，连续性评分 **1.0000**：

```bash
$ yuris ystb guess-key bn.ypf --name '$ysbin\yst00000.ybn'
key = 2b904f93  score = 1.0000  (contiguity, 1.0 = perfect)
```

原理（见 `docs/formats/ystb.md` §5）：槽位表不变量
`off[n] + len[n] == off[n+1]` 是密钥正确性的判定器。
两个候选（首槽位 `tag==0` / 首槽位 `offset==0`）取连续性更高者。

> 这一步意义重大：**换一个 YU-RIS 游戏，密钥不再需要人工逆向**。

### 成果 10：YSCF 字段级解析（**Confirmed**）

```
version=555
screen = 1920x1080
file_priority: dev=1 debug=1 release=0  (unpacked read = disabled)
image_type_slots = [1, 2, 3, 4, 5, 6, 0, 0]
sound_type_slots = [1, 2, 0, 0]
caption = "Kemonomichi Girlish Square 2"
```

**重要确认**：`(dev, debug, release) = (1, 1, 0)` ——
**`filePriorityRelease`** **默认为 0**，与 `[YU-RIS] 免封包处理` 一文
「我们的目的正是在把 filePriorityRelease 变为 1」**逐字吻合**。
这把该文从「逆向结论」升级为「有实测数据支撑的规格」。

### 成果 11：虚拟根标记澄清（**Confirmed**）

条目名的首字符**是名字的一部分**，且取三种值：

| 前缀  | 数量  | 条目                                                        |
| --- | --- | --------------------------------------------------------- |
| `$` | 303 | 302 × `yst0xxxx.ybn` + 1 × `yst_list.ybn`                 |
| `%` | 5   | `ysc.ybn` / `yse.ybn` / `ysv.ybn` / `ysl.ybn` / `yst.ybn` |
| `9` | 1   | `yscfg.ybn`                                               |

**正确条目名**（此前文档误写为 `$ysbin\*`，已更正）：

| 内容            | 正确名字                  | magic  |
| ------------- | --------------------- | ------ |
| **Opcode 名表** | `%ysbin\ysc.ybn`      | `YSCM` |
| 工程配置          | `9ysbin\yscfg.ybn`    | `YSCF` |
| 文件名映射表        | `$ysbin\yst_list.ybn` | `YSTL` |
| 错误/消息表        | `%ysbin\yse.ybn`      | `YSER` |
| <br />        | `%ysbin\ysv.ybn`      | `YSVR` |
| <br />        | `%ysbin\ysl.ybn`      | `YSLB` |
| stored 16 字节  | `%ysbin\yst.ybn`      | —      |

三种前缀的语义（`$` / `%` / `9`）仍为 **Unknown**，
但与条目尺寸算术完全自洽（19/14/16 字符 × `len+26`）。

### 索引闭合问题（勘误补充）

引入「末条 tail 按剩余空间钳制」规则后，解析位置**精确等于** `first_data_off`：
`0x24 + Σ(309 条) = 0x3655` ✓
此前记录的「残差 −4」是**朴素固定读 8 字节**造成的，不是格式本身的问题。

***

## 2026-09-02 — Phase 1：P1.1 \~ P1.3 完成（YSCM + 字节码全语料 + Opcode 表）

**测试结果：17 passed / 0 failed**（6 core 单测 + 2 YSCM 单测 + 9 真实样本集成断言）。

### 成果 12：YSCM 二进制结构 —— **Confirmed**

样本：`%ysbin\ysc.ybn`（解压后 11,102 字节）。验证：`python3 scripts/probe_yscm.py /tmp/ysc.ybn`
（全部逐条断言通过）+ `cargo test --test sample yscm_parses_121_commands_and_1113_params`。

```
[Header 0x10]  magic "YSCM" + version(555) + command_count(121) + unknown(0)
[Body]         121 × { name\0  u8 param_count
                       param_count × { param_name\0  u16 type } }
[Tail]         0x2749 起 1045 字节（结构 Unknown，原样保留）
```

- 命令 121 条 / 参数 1113 个；命令名唯一、可打印；body 干净收尾于 0x2749

- 首命令 ALIAS(0 参数)，CG(58 参数：ID:0x0001, IDNO:0x0100, X:0x0000, TSX:0x1500…)，
  末命令 PROJECTFOLDER；SYSTEMMODE(0x75) 的参数含 FILEPRIORITY\* 三键

- 参数类型码直方图：低字节 ∈ {0,1,2,3}，高字节 ∈ {0x00..0x1c}（语义 Unknown）

- 规格：`docs/formats/yscm.md`；Rust 实现 `yuris-format::yscm`（上会话已写，
  本会话独立验证 + 补样本集成测试）

### 成果 13：content 自描述编码 + 共享池窗口模型 —— **Confirmed**

验证：`python3 scripts/probe_opcode_scan2.py "pac/bn.ypf"`（全语料扫描）。

1. **指令编码**：`[op:u8][operand_len:u16 LE][operand]`，自描述变长。
   全语料 278 文件 / 506,351 条指令 / 194,600 槽位，非 tag0 窗口**零失败**精确闭合。
   除 0x4d 外每个 op 的操作数宽度全语料恒定。
2. **收敛性检验通过**：30 种 opcode（原 18 种是 74 文件的子集）。
3. **密钥统一**：278 个可判定文件全部恢复出 `2b904f93` —— **同一游戏一个密钥**，
   不是逐文件。12 个空脚本（command\_len==0）无槽位可判定。
4. **窗口模型（架构修正）**：content 是**共享字节码池**，槽位是窗口注记。

   - 顺序型（74/302）：窗口无缝覆盖全池，连续性 1.0（yst00000 属此类）

   - 池式（~~200/302）：窗口重叠（同字节被 2~~5 窗口引用），tag0 文本窗口是
     其他窗口的**前缀截断**，正确密钥下连续性也只有 \~0.70

   - 非 tag0 窗口全部独立闭合；tag0 窗口不保证

   - 3 文件末槽位越界（off==content\_len）——特殊记录，Unknown
5. **新结构事实**：`part1_len == part4_len == 4 × unknown1` **302/302 成立**
   （⇒ 命令区 XOR 相位恒 0）。part1 = 小值域 u32 表（如 yst00000: 529×169+530×32+13×1），
   part4 呈 4 字节周期模式；与 slot tag 的位置相关性检验不成立，语义仍 Unknown。
6. **文本载体**：本游戏对话文本在 `sc.ypf`（36 个 `scenario\*.txt`，SJIS 明文，
   语法 `\BG(...)` / `\VO(...)(ID:4987)\LE(...)\LT(...)`）—— ybn 内无明文日文。
   另发现**第 4 种虚拟根前缀** **`-`**（已知 `$`/`%`/`9`）。

### 成果 14：Opcode 表第一版 —— P1.3 产出

`docs/opcode/opcode-table.md`（30 opcode 全表 + 证据等级 + 交叉比对结论）。

- **Whirlpool 锚点 6/8 在 v555 语料原样出现**（420100=pushint8 ×115,182、
  570200=pushint16 ×11,004、48030040=pushscalarvar ×124,126、3D0000=equal ×21,258、
  260000=logand ×1,646、7C0000=logor ×1,331）—— 跨引擎版本编码稳定性强证据。
  5A0000/530000 也在（837/387 次），但 ge/le 的方向判定降为 Likely。

- **字面量族**（宽度自证）：0x42'B'=1B、0x57'W'=2B、0x49'I'=4B（样本形似 ARGB 颜色）、
  0x4c'L'=8B（大整数）、0x46'F'=8B（**IEEE double 10.0/2.0 解码成功**）。

- **算术/比较 ASCII 对应**（Hypothesis）：0x2a'\*' 0x2b'+' 0x2d'-' 0x2f'/' 0x25'%'、
  0x3c'<' 0x3d'=' 0x3e'>'、0x21'!' 0x5e'^'。

- **变量引用**（0x48/0x56/0x76，3 字节）：`[type_tag][id_lo][id_hi]`，
  type\_tag ∈ {0x40×67k, 0x24×18k, 0x23, 0x60}（语义 Unknown）。

- **已证伪**：「tag 低字节 = YSCM 命令下标」（0x16↔FLASH、0x21/0x22↔G\_INT 不匹配）。
  tag>>16 实测分布 {0,1,2,3,4,0x101,0x102,0x103,0x201..0x701}，语义 Unknown。

### 实现落地（Rust）

- `yuris-format::ystb`：`segment_window()`（Confirmed 编码切分）、
  `RawInstr`、`window_score()`、`key_score()`；`guess_key()` 升级为
  连续性 ∨ 窗口闭合率 —— **池式文件也能自动猜钥**（yst00034 测试覆盖）

- 集成测试新增 4 个：YSCM 全量断言、顺序型全窗口闭合、池式密钥+窗口、空脚本

- 探针脚本：`scripts/probe_yscm.py`、`scripts/probe_opcode_scan2.py`（可复现）

***

## 2026-09-03 — Ghidra 逆向 + 官方 SDK:opcode 语义大面积解除

**环境**:Ghidra 11.3.2 + JDK 21(均为用户态安装,macOS 原生);官方 SDK 0.495
已下载解包至 `yu-ris_sdk_0495/`(yu-ris.net/download → down.cgi?yu-ris\_0495\_031\_z,24.7MB)。

### 成果 15:官方 SDK 落地(验证方式:解包 + 交叉比对脚本)

- **YSCom.ycd**(编译器命令字典,v495/119 命令):结构
  `{名\0, u8 参数数, 参数×{名\0, u16 a, u16 type}}` ——
  **第二 u16 与 v555 YSCM 类型码 639/639 逐个一致**,命令顺序一致 → 类型系统跨版本稳定

- **手册**(マニュアル/YU-RIS/html):完整命令参考。关键页:基本文法(`CMD[key=expr]`、
  四则运算 `+ - * / %`)、变量(`@`数值/`$`字符串、INT/FLT 均 64 位)、
  IF(`== != > >= < <= && ||`)、MACRO(编译期文本替换)、系统变量(`_TIME`/`_PINT(n)`…)

- **ERIS 源码**(data/script/ERIS/\*.yst):`#=ES.GAMEMAIN.LOOP` 标签语法、
  点分变量名 —— 字节码 M-串里的 `es.*` = **ERIS 脚本的标签名**

### 成果 16:YSCom.exe 编译器逆向(340 函数全量反编译)

关键函数已存档至 `docs/reverse/decompiled/`(详见 `docs/reverse/yscom-compiler-notes.md`):

1. **密钥公式(Confirmed)**:`key = CRC32(种子串) 大端 4 字节`。
   默认种子 `"Yu-ris"` → CRC32 = 0xd36fac96 → 字节 `d3 6f ac 96`
   \=《SDK编译器调用》默认密钥 `0x96AC6FD3`(LE 读法)——两文互证。
   样本游戏 `2b904f93` = 种子被改(种子串未暴力命中,引擎侧待定位;
   引擎内已找到同一 CRC32 表 @文件偏移 0x192c80)
2. **YSTB 写出器**:header 逐字段吻合;**XOR 每区独立** **`key[i&3]`** **重新计数**
   (非跨区连续;与"绝对相位"模型仅在 `part1_len ≡ 0 (mod 4)` 时等价 ——
   该条件 302/302 成立,故语料不受影响,但实现必须按分区模型)
3. **表达式发射器**:`[op][len:u16][operand]` 逐字节构造;
   字面量按值域自动选 0x42/57/49/4c(int8/16/32/64)、0x46(double)、
   0x4d(字符串,自动加引号 0x22)

### 成果 17:opcode 语义大面积解除(30 个中 27 个 Confirmed)

编译器运算符映射(与 tokenizer 逐行对齐)+ 语料 + 手册三方一致:

```
0x2b +   0x2d -   0x52 一元-   0x2a *   0x2f /   0x25 %
0x3d ==  0x21 !=  0x5a >=  0x53 <=  0x3e >   0x3c <
0x41 &   0x4f |   0x26 &&  0x7c ||  0x5e ^
0x73 $() 转字符串        0x69 @() 转整数
0x48 变量值 / 0x56 变量引用 / 0x76 下标形式 —— 操作数 [前缀字符][id:u16]
     前缀: @=0x40  $=0x24  #=0x23  `=0x60(与语料 type_tag 完全一致)
0x29 数组元素读取(维数校验)   0x2c 表达式组分隔
```

详见 `docs/opcode/opcode-table.md` 第二版。剩余 Unknown:
**流程控制(0x00/0x01/0x08 + GO/GOSUB/IF 编译形态)**、tag>>16 语义、
part1/part4、v555 密钥种子串。

### 成果 18:P2.2 指令解码器落地(`yuris-script`)

- `Insn` 枚举:PushInt/PushFloat/PushStr/PushVar/PushVarRef/PushVarIndexed/
  ArrayLoad/Binary(16 种二元运算)/Unary(Neg)/Cast(ToStr/ToInt)/GroupSep/
  **Unknown{raw\_op, operand}**(未证实原样保留,绝不猜)

- `VarRef { space: VarSpace, id: u16 }`,VarSpace = At/Dollar/Hash/Backtick/Unknown(u8)

- `decode_window(&[u8])`:已知 opcode 宽度不符 → Unknown(诚实降级)

- 集成测试(样本):yst00000 全 402 窗口 868 条指令**零 Unknown**,
  逐 opcode 分布断言(int=402/var=78/varref=123/aload=123/str=32/add=99/groupsep=11);
  首窗口语义断言 = `pushvarref $1226; pushint 400; pushint 1; add; aload`
  —— 与手册 `$S(10,5)` 数组语法吻合;池式 yst00034 解码;M-串全引号包裹

- CLI:`yuris ystb disasm` 子命令

- **测试:27 passed / 0 failed**(此前 17 + 新增 10)

### 成果 19:P2.1 `yuris-value` + P2.x 表达式求值器落地

**yuris-value**:

- `Value { Int(i64), Float(f64), Str(Vec<u8> /*SJIS 原样*/) }`(类型=手册 Confirmed)

- `VarRef/VarSpace` 从 script 移入(变量模型归值层);`VariableStore` 键 = (前缀字节, id)

- 运算语义逐条标注:add(拼接 Confirmed/环绕 Likely)、sub/mul/div/mod(除零显式报错)、
  eq/ne(字符串字节相等 Confirmed)、ge/le/gt/lt、bitand/bitor/xor(仅 Int,Likely)、
  logand/logor(真值合并)、neg、cast\_to\_str(Int→十进制 Confirmed;Float→格式化 Unknown 报错)、
  cast\_to\_int(纯数字串 Confirmed;非数字 Unknown 报错;Float 截断 Likely)

- 数组存储模型 Unknown → 未实现;未定义变量读取显式报错(引擎默认值 Unknown)

**yuris-script 求值器**(`eval.rs`):

- `Evaluator::eval_instructions/eval_window`:栈式执行已证实语义;
  PushVarRef/PushVarIndexed/ArrayLoad → Unimplemented(存储模型未解);
  Unknown 指令 → 新增 `Error::UnresolvedOpcode{code, offset}`

- 样本烟雾:yst00000 全 402 窗口执行,Ok+Err=402、Ok>0、错误类别全部符合预期、零 panic

- **测试:35 passed / 0 failed**(27 → 35)

### 成果 20:store 语义 + P2.4 VM 骨架(`yuris-vm`)

**引擎语料勘验(有界搜索)**:

- `004da1b2`(23KB)/`004fa579` 的 case 0x42/0x48/0x4d/0x2b 簇是**误报**:
  前者携带 `0x10300000`/`0x60900004` 等打包常量 = 渲染/场景节点类型;
  后者含**行号报错**(`invalid use of '%s'` @行 0xbbd)与 token 期待
  (`FUN_004cbd5f(0x24)` = 期待 `$`)→ 引擎内置**明文剧本运行时解释器**
  (sc.ypf 直译)。**引擎存在两套脚本系统**:编译 YSTB(ERIS 层)+
  明文剧本解释器(场景层)—— 与 v5xx 商业版直接发 plaintext 剧本互证

- 0x00/0x01/0x08 的执行点仍未定位(搜索被 CRT/渲染噪声淹没)→ 保持 Unknown

**store 语义(按证据实现)**:

- 语料中不存在独立 store opcode;所有证据(指令/数据分离、PushVarRef 服务
  赋值左值、tag 标注窗口、空文本槽)指向 **槽位级绑定模型**:窗口求值结果
  由 tag 绑定到引擎对象;tag → 对象的精确映射 Unknown → 以事件输出

**P2.4 VM 骨架**(`yuris-vm`):

- `YurisVm::load/run(budget)/resume(ResumeResponse)`:执行单元 = 槽位语句

- `VmState { Idle, Running, Finished, Error }`;`VmSuspend { None, Complete, Error }`

- **指令配额**:跨槽位累计、槽位原子执行(预算检查在槽位边界,至多超出一个窗口)

- **suspend/resume**:配额耗尽 → `None` 可续;走完 → `Complete`/`Finished`;
  未证实语义 → `Error` 态挂起(pc 停在出错槽位,事件可复盘,resume 显式拒绝)

- **`VmEvent`** **事件流**:`Statement{slot,tag,result}`(store 绑定)+
  `TextWindow{slot,len}`(tag0 注记)—— Golden Test 的数据源

- 合成脚本测试:在测试内构造**完整加密 YSTB**(分区 XOR 模型)走 VM;
  真实样本烟雾:yst00000 预算递进至挂起、配额单调、事件非空

- **测试:38 passed / 0 failed**(35 → 38)

### 成果 29:存储模型落地 + LET/数组读写 + Golden Test 框架(测试 49 → 59)

**引擎处理器展开(0x48/0x56/0x76/0x29 运行期,反编译 extra/ 全读)**:

- **0x56 = 平行左值引用栈**({栈位, id, kind},不装值)——赋值目标的忠实机制

- **0x76 = 延迟占位**(标记下标形式;与 0x56 的配对细节 = Likely)

- **0x29 = 引用点收集下标表**(引用点到栈顶的纯值项,推入序 = 维度序)→
  逐维越界检查(0x1a202/0x1a20c)→ **行主序线性化(最后一维最快)**
  → 装载 8 字节元素;数组元素 = 描述符 bounds\[];帧局部(id 0x32/33/34/35/46)
  边界来自 GOSUB gparam;id>999 需描述符(0x1a5ea)

- 数组读编译形态实测:`@5000[2]` = `76 03 00 40 88 13` + `42 01 00 02` + `29 01 00 00`

**Rust 落地**:

- `yuris-value`:`ElemType`/`ArrayStorage`(行主序 `linear_offset` + 越界/维数断言

  - 类型检查 + 清零初始化)、`compound_assign(B3 0..8)`、`VariableStore` v2
    (arrays 表 + declare/get\_elem/set\_elem/array\_dims);+5 测试

- `yuris-script`:求值器升级 **Slot 模型**(Val/LValue 忠实建模平行引用栈)、
  `eval_slots`/`eval_lvalue_window(_instrs)`;PushVarRef/ArrayLoad **解锁**;
  PushVar = 标量或数组 0 号元素(load\_auto);+2 测试(54)

- `yuris-vm GroupVm`:**LET(0x35)实现**(双窗;复合码 = w0.B3;
  帧局部 id 0x32-0x35/0x46 → 诚实 Unsupported)、YSVR 初始化链测试
  (3362 条解析 → 标量/数组初值应用);+3 测试(57)

- **P3.2 Golden Test 框架**:事件流 JSONL 快照(Str=十六进制字节、Float=位模式,
  快照字节稳定);`YURIS_REGEN_GOLDEN=1` 再生成;**自基线**(引擎真值待接);+2 测试(59)

- 测试组织:make\_ystb/DUMMY 抽到 `tests/common/mod.rs`

**测试:59 passed / 0 failed**(49 → 59)

### 成果 30:命令处理器表全提取 + LOOP 族 + LET 落地(测试 59 → 61)

**命令处理器表逆向(Confirmed)**:

- FUN\_0046305c 完成 YSCM 参数表初始化后,把 121 项处理器表(0x78b020,BSS)
  填默认报错 stub(0x45c4d4),再逐项赋值 **60+ 个实际处理器地址**(全提取)

- **运行期表索引 = YSCM 下标 + 8**(12+ 锚点吻合:GO 0x2a→0x32、GOSUB→0x33、
  IF→0x34、LET 0x35→0x3d、RETURN 0x4f→0x57、RETURNBREAK(编译期 no-op)→FUN\_00423080、
  END→0x15、CG→0x09、LABEL→0x3b no-op);声明类 F\_*/G\_*/S\_\* 落在 stub 槽
  (0x10-0x31 无赋值)——「声明组是加载期数据」再次自洽

- 前 8 项(索引 0-7)= 系统伪命令保留位,语义 Unknown

**LOOP 族反编译(0x3f/0x40/0x41/0x42)+ 语料验证**:

- 语料实测 LOOP 组恒 2 窗:w0=计数表达式(INT,B2=1;实测 `pushvar @1433`),
  w1=循环体表达式(tag0,99-131 字节)

- LOOP:压循环记录 {body\_start=pc+1, counter=1, limit};深度上限 0x40(0x18ce0 报错)

- LOOPEND:counter+1;counter≥limit → 弹帧顺序继续;否则 PC=body\_start
  (64 位计数;0xffffffff/-1 = 无限标记;计数细节 Likely)

- LOOPBREAK/LOOPCONTINUE:LV 参数 = 回退层数(默认 1),回退到目标层的
  退出点 / 体顶部(引擎 record+8 的确切来源 = Likely)

**Rust 落地**:

- `GroupVm`:LOOP/LOOPBREAK/LOOPCONTINUE/LOOPEND 四命令(LoopFrame、
  JumpKind::LoopBreak/LoopContinue、`find_paired_loopend` 动态配对);
  **LET(0x35)实现**(双窗;复合码 = w0.B3;帧局部 id 0x32-35/0x46 → 诚实 Unsupported)

- `yuris-script`:求值器升级 **Slot 模型**(Val/LValue,引擎平行引用栈的忠实建模);
  PushVarRef/ArrayLoad **解锁**(aload = 引用点收集下标 → 行主序装载 → sp 重置);
  `eval_lvalue_window`(LET 左值窗 = 引用+下标,无 aload)

- `yuris-value`:`ArrayStorage`(行主序线性化+越界/维数/类型断言+清零初始化)、
  `ElemType`、`compound_assign(B3 0..8)`、`VariableStore` v2(标量+数组双表)

- 端到端测试:LOOP 3 次迭代(LET 计数器读回=3)、LOOPBREAK 提前退出
  (IF+aload 条件)、LET 帧局部诚实 Unsupported、YSVR 初始化链(3362 条)

- **测试:61 passed / 0 failed**(59 → 61)

### 成果 31:WAIT/TEXT/CG/SOUND 事件化落地(测试 61 → 66)

**处理器反编译(extra/,逐项读)**:

- **WAIT(0x455d58,3 参)**:FRAME(槽0)→ obj+0x18 帧计数,让出;TIME(槽1)→
  obj+0x1c = timeGetTime()+ms,让出;两者都无 → 不等待。**第一个真实挂起点**

- **TEXT(0x452888)**:FILE 字符串参数作显示文本(带 `\`/`/` 转义、`\r\n` 结尾、
  LET/CLEAR 布尔);**不阻塞**(点击等待在文本层,非本 VM)

- **CG(0x43c984)**:求值全部参数按 B0 槽填;YSCM 参数序 ID/X/Y/Z/SIZE/… 58 项

- **SOUND(0x44dae0)**:ID/FILE/PLAY/FADE/LOOP/VOLUME/PAN/PITCH/SPEED 等

**GroupVm 落地**:

- `VmSuspend::Wait { counter, time_ms }`(WAIT 挂起;resume 继续)

- `VmEvent::Text/Cg/Sound`(事件化;ID/位置/PLAY/BOOLEAN 字段)

- WAIT 无参数 → 不挂起直接继续;TEXT/CG/SOUND 逐窗求值 → 事件

- `Value::as_int_opt/str_as_string` 助手移入 yuris-value

**测试 +5(66 全过)**:wait\_counter\_suspends、wait\_no\_param\_no\_suspend、
text\_emits\_text\_event、sound\_emits\_sound\_event、cg\_emits\_cg\_event\_with\_position。

**关键修正(记勘误)**:

- 早期把 `cmd::CG = 0x01` 加进常量后,两个「未实现命令」测试(用 0x01)与之
  冲突暴露 —— 改用真正未实现的 0x0e(ERROR)/0x1f;**命令下标常量必须是 YSCM
  真实下标**,先查 command-layer.md §7 再定义

- tag 的 **B0 = 最低字节(LE)** — 窗口 tag 数组 `[0x04,0,0,0]` 才是槽=4;
  我误写 `[0,0,0,0x04]`(小端读到 B0=0)导致 X/Y 测不出

- `0x42`(pushint8)装 200 会溢出为 -56 → 测试改用 pushint16(0x57);
  编译器按值域自动选宽,手写合成脚本必须自己选对

### 成果 32:GOSUB 帧局部存储 + LET 帧局部解锁(测试 66 → 67)

**GOSUB 处理器反编译(CMDH\_004428c0)帧布局**:

- 帧 = `malloc(0x328)`,新帧压 `obj+0x148 + depth*4`;**RETURN 弹帧即丢弃局部**

- INT 局部 帧+8(每槽 8B)计数 +0x2bc;FLT +0x90 计数 +0x2cd;STR +0x120 计数 +0x2de

- gparam 维数解码:`int=(u16&0xff)>>3`、`flt=(u16&7)*4+(u16>>14)`、`str=(u16>>9)&0x1f`

- 实参:窗口 B0 槽(1..,条件槽 0 跳过)求值后拷入帧对应类型区

- **LET 处理器(FUN\_00443808)**:左值 id 查变量描述符 `DAT_0087240c[id]` byte+1 得类型
  → 帧内对应类型区读写 → **帧局部 id = 声明命令 id(0x32-0x35/0x46)**,独立空间

**Rust 落地**:

- `GosubFrame` 增加 `locals: VariableStore`(帧局部独立存储;非 Copy 化);
  弹帧(RETURN)自然丢弃 —— 与引擎局部生命周期一致

- `Evaluator::with_locals(store, 帧局部)`:PushVar 读帧局部优先,再查全局
  (帧局部 id 独立空间;GroupVm 用字段级借用 `frames.last()` + `&mut store` 解除 borrow 冲突)

- `seed_frame_locals`:gparam 解码 + 窗口实参(槽 1..)求值写入帧 locals

- LET 帧局部(id 0x32-0x46):无帧挂起(引擎 0x18e6e 同族)/有帧写当前帧 locals;
  **系统变量族(0x75+)仍走全局**

- **测试 +1**:gosub\_frame\_local\_let\_readwrite(GOSUB 帧内 LET 帧局部读写 →
  RETURN 后帧丢弃、全局不受污染);let\_frame\_local\_no\_frame\_halts(无帧挂起)

- **测试:67 passed / 0 failed**(66 → 67)

### 勘误:ELSE 与 IFBLEND 语义纠错(成果 33 前导)

**实测命令类型(probe\_part1\_groups 直方图,83465 组/70 种命令)+ 处理器反编译**:

- **ELSE = 0x0b**(1517 次),**IFBLEND = 0x2d**(1517 次)——二者数量相等配对。
  旧文档 §4 写 ELSE=0x2d **错误**;YSCM 名表第 0x0b 条即 ELSE

- **真实语义**(处理器 0x43d34c=ELSE、0x432e4=IFBLEND)：

  - **ELSE\[expr]**:求值 w0;真/无窗 → **顺序进 else 块**;假 → 跳嵌套栈顶 end

  - **IFBLEND**:无条件跳到嵌套栈顶 end(跳过 else 块)

  - **IF 假** → PC = w1.len ?: w2.len(w1.len=ELSE 起点,w2.len=end;处理器 0x431ec)

- 旧实现把 ELSE 当"无条件跳 end"是误解;`cmd::ELSE=0x2d` 恒错,
  现改为 `cmd::ELSE=0x0b`、`cmd::IFBLEND=0x2d`

- 相关测试与 golden 基线已按真实语义更正(if\_false 链,golden 再生成)

## 2026-09-03 — 引擎命令层逆向:流程控制全部解除(U2 关闭)

**方法**:锚点过滤(真求值器 switch 含 case 0x42/0x48/0x4d/0x2b 各恰好一次)→
字符串锚点 `ysbin\yst%05d.ybn` → 加载器数据流 → 处理器表初始化 → 主循环。
新增探针:`scripts/probe_engine_interp_scan.py`、`probe_engine_va.py`(PE 节表 VA→off)、
`probe_part1_groups.py`(全语料逐条断言)、`probe_tag_semantics.py`、`probe_yscm_index.py`。
Ghidra 补建 85 个命令处理器函数(`decompile_handlers.py`,输出 `/tmp/ghidra_all/.../extra/`)。

### 成果 21:YSTB = 命令流(三层模型) —— **Confirmed**

引擎加载器 FUN\_00450dfd + 全语料断言 302/302(`probe_part1_groups.py`):

```
part1[i] (u32): byte0=YSCM 命令下标, byte1=窗口数 count, [2:4]=u16 gparam
commands = Σcount_i × 12B 窗口记录(tag/len/offset)
content(+part4) = 表达式字节码池
断言: part1_len==4G ∧ Σcount_i*12==command_len, 302/302 零失败
```

- 旧「未知1/part1/part4 语义 Unknown」解除;「共享池+窗口注记」模型升级为
  「窗口=命令实例的参数槽」(池共享 = 重复参数只编译一份的空间优化,仍成立)

- 744 窗口(149 文件,全 tag0)off+len 伸入 part4 —— content 逻辑上延续进 part4

### 成果 22:主执行循环 = 命令级线程化解释器 —— **Confirmed**(FUN\_0040449c)

```
while (!quit && !yield) { pc = obj->pc++; yield = A[sel][pc](); }
```

- 双处理器数组:X\[0]=加载后全为等待/消息泵 stub(0x45c4ec),X\[1]=命令表
  DAT\_0078b020\[type]\(BSS,由 FUN\_0046305c 初始化,121 项,默认=报错 stub 0x45c4d4)

- 处理器返回非零=让出(suspend);obj 倒计时/定时字段驱动 WAIT 恢复

- **obj+0(unk1) 加载后复用为 PC**

### 成果 23:流程控制语义(GO/GOSUB/RETURN/IF/LET) —— **Confirmed**

- **GO(0x2a)**:len 域高位=0 时解码标签名→Murmur2 哈希表(FUN\_0045124c,乘数
  0x5bd1e995)查 id;PC=标签表项{目标组PC@+8,脚本号@+0xC};跨脚本则惰性加载重绑

- **GOSUB(0x2b)**:条件窗口假→跳过;gparam 解码帧局部数组维数;压帧{ret\_pc=pc+1,
  脚本号,记录指针,字符状态};实参写入新帧;标签可 (B3<<8|B2) 内联 id 或字符串

- **RETURN(0x4f)**:返回值写调用者帧;弹帧恢复 PC/脚本;depth==0 → 脚本结束

- **IF(0x2c,恒 3 窗)**:条件假 → PC = w1.len ?: w2.len(**len 域存编译期组号**);
  嵌套栈 obj\[0x40+level\*4]

- **LET(0x35,恒 2 窗)**:左值 kind=描述符 byte+1(1=INT/2=FLT/3=STR);复合赋值码
  \=B3(0..8 = 赋值/+=/-=/\*=//=/%=/&=/|/^=);变量空间=声明命令 id(0x32=INT/0x33=FLT/
  0x34=STR/0x35=LET 局部/0x75+/0x90+/0x119+=系统变量)

- 语料块结构佐证:IF↔IFEND 各 8649、LOOP↔LOOPEND 各 946、ELSE↔IFBLEND 各 1517

- **表达式层无流程指令**;语料 0x00/0x01/0x08 疑为 tag0 前缀截断解析伪影(待复核)

### 成果 24:tag 位段 + 参数求值器 —— **Confirmed(结构)**

- tag=\[B0]\[B1]\[B2]\[B3]:**B0=YSCM 参数下标**(GOSUB/RETURN 稀疏填参 0x00/0x10/0x20)、
  B1 恒 0、**B2=值类型**(0=通用/1=@/2=FLT/3=$;F\_INT→1/F\_FLT→2/F\_STR→3 吻合)、
  B3=复合赋值码/标签族;旧「tag>>16=实例编号」**作废**

- 参数求值器 FUN\_004253cd:按 YSCM 参数 kind(0=int/1=str/2=延迟指针)选求值表,
  attr2=值域校验规则(min/max 表+特例);MOVIE 处理器消费端按参数槽取值互证

- YSCM tail 解码(**勘误**):35 条 CRT 错误消息 + 256B 映射表 + 4B 0,
  785+256+4=1045 逐字节闭合;「tail=系统变量表/配置键」证伪

- 求值 thunk:int=00420bdc/00420c0c、flt=00420c60/00420c8c、str=00420a90/00420ba8
  (最终解释器在其内部,待展开 = U2b)

- 引擎初始化(FUN\_00468160)发现 "revsiruy"(yu-ris 反写)缓冲(U4 线索,未定位 CRC 调用点)

详见 `docs/engine/command-layer.md`(证据分级全文)。

### 成果 25:Rust 落地(组模型 + YSCM tail)+ YSVR 变量定义表 —— **Confirmed**

**Rust(测试 38 → 42,全过)**:

- `yuris-format::ystb`:`CommandGroup` + `groups()`(引擎模型逐条断言:
  part1\_len==4G ∧ Σcount\*12==command\_len)+ `group_windows/group_first_slots` +
  `window_bytes_pooled(_copy)`(窗口可伸入 part4)

- `yuris-format::yscm`:`YscmTail::parse_tail`(引擎消费模型)

- 新集成测试 4 个:全语料 302 文件组模型断言(含 GO=103/IF=8649/LET=18315/
  part4 溢出=744/tag B1 恒 0)、yst00000 直方图(169×F\_INT+32×F\_STR+1×END)、
  yst\_list≠YSTB 诚实报错、YSCM tail

- 环境坑:沙箱禁 exec Xcode.app 的 clang → `export DEVELOPER_DIR=
  /Library/Developer/CommandLineTools` 恢复链接

**YSVR = 变量定义表(探针 probe\_vartab.py,3362/3362 条目精确闭合)**:

- `%ysbin\ysv.ybn`:magic YSVR + version@+4 + u16 条目数@+8 + 条目流@+10

- 条目 = {kind(1/2/3=全局/按脚本/?), 类别, 脚本号, 变量 id, 类型(1=INT/2=FLT/
  3=STR/0=仅声明), 维数, 边界\[], 初值};样本 kind=1226/1226/910、
  type=2357/127/426/452、id 0..7570

- **运行期变量无名字**:变量 = 纯 id,LET 的 LHS id = 声明命令 id
  (0x32=INT/0x33=FLT/0x34=STR/0x35=LET 局部/0x75+ 系统变量);
  名字只在编译期(ERIS 源码)存在 → t5 关闭

- 启动链 FUN\_0046b63c:YSCM → 变量描述符 → YSVR → 应用初值 →
  哈希查入口标签 → 建任务 → 加载脚本 → 运行

- 新发现容器:yst.ybn = **YSTD**(16B)、ysl.ybn = YSLB(139KB,疑标签表)

### 成果 26:表达式最终解释器展开(U2b 关闭) —— **Confirmed**

- **FUN\_00420acc 主循环**: `i += *(u16*)(base+i+1) + 3; err = handler_table[*pc]()`
  —— 与我们 Confirmed 的编码 `[op:u8][len:u16][operand]` 逐字段一致

- **处理器表 FUN\_0046a21c**(256 项,BSS):全部 27+ opcode 的引擎处理器地址
  逐项对齐编译器语义(0x2b add=004225b0、0x3d equal=00422700、0x4d str=00420cb8…)
  ——opcode 语义现为**编译器+引擎双侧 Confirmed**

- **0x2c GroupSep = FUN\_00423080 = ALIAS 的 no-op 处理器** —— 我们求值器的
  no-op 处理直接获得引擎佐证

- **0x00/0x01/0x08 = 默认报错处理器** —— 引擎侧证实它们不是有效指令,
  「tag0 前缀截断解析伪影」假说加强(Likely)

- 变量类(0x48/0x56/0x76)双态:载入期共用占位 00421a3c(加载器解码标签窗口时
  不求值),运行期各自独立 —— 与加载器 FUN\_0046a21c(0)/(1) 切换吻合

- 详见 `docs/engine/command-layer.md` §5b(op→处理器地址全表)

### 成果 27:YSLB 标签表 + murmur2 + Rust 解析器三件套 —— **Confirmed**

- **`%ysbin\ysl.ybn`(YSLB) = 标签表**(引擎 FUN\_00463c7c):magic YSLB + ver +
  u32 标签数 + 256×u32 桶头 + 条目×{len, name, hash, target\_pc, script\_id, 标志×2}
  样本 **4153/4153 条、murmur2(name)==hash 零失败、139051/139051 精确闭合**

- **引擎查找哈希 = murmur2(seed=0)** Confirmed(4153 条全量);
  已入 `yuris_core::hash::murmur2`

- **Rust 新增**:`yuris-core` murmur2 + 单测;`yuris-format::ysvr`(YSVR 变量表)、
  `yuris-format::yslb`(YSLB 标签表)解析器;样本集成测试 +4(42 → 46 全过)

- **语料交叉验证**:GO 99 M-串窗 98 命中 YSLB(4 未命中=截断伪影,U2c 同族);
  GOSUB tag0 M-串 27021 条命中 26942(99.7%)——未命中=其他字符串参数
  (引擎静默跳过) → GO/GOSUB 跳转在 Rust VM 可完整实现

- 引擎怪癖:标签 stride 按 len+13 读,flag\_b 实际吃到下一条 len 字节(兼容兼容)

### 成果 28:yuris-vm 组级重构(旧槽位 VM 替换) —— Confirmed 模型落地

- `GroupVm` 替换旧 `YurisVm`(旧「槽位级绑定模型」已勘误作废):

  - 执行单元 = 命令组;PC = 组下标(引擎先取后增语义)

  - **已实现(引擎 Confirmed 语义)**:GO(标签表查找+同脚本跳转+载入期标记
    分支识别)、GOSUB(条件求值→压帧{return\_pc=pc+1}→标签跳转,未命中静默)、
    RETURN(弹帧;帧空=脚本结束)、IF(条件求值;假跳 w1.len?:w2.len;嵌套栈)、
    ELSE(跳栈顶 end)、IFEND(弹栈)

  - **诚实边界**:跨脚本跳转(需多脚本重绑)、其余命令语义(未逆向)→
    Unsupported 事件;strict(默认)挂起 / trace 模式记录后继续 —— 不猜

  - 标签表经 `set_labels` 注入(来源 YSLB);`set_script_id` 支撑跨脚本判定

- 测试重写:合成脚本走完整加密管线;覆盖 IF 假→ELSE→IFEND→RETURN 链、
  IF 真→GO 标签跳转、GOSUB/RETURN 往返、strict 挂起、trace 继续、
  真实样本 yst00000(首组 F\_STR 声明 → Unsupported,引擎亦不执行声明组)

- **测试 46 → 49 全过**(yuris-vm 3 旧 → 6 新)

***

### 成果 33:END + 声明类命令处理 + ELSE/IFBLEND 语义纠正(测试 67 稳定)

**勘误纠正(重要)**:

- **ELSE = 0x0b**(1517 次),**IFBLEND = 0x2d**(1517 次)——配对。旧文档 §4 写
  ELSE=0x2d **错误**;YSCM 名表第 0x0b 条即 ELSE

- 语义(处理器 0x43d34c=ELSE、0x432e4=IFBLEND):ELSE\[expr] 真/无窗→顺序进 else 块、
  假→跳嵌套栈顶 end;**IFBLEND 无条件跳 end**;IF 假→跳 w1.len?:w2.len(处理器 0x431ec)

- 命令类型=YSCM 下标(实测 0x2a=GO/0x68=WAIT/0x01=CG 与文档 §7 一致);
  `cmd::WAIT=0x68` 等常量正确;命令表数据槽= 0x78b020+4\*(cmd+8)

**实现**:

- **END(0x0d)**:设结束标志→ Complete;可选返回码

- **声明类命令**(:INT/FLT/STR/G\_*/S\_*/F\_\* 共 \~23 种):引擎运行器用默认 stub 不执行
  (加载期数据)。GroupVm 记录 `VmEvent::Declaration` 后**继续**(与引擎一致)

- 高价值待落地清单锁(S\_INT/S\_STR/FLT/CGACT/LOAD/VARACT/SAVE 等,见直方图)

**测试:67 passed / 0 failed**(sample 声明测试、trace 未实现测试适配;golden 再生成)

***

### 成果 34:YSTD/YSER 结构(低成本项,测试仍 67 绿)

- **YSTD (yst.ybn, 16B)**:`YSTD`+ver 555+`0x1dba`+0 —— 恒 16B,引擎语料无引用
  (本引擎可能未用);f8/f12 语义 Unknown

- **YSER (yse.ybn, 6645B)**:`YSER`+ver 555+count 123;`+0x14` 起**连续 C 串错误
  消息池**,终点精确封闭(245 条,0x19f5==0x19f5,零残留)—— **Confirmed(池封闭)**;
  内容=日文错误模板(`メモリ不足です。`/`%s`/`[内部エラー]` 等);
  count(123)与池条目(245)的映射、+0x0c/+0x10 字段、池内个别非 SJIS 字节
  (疑似长度前缀/分段)→ **Unknown 不猜**

- 规格:`docs/formats/yser.md`;探针:`scripts/probe_yser_ystd.py`(可复现断言)

***

### 成果 35:U5 解除 —— content+part4 是连续池(测试仍 67 绿)

- **结论(Confirmed)**:content 与 part4 逻辑上一大段,`content ‖ part4` 为完整
  表达式/文本池;窗口 offset 相对**拼接段**。

- **决定性验证**:302 脚本/194,634 窗,拼接后 `off+len<=ctlen+p4len` **零越界**;
  744 个"溢出窗"起点恰在 ctlen、尾部伸入 part4(全 tag0=文本窗)。

- `part4_len==4G` 双重性统一为:part4=content 逻辑尾部,长度恰 4G 是分配巧合,
  **非"每组 u32 表"**。

- 既有 `window_bytes_pooled_copy` 已按跨界拼接读法,与结论一致,无需改动。

***

### 成果 36:U4 有界侦查(revsiruy 已定性,未破,如实记录)

- 引擎 .rdata 存在 `"revsiruy"`(VA 0x87d410),"Yu-ris" 正写不在引擎 → 反写串
  是引擎保留的种子片段线索(编译器用正写 "Yu-ris")。

- **已证伪**:`CRC32("revsiruy") = 0x5b1a1e99` ≠ 样本密钥 CRC `0x2b904f93`;
  "revsiruy" **不是** YSTB 密钥种子(该串可能用于其他用途,如另一派生/邮箱序)。

- 样本密钥 `2b904f93`(已 Confirmed,268 文件自动猜测)的种子串仍 **Unknown**:
  引擎内 `Yu-ris` 正写不存在,暴力 ASCII 子串匹配未命中,唯一 CRC 函数调用者
  (00515221/0051523d)是通用增量校验和。**U4 不虚报为完成**。

- 此非"打开游戏"的技术阻塞:密钥已知,VM 已能跑;仅种子别名学术性待考。

***

### 成果 37:P0 跨脚本跳转落地(测试 67 → 68)

**动机**:GOSUB 占语料 27065 组(最高频),真实流程在 yst%05d.ybn 间不断跳转;
GroupVm「跨脚本 GO/GOSUB → Unsupported」是必须解除的诚实边界。

**实现**(基础已由 成果 27/30/32 备齐):

- 新增 `yuris-vm::host`:`ScriptCtx`(script\_id + 组表 + 窗口起始下标)、
  `ScriptHost` trait(`load(id) -> ScriptCtx`)、`InMemoryHost`(注入/缓存,测试用)

- `GroupVm` 重构:单脚本字段 `script/groups/first_slots/script_id` → `ctx: ScriptCtx`

  - `host: Option<Box<dyn ScriptHost>>`;`load()` 仍单脚本(host=None,向后兼容),
    `set_host()` 启用跨脚本

- **GosubFrame 增加** **`script_id`**(引擎帧含脚本号字段,成果 32 已记)

- **跨脚本语义**:

  - GO:标签名 → 标签表{target\_pc, script\_id};script\_id 不同 → `CrossScript` →
    `switch_script(切 ctx + PC)` → `VmEvent::ScriptSwitch`

  - GOSUB:压帧{return\_pc, script\_id=当前脚本} → 目标脚本不同则切换上下文

  - RETURN:弹帧 → 帧 script\_id ≠ 当前 → `switch_script` 切回(引擎:帧含脚本号)

- **测试 +1**:cross\_script\_gosub\_and\_return(InMemoryHost 注入 2 脚本,跨脚本
  GOSUB→目标脚本 LET 写全局→RETURN 切回原脚本→结束;ScriptSwitch/Jump-Return 断言)

- **验收达成**:跨脚本 GOSUB/RETURN 零 Unsupported,流程控制命令全集跨脚本可跑

- **测试:68 passed / 0 failed**(67 → 68)

***

### 成果 38:P0 第3项 —— 启动链编排器 + 真实样本端到端(测试 68 → 69)

**实现**:

- `yuris-vm::host` 增加 **`YpfScriptHost`**(拥有 `YpfArchive`,`static` 可进
  `Box<dyn ScriptHost>`;按 script\_id 拼 `$ysbin\yst%05d.ybn` 读取 + 解析 + 缓存;
  额外 `read_entry` 供启动链读 YSLB/YSVR)

- `GroupVm::apply_ysvr`:应用 YSVR(kind==1 全局初值)标量/数组,写入 store;
  kind==3 Unknown 跳过不猜

- **`yuris-vm::boot`** **Bootstrap 编排器**(引擎 FUN\_0046b63c):
  `读 YSLB → 查入口标签(SYSTEM_START)→ 载入口脚本 → 应用 YSVR 初值 →
  建 GroupVm + set_host + set_labels → set_pc(入口组)`

- LET 左值窗三种语料形态——单条 0x48 PushVar(10901×)/0x56+下标+aload(7384×)/
  0x76(30×)——加单条 PushVar 快速路径(对应 SYSTEM\_START 里的 `LET @1041=0`)

- `VarRef::display`(`@1041`/`$F`)改善未定义变量报错

**真实样本端到端**(bootstrap\_system\_start\_reaches\_end):

- 从 bn.ypf 的 SYSTEM\_START(script 0x111=yst00273)启动,自动跨脚本
  GO/GOSUB 到 es.ERIS → es.S.GO → es.BID.GRP.NO.CHK → es.\_strlen …,一路
  ScriptSwitch/GO/GOSUB/RETURN 全部走通,**流程控制命令零 Unsupported**
  (LET/IF/IFEND/GO/GOSUB/RETURN 全落地执行;let 帧局部可读写)

- 结束于撞上未实现命令(0x67 VARINFO)——**非流程命令**,如实报告

- 验收:跨脚本链推进多组、有 ScriptSwitch、流程控制零 Unsupported → **达成**

**修复的关键语义**:

- LET 左值窗不总是 0x56 引用:**0x48 PushVar(仅取 id)也是合法左值**(语料
  10901 例,占 60%),此前实现只认 0x56;已加单指令快速路径

- 启动链需**先应用 YSVR 初值再跑入口**,否则 LET/IF 读到未声明变量

- YpfScriptHost 必须**拥有** YPF(否则 `Box<dyn ScriptHost>` 生命周期无法 'static)

- 测试:69 passed / 0 failed(68 → 69)

***

### 成果 39:P2 第一梯队 —— VARACT(0x66)/VARINFO(0x67)只读落地(测试 69 → 72)

**逆向**(反编译 `CMDH_00453178`(5290B)+`CMDH_004550a0`(2775B);处理器地址
LAB\_00453178/LAB\_004550a0 来自 FUN\_0046305c 命令表,下标换算 slot=index/4、cmd=index-8):

- 两命令先 `FUN_004253cd()` 参数求值,再按 **B0=参数下标** 槽分发
  (`DAT_006624aN`);变量描述符 `DAT_0087240c[id]` 按 byte+1 类型分支。

- 所有路径收尾 `FUN_0045baf8()` 显示查询结果,返回 0。

- YSCM 参数名:VARACT 29 参(SET/LET/CUT/COPY/POS/LENGTH/TYPE/…/PUSH/POP/
  INIT/G\_INT..G\_STR4);VARINFO 22 参(SET/LET/TYPE/STRTYPE/DIMNUM/
  DIMSIZE..8/LENGTH/SEARCH/STRFIRST/SJISCODE/INT/FLT/STR/NO/NO2)。

- 真实调用(script190 g4):B0=13=LENGTH,引用 `$0x37[1]`(STR 数组,YSVR 17 元素)

  - `pushvar @0x188e`;即 LENGTH 查询(Likely:字节长度,按 SJIS 步进表)。

**落地**(铁律分级):

- `cmd::VARACT=0x66` / `cmd::VARINFO=0x67`;`VmEvent::VarQuery{pc,command,evaluated}`

- **只读槽可执行**:VARINFO{TYPE/STRTYPE/DIMNUM/DIMSIZE..8/LENGTH/SEARCH/
  STRFIRST/SJISCODE}+VARACT{TYPE/DIMSIZE}——求值记录,不写游戏状态。

- **写回槽走 Unsupported**:SET/LET/CUT/COPY/PUSH/POP/UPPER/LOWER/INIT/G\_\* —
  未逆向写回目标,不猜。`0x67` 的 a2/a3… 内对调试缓冲的写入是临时信息,
  非变量存储 —— 不实现。

- 文档:`docs/engine/varact_varinfo.md`。

**验收**:

- 合成测试:varinfo\_length\_readonly\_query(STR 长度查询)/varinfo\_writeback\_halts\_strict
  (SET 写回 strict 挂起);真实样本 sample\_script190(0x67 组窗口全部可解码;
  `@0x188e` 不在 YSVR=帧局部,如实报告;`$0x37` 经 YSVR 交叉验证)。

- **启动链越过 0x67**:bootstrap 从 SYSTEM\_START 继续推进,撞上下一个
  未实现命令(非流程) —— 流程控制命令零 Unsupported 保持。

- **测试:72 passed / 0 failed**(69 → 72)。

***

## 勘误记录

| 日期         | 错误结论                                              | 更正                                                                                                      | 原因                                                                                         | <br />               |
| ---------- | ------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | :------------------- |
| 2026-09-05 | 「CGINFO 槽13/14 应答 = 静态默认 SX=1/SY=1」（成果 62）        | **= CG 装载图像的真实宽/高**（watch oracle:s9 g1089 occ1 写 1.0 = tip\_meswindow.png 1×1、occ2 写 1350.0 = txspace 1350×200）；「默认 1/1」是 occ1 恰好命中 1×1 占位图的巧合 | engine\_trace.py `--watch-arm-ev` 延迟布防硬件写监视 + 包扫描双证；g1092 IF = 「是 1×1 占位图」判定 | <br />               |
| 2026-09-05 | 「YpfIndex 判别式 `is_bn = !(落界)`」（成果 65 勘误 A 的修正遗留取反） | `is_bn = b0∈{0,1} ∧ bn 解释落界`（去取反）；取反使 **sc.ypf 自第 2 条起逐条漂移 1 字节**，bn 包条目名/读取全错 | 勘误 A 只用 cg.ypf（纯 se,走类型码路径）验证,恰好测不到 bn 分支；bn 包一直走 YpfArchive 掩盖；sc\_zlib\_text\_read 真跑暴露 | <br />               |
| 2026-09-05 | 「引用槽写回目标 = 窗口最后一条 var 指令」（extract\_var\_target）    | 基点 = **首条** var 指令,其后至 0x29 = 下标表达式,**写/读时延迟求值**（引擎 kind 2）；旧实现把 `56 @1265 | 48 @1704 | 29` 的下标变量 @1704 当基点 → CGINFO/VARINFO/VARACT/SAVE/LOAD 写错位 | 与成果 44 的 LET 左值窗同族语义,引用槽 8 个调用点漏同步；对拍 [144055] 分歧定位              | <br />               |
| 2026-09-04 | 「0x48 在 LET 左值窗与 0x56 同为引用语义」（成果 43）              | **0x48 运行期一律推值**（VARH\_00420ec4）；0x56 才是引用标记；左值窗基点=**首条** var 指令，其后 0x48 是下标表达式                         | 全语料扫描：LET w0 含 aload 的 6836 窗全部 0x56 起头（0x48 起头 0 个）+ 引擎 0x29 延迟模式反编译；端到端 pc264 把下标变量当数组报错 | <br />               |
| 2026-09-04 | 「YSVR kind2 由调用方按 script\_id 决定」（apply\_ysvr 注释）  | 启动链应（且现已）应用 kind1+kind2；**kind3 永不匹配 = 死数据**（FUN\_00451348 in\_EAX 匹配规则反编译）                             | 端到端 $1895(kind2) 未声明卡死；1226 条 kind2 全部丢失                                                   | <br />               |
| 2026-09-04 | 「undefined array variable 641897」中的 641897 是变量 id | 是**显示拼接 bug**（前缀字节 0x40 直接拼十进制 id）；已改 VarRef::display                                                   | 0x40                                                                                       | 1897 十进制拼接 ≠ 任何合法 id |
| 2026-09-03 | 「ELSE=0x2d / 无条件跳 end」                            | **ELSE=0x0b**(真/无窗顺序进 else 块、假跳 end);IFBLEND=0x2d 才无条件跳 end                                             | 实测命令直方图 + 0x43d34c/0x432e4 处理器反编译;旧 §4 下标笔误                                                | <br />               |
| 2026-09-03 | 「命令类型=YSCM 下标+8」                                  | 命令类型字节(part1 byte0)= YSCM 下标本身;命令表槽=0x78b020+4\*(cmd+8)                                                 | 实测直方图 0x2a=GO/0x68=WAIT 与文档 §7 一致                                                          | <br />               |
| 2026-09-02 | 「YPF 索引解析后精确闭合于 `first_data_off`」                 | 末条 tail 仅 4 字节，残差 −4；首个名字在 `0x24` 而非 `0x20`                                                             | 早期用 `len+26` 模型反推总和，未做逐条间距实测；写脚本时断言失败才暴露                                                   | <br />               |
| 2026-09-02 | 「`16 00 03 00` 作为 u32 LE = 0x03160000」            | u32 LE = `0x00030016`                                                                                   | 手算复核，未逐字节核对                                                                                | <br />               |
| 2026-09-02 | ystb.md §6 首槽位 22 字节 listing 混入下一槽位 5 字节          | 22 字节止于 `29 01 00 00`；`4d 02 00 22 22` 是槽位 #1                                                           | 自描述编码逐指令闭合时暴露                                                                              | <br />               |
| 2026-09-02 | 「off\[n]+len\[n]==off\[n+1] 全程成立」是密钥判定器           | **仅顺序型文件成立**；池式文件正确密钥下 \~0.70，判定改用 `key_score()`（连续性 ∨ 窗口闭合率）                                           | 固定密钥全语料扫描，216 文件连续性≠1.0，逐槽位解剖 yst00034 发现窗口重叠                                              | <br />               |
| 2026-09-02 | ystb.md §5 约束 1「offset==0 → key=cipher\[4..8]」    | offset 在 cipher\[**8..12**]；\[4..8] 是 len 字段                                                            | 对 yst00034 逐字段验证                                                                           | <br />               |
| 2026-09-02 | 隐含假设「每个 YSTB 的 content 槽位是独立程序」                   | content 是**共享池**，槽位是窗口注记；tag0 窗口是前缀截断                                                                   | tag0 窗口在指令中间截止、字节被 2\~5 窗口重叠引用                                                             | <br />               |
| 2026-09-03 | XOR 相位按"绝对偏移"跨区连续                                 | 编译器实测**每区独立重新计数** `key[i&3]`；两模型仅在 `part1_len ≡ 0 (mod 4)` 时等价（该条件 302/302 成立，语料不受影响）                   | 逆向 YSCom.exe 的 YSTB 写出器（004160ac）                                                          | <br />               |
| 2026-09-03 | opcode 第一版大量标 Likely（ASCII 假说）                    | 升级 **Confirmed**（编译器映射逐行对齐）；新增解出 0x52=一元负号、0x41/0x4f=单字符位与/或、0x73/0x69=`$()`/`@()` 类型转换                 | Ghidra 全量反编译 YSCom.exe + tokenizer 对齐                                                      | <br />               |
| 2026-09-03 | 「content 是共享字节码池，槽位是窗口注记」+「tag>>16=实例/参数编号」       | 槽位 = **命令实例的参数窗口**（part1=命令表，byte0=YSCM 下标/byte1=窗口数）；tag>>16 观测值实为 (B3<<8\|B2) 类型组合；池共享=重复参数的空间优化（仍成立） | 引擎加载器 FUN\_00450dfd 逆向 + 全语料 302/302 断言                                                    | <br />               |
| 2026-09-03 | 「YSCM tail=系统变量表/配置键（结构 Unknown）」                 | tail = 35 条 CRT 错误消息（日文）+ 256B 映射表 + 4B 0，逐字节闭合；配置键（FILEPRIORITY\* 等）是 SYSTEMMODE 的参数名，在 body 内         | probe\_yscm\_index.py 按引擎 FUN\_0046305c 模型解析                                               | <br />               |
| 2026-09-03 | 「YSCM tail=35 条消息+256B 表+4B 0（785+256+4 闭合）」      | **37 条**消息（789B）+256B 表=1045，尾部无剩余；引擎 do-while `i<0x91,i+=4` 先解析后判步恰 37 次；「35 条」读法把末两条空格串误并入表           | Rust 测试断言失败暴露；逐字节重验（又一次「总和闭合≠逐条对上」）                                                        | <br />               |
| 2026-09-03 | 「最大阻塞项=流程控制 0x00/0x01/0x08」                       | 表达式层**无流程指令**；流程控制全部在命令层（GO/GOSUB/IF/LOOP/RETURN 处理器）。0x00/0x01/0x08 疑为 tag0 前缀截断窗口的解析伪影                | 引擎主循环 FUN\_0040449c + 命令处理器逆向                                                              | <br />               |

> 教训：**总和对得上 ≠ 逐条对得上**。后续所有格式结论必须做逐条断言，不能只看总量。
> 教训 2：**「在 A 样本上成立」≠「格式性质」**。连续性不变量曾在 74 个文件上全成立，
> 但那只是顺序型形态的巧合；必须扫全语料再下结论。

***

## 当前结论汇总表

| 层       | 项                                     | 等级                                     | 备注                                                                           |
| ------- | ------------------------------------- | -------------------------------------- | ---------------------------------------------------------------------------- |
| 格式      | YPF header                            | **Confirmed**                          | 4 字段 + 索引区头部 4 字节（用途未知），样本 v500                                              |
| 格式      | YPF entry 布局                          | **Confirmed**                          | 308 间距逐条吻合；末条 tail 被 `first_data_off` 钳制，解析后精确闭合                             |
| 格式      | YPF 文件名 XOR 0xC9                      | **Confirmed**                          | 仅本样本；跨版本待验                                                                   |
| 格式      | YPF 虚拟根前缀 `$`/`%`/`9`                 | **Confirmed（存在）** / 语义 Unknown         | 属名字一部分，与尺寸算术自洽                                                               |
| 格式      | YPF se 型条目类型码 PNG/OGG/WAV           | **Confirmed**                          | 0x02/0x06/0x05(解码 0xCB/0xCF/0xCC);第二样本实证(成果 70/71)                        |
| 格式      | YPF 名字边界(0xC9 尾字节假 NUL)           | **Confirmed**                          | SJIS 尾字节 0xC9 异或后=存储 0x00;结构化校验消歧(成果 70 §3)                          |
| 格式      | YPF entry tail 8 字节                   | Unknown                                | 疑似校验和                                                                        |
| 格式      | YSTB header                           | **Confirmed**                          | 8 字段，样本 v555                                                                 |
| 格式      | YSTB XOR（4 字节循环，跳 0x20）               | **Confirmed**                          | 机制确认；密钥可**自动猜测**（成果 9）                                                       |
| 格式      | YSTB command 定长 12 字节                 | **Confirmed**                          | 偏移自洽，零越界                                                                     |
| 格式      | YSTB part1                            | **Confirmed**                          | = 命令实例表（byte0=YSCM 下标/byte1=窗口数/gparam u16）；`Σcount*12==command_len` 302/302 |
| 格式      | YSTB commands                         | **Confirmed**                          | = 参数窗口表（tag=B0 参数下标+B2 类型+B3 族 / len / off）                                  |
| 格式      | YSTB content/part4                    | **Confirmed(池)** / part4 恒等式含义 Unknown | 窗口可伸入 part4（744 处）；`part4_len==4G`                                           |
| 格式      | YSCF                                  | **Confirmed**                          | 字段级解析完成（成果 10）                                                               |
| 格式      | YSCM 结构（header+命令表+tail）              | **Confirmed**                          | 121 命令 / 1113 参数；tail=35 CRT 消息+256B 表（成果 24）                                |
| 格式      | YSCM = 脚本命令表（名+参数+类型码）                | **Confirmed（结构）**                      | 引擎按命令下标→处理器表；参数 kind/attr2 参与求值与校验                                           |
| 格式      | YSTL / YSLB / YSVR                    | **Confirmed（存在）** / 结构 Unknown         | 样本中各有 1 个                                                                    |
| 格式      | YSTB content = 共享池 + 窗口注记             | **Confirmed**                          | 顺序型 74 / 池式 \~200 / 空脚本 12；tag0 窗口前缀截断                                       |
| 脚本      | 指令编码 `[op][len:u16][operand]`         | **Confirmed**                          | 506,351 条指令零失败精确闭合                                                           |
| 脚本      | opcode 种类（30 种）+ 每种宽度                 | **Confirmed**                          | 全语料统计，收敛性检验通过                                                                |
| 脚本      | opcode 名称（27 Confirmed 编译器+引擎双侧）      | **Confirmed**                          | 详见 docs/opcode/opcode-table.md 第二版                                           |
| 脚本      | 算术/比较 ASCII 对应（2a/2b/2d/2f/25/3c/3e…） | **Confirmed**                          | 编译器映射；表达式层无流程指令                                                              |
| 脚本      | 对话文本在 sc.ypf 明文 SJIS（本游戏）             | **Confirmed**                          | 36 个 scenario\*.txt 实测                                                       |
| VM      | 命令层执行模型                               | **Confirmed**                          | 命令级线程化解释器：pc 先取后增、双分发数组、返回非零让出（成果 22-23）                                     |
| VM      | GO/GOSUB/RETURN/IF/LET 语义             | **Confirmed**                          | Murmur2 标签哈希表、调用帧压/弹、编译期组号跳转、复合赋值码（成果 23）                                    |
| VM      | 表达式求值语义（27 op）                        | **Confirmed**                          | 编译器+引擎双侧（成果 26）                                                              |
| VM      | 变量存储模型                                | **Confirmed(装载)/Likely(0x56/0x76 配对)** | 平行左值引用栈、行主序线性化、8B 元素（成果 29）；帧局部未实现                                           |
| VM      | LET/数组读写(Rust)                        | **Confirmed 语义落地**                     | GroupVm LET + ArrayStorage + YSVR 初始化链（成果 29）                                |
| Golden  | P3.2 框架(自基线)                          | **Confirmed(机制)**                      | JSONL 快照 + 再生成；引擎真值待接                                                        |
| VM      | 变量系统                                  | **Confirmed(寻址)/Likely(声明消费)**         | 变量空间=声明命令 id、元素 8B、描述符 byte+1=类型；声明组=加载期数据                                   |
| VM      | YSVR kind 语义                          | **Confirmed**                          | FUN\_00451348:1=全局(全量)/2=按脚本(script 匹配)/3=死数据不应用（成果 44）                      |
| VM      | LET 左值窗形态                             | **Confirmed**                          | 0x48=推值、0x56=引用标记；基点=首条 var 指令；6836/6836 窗 0x56 起头（成果 44 勘误）                 |
| VM      | VARACT CUT/COPY/POS/LENGTH            | **Confirmed**                          | 字符区间 \[POS-1,POS-1+LENGTH) SJIS 步进;POS=1 基字符序数;空串→空串不报错;守卫 0x1d4ca/d4 在引擎为致命退出(成果 73)  |
| VM      | VARINFO LENGTH-on-STR = 字符数           | **Confirmed(汇编)**                     | 0x4551dc 步进循环 EAX 计数器入栈,strlen 仅循环界;曾误判字节数致 s190 pc=36 停摆(成果 73)                          |
| Runtime | Scene / Layer 模型                      | **Unknown**                            | 有 YSCM 参数名线索（SX/SY/RLX/RLY…）                                                 |
| 资源      | UI 素材 `_N` 变体剥离回退                   | **Likely(对拍)**                        | 系统剧本字面量 `sound_2/tip_cgauge` 等 ↔ 包内 `sound\tip_cgauge.png` 剥 `_N` 6+ 例全中;resolve_entry 已落地(成果 74) |
| 资源      | cgsys_ec.ypf 名字首字节(0x10~0x3B)       | Unknown                                | 盘上名字自带,非解析漂移/非名字哈希;可打印时即「前导杂字节」;查找由剥根双索引免疫(成果 74)               |
| 资源      | config 系 UI 按钮 btn_all_mask/btn_c01~16/other/* 真缺失 | **Confirmed(字节级)**                | 12 包 XOR-0xC9 检索不存在;引擎同表现空按钮,非分歧(成果 74)                          |
| 资源      | 剩余 151 条失败根因 = 产品未打包可选素材             | **Confirmed(算术闭合)/Likely(引擎同执行)**   | 97 路径↔s250~s254 字面量逐组钉死,失败次数==字面量数(40+6+32+67+6=151);config 仅 sound/system/text 三页,通用钮替代 per-channel 钮;引擎未找到=设计路径(成果 75) |
| UI        | 标题按钮盲推进(点任意按钮都进游戏) → 已路由         | **Confirmed(命中路由)/Likely(START 落入)**   | 根因 = `\TITLE`+Wait::Line 盲推进,按钮纯贴图;Wait::TitleMenu+poll_title_menu 五按钮命中路由,END 实测 exit 0,LOAD 失败不落穿(成果 76) |
| Runtime | 免封包优先级                                | **Confirmed（机制）**                      | YSCM 含 `FILEPRIORITY*` 键                                                     |
| 资源      | 图片格式                                  | **Likely**                             | YSCM 含 `BMP PNG JPG GIF AVI PSB WEBP`                                        |
| 资源      | 音频格式                                  | **Likely**                             | YSCM 含 `WAV OGG`                                                             |

***

## 2026-09-03 八段续 — 引擎「引用槽」模型勘误 + 全脚本声明消费(成果 40-43,测试 75 → 76)

### 成果 40:VARINFO/VARACT SET/LET 槽勘误(端到端卡点解除) —— **Confirmed**

- **勘误**:YSCM 参数名 SET(0)/LET(1) 的真实角色 = **变量引用槽**(kind 2 延迟求值),
  **不是写回操作**。旧实现把 B0=0(SET) 判为写回 → 端到端卡死。

- 处理器 CMDH\_004550a0(VARINFO) 直接证据:所有分支从槽 0 出发;STRTYPE 分支
  把判定结果(1=半角/2=全角)**写入 LET 引用的下标元素**(写回目标=LET 槽)。

- 接收器三件套定型:`FUN_0045baf8`=INT / `FUN_00454630`=FLT / `FUN_0045b9f8`=STR
  (VARINFO/VARACT 各查询分支按目标类型选接收器 → 全部语料组合自洽)。

- 全语料槽组合分布(探针 probe\_varinfo\_slots.py):SET 从不单独出现,恒伴随操作槽;
  最高频 VARACT\[SET,DIMSIZE]×131 / VARINFO\[SET,LET,DIMSIZE]×131 /
  VARACT\[SET,POP]×113 / \[SET,PUSH]×93。

- VARINFO 查询落地(Rust):TYPE(2)/DIMNUM(4)/DIMSIZE(5..12=第 N 维)/LENGTH(13=
  SJIS 字节长)/fallback=LENGTH;结果写 LET 目标(Scalar set / Indexed set\_elem);
  SEARCH(14)/STRFIRST(15)/SJISCODE(16) 未实现 → strict 挂起(不猜)。

- VARACT DIMSIZE(13) 落地:1 维数组 resize(引擎 desc+2==1 检查;
  `VariableStore::resize_array` 保留前缀元素,**Likely**)。

### 成果 41:全脚本声明组消费(引擎描述符表全局共享) —— **Confirmed(形态)/Likely(默认值)**

- 端到端证据链:启动链读 `@6292`/`@1417`/`@1416` 报「未定义变量」→ 这些 id 由
  **yst00190 的 INT 组 / yst00000 的 F\_INT 组**声明(probe\_decl\_find.py 全语料定位)。

- 引擎变量描述符表 `DAT_0087240c` **全局唯一**,启动时消费**全部脚本**的声明组
  (非按脚本惰性)。Rust:`Bootstrap` 经 `host.script_ids()` 遍历 302 脚本
  `consume_declarations_into`(store 插入);`switch_script` 亦消费(幂等)。

- 声明命令族(YSCM 名表 Confirmed):FLT 族(0x10/0x19/0x1d-0x20/0x52)/
  INT 族(0x11/0x32/0x21-0x24/0x53)/STR 族(0x12/0x5c/0x25-0x28/0x54);
  VAR 族(0x13/0x29/0x55)类型不定跳过(Unknown)。

- 窗口形态(探针 probe\_decl\_forms.py):FLT/INT/STR 组全语料 **4248 组全部标量**
  (w0=单条 48/56 引用 + w1=名字 M-串);F\_ 族含**数组上界引用**形态
  (`56 $1226 + 400+1+add + aload` → declare\_array(bounds=\[401]),
  同数组多条声明取最大上界扩容;上界语义 **Likely**)。

### 成果 42:CGINFO(0x04) 事件化 —— **Confirmed(不存在路径)/Likely(结果值)**

- 处理器 CMDH\_0043b084:ID(槽0)=CG 名;LET(槽33)=写回目标;
  其余槽=查询项(EXIST/X/Y/SX/SY/ONMOUSE…),全部汇入结果缓冲
  (函数头 `DAT_005c0840=0` 清零)→ 接收器写 LET。

- 全语料 245 组槽组合规整(ID+LET 必带;probe\_cginfo\_slots.py)。

- **无图形后端 ⇒ CG 必然不存在 ⇒ 恒走引擎「不存在」路径**(iVar6==0 →
  清零缓冲写 LET)= 写 Int(0),引擎同路径 Confirmed,非猜测。

- `VmEvent::CgInfo{pc,id,evaluated}` 事件。

### 成果 43:LABELINFO(0x34) 真查询 + 流程控制两处修复 —— **Confirmed**

- **LABELINFO** 处理器 CMDH\_00443674(402B):`#`(槽0)=标签名、LET(槽1)=写回、
  EXIST(槽2) 非零 → FUN\_0045124c(**murmur2 查标签表**)→ 命中写 1/未命中写 0
  (写 LET 引用元素)。Rust 用 YSLB 标签表真查 —— 非退化路径。

- **LOOP limit=0 修复**(端到端踩坑):LOOP(limit=0) 不能「立即 pop+落下一组」——
  循环体内的 GOSUB 返回后 LOOPEND 将无配对。引擎 LOOPEND 逻辑
  「counter+1 ≥ limit → 弹帧」对 limit=0 自然退出 → **LOOP 恒压帧**(勿回退)。

- **LET 左值 0x48 多指令形态**:左值上下文中 0x48 与 0x56/0x76 同为引用语义
  (引擎 LET 处理器按左值窗 id 写回);eval\_lvalue\_window\_instrs 统一变换
  PushVar→PushVarRef 后走引用+下标收集。

- 处理器反编译已存仓库 `docs/reverse/decompiled/engine/`(VARACT/VARINFO/
  CGINFO/LABELINFO/变量类/解释器主循环,不再只写 /tmp)。

**端到端推进**:SYSTEM\_START → VARINFO ✓ → CGINFO ✓ → LABELINFO ✓ →
LOOP/LOOPEND/GOSUB 链(	script22 es.BT 主流程)零 Unsupported 深入推进;
流程控制命令全程零 Unsupported。测试 75 → 76(全过;varinfo 两测改新语义)。

***

### 成果 44:端到端推入游戏主循环 —— YSVR kind2 / 左值窗基点 / VARACT CUT 三修复(测试 76 → 79)

**驱动**:端到端在 script22 pc264 卡死(`undefined array variable`),
追查揭出**三个独立错误**,全部修复后推进越过了此前所有卡点,
进入 es.BT 每帧 `WAIT(FRAME=1)` 的**游戏主循环**(无渲染后端下
表现为永不 Complete 的帧循环 —— 非死循环,是引擎帧模型本身)。

#### 修复 1:YSVR kind==2(按脚本初值)从未被应用 —— **Confirmed**

- **勘误**:`apply_ysvr` 旧注释称「kind2 由调用方按 script\_id 自行决定」
  且调用方从未做 —— 1226 条 kind2 条目全部丢失,$1895(STR\[100]) 无声明源。

- **引擎真值(新反编译** **`FUN_00451348`,已存** **`docs/reverse/decompiled/engine/`)**:
  `in_EAX = script_id`(启动链 FUN\_0046b63c 传 0xffffffff=全量;
  脚本加载器 FUN\_00450dfd 加载后也调一次),匹配规则 =
  `kind1 ∧ EAX==0xffffffff` 或 `kind2 ∧ EAX==条目脚本号`;**kind3 永不匹配**
  (910 条 = 死数据,不应用不猜测)。

- **本语料实测(探针)**:kind1+kind2 合并 (prefix,var\_id) **零冲突**(2452 条互异);
  抽样 60 个 kind2 变量**全部只在所属脚本被引用** → 启动时急切应用 kind1+kind2
  与引擎「按脚本惰性」在本语料不可区分。Rust:`apply_ysvr` 应用 kind1+kind2,跳过 kind3。

#### 修复 2:LET 左值窗「0x48=引用」是过度概括 —— **Confirmed(勘误)**

- **勘误(推翻成果 43 的「0x48 在左值窗同样携带引用语义」)**:引擎
  VARH\_00420ec4(0x48)运行期**推值**、VARH\_004218b0(0x56)才推引用;
  0x29(VARH\_00421a4c)命中**底层引用**且延迟模式时不装载、保留 {基点,下标}
  交 LET 写回(`iVar7==0 && db970!=0 → return 0`)。

- **全语料形态扫描(决定性)**:LET w0 含 aload 的窗口 **6836 个全部 0x56 起头**
  (首指令 0x48 的多指令窗 **0 个**);值窗含 aload 的 **15930 个全部含 0x56/0x76**。

- 旧实现把窗内全部 0x48 统一变换为引用 → `eval_slots` 的 ArrayLoad 用
  `rposition` 取**最后一个** LValue 当基点 → 下标变量 @1897 被当数组,
  报 `undefined array variable 641897`(= 字符 `@` 0x40 打头拼 id 的显示 bug,
  顺带修正为 VarRef::display)。

- **新语义**:基点 = **首条** var 类指令(0x48/0x56 统一);其后 0x48 保持
  **取值语义**(下标表达式);末条 0x29 剥除(引擎延迟模式不装载)。
  `eval_lvalue_window_instrs` 重写,合成测试 `let_lvalue_window_0x48_index_is_value_not_ref` 覆盖。

#### 修复 3:VARACT CUT(2)/COPY(3)/POS(4)/LENGTH(5) 落地 —— **Confirmed(结构)/Likely(越界近似)**

- 全语料槽组合直方图(360 组):`[SET,DIMSIZE]×131`、`[SET,POP]×113`、
  `[SET,PUSH]×93`、`[SET,LET,COPY,POS,LENGTH]×15`、`[SET,CUT,POS,LENGTH]×4`、
  其余单写槽 ×1。**DIMSIZE/PUSH/POP 恒无 LET 槽**(与引擎直接改数组描述符自洽)。

- 处理器逐分支落地:a3(COPY)=截取字符区间 \[POS-1, POS-1+LENGTH) 写 LET 目标;
  a2(CUT)=写出切除该区间后的剩余;POS∈{0,1}→首字符(引擎专门短路);
  **对象空串 → 两步进循环不执行 → 结果空串,不报错**(端到端 script190 pc333
  空串卡点);非空串越界报错(0x1d4ca/0x1d4d4 同族)。SJIS 双字节按步进表(0x81-9F/E0-EF)计 2。

- 未实现写槽(UPPER/LOWER/HANTOZEN/ZENTOHAN/INIT/G\_\*/TYPE 转换)诚实挂起(不猜)。

- 合成测试 `varact_copy_substring_and_empty_object_guard` 覆盖(含空串守卫)。

#### 附带修复

- **eval\_slots ArrayLoad 基点**:仍用 rposition(值窗内 0x56 基点在栈底,
  与引擎引用栈单标记一致),LET 左值窗已改为首引用;合成测试回归通过。

- **switch\_script 重复全扫声明组**(性能):GO/GOSUB 高频跨脚本时每跳
  O(组数) 重扫成为主热点(sample 实测 consume\_declarations 占 48%)。
  以 `declared_scripts: HashSet<u16>` 跟踪,每脚本只全扫一次;语义不变(声明幂等)。

- **bootstrap 端到端测试预算化**:主循环每帧 WAIT 让出、驱动立即 resume →
  无后端下永不 Complete(实测 4.4 亿组仍在推进)。测试以 200 万组为界,
  验收不变(跨脚本切换 + 流程控制零 Unsupported);`diag_flow.rs` 同步预算化。

- `Evaluator.verbose` 调试输出还原为默认关闭。

**端到端现场(修复后)**:SYSTEM\_START → es.ERIS → es.BT 主流程 →
VARACT COPY/POS/LENGTH ✓ → 进入 **script22 es.BT 主循环(pc61,每帧
WAIT FRAME=1 让出)** —— 流程控制、变量读写、字符串操作全程零 Unsupported。
对照:修复前推进 559 组即卡死;修复后 2 秒推 43 万组仍健康推进。

**测试:79 passed / 0 failed**(76 → 79;bootstrap/diag 两测改预算制)。

## 下一步(重排于 2026-09-04 P4 收官;P0-P4 全部达成,workspace 93/93 全过)

**当前态势**(成果 49 收官时点):

- 格式层 / 表达式层 / 流程控制 / 变量读写 / 字符串操作全部 **Confirmed**;
  端到端已推入游戏主循环(es.BT 每帧 WAIT(FRAME=1) 让出,成果 44)

- 引擎真值对拍 99,999/100,000 一致;唯一分歧 = @53\[2]\(引擎系统态,**Unknown**,
  成果 46/47)

- Runtime trait + Null/Mock backend 就绪(成果 48);**真后端 = 0** ——
  yuris-render / yuris-audio / yuris-input / yuris-resource / yuris-save 均空壳,
  yuris-cli 仅骨架

- Scenario 侦查完成(成果 49):32 命令全表 + 解释器区域定位;**解释器未实现**

- **核心判定:引擎是双脚本系统** —— YSTB 字节码(系统层,已跑通)+
  sc.ypf 明文剧本(对话/演出载体 = 游戏内容本体)。「VM 跑通 ≠ 能玩」,
  到「可游玩」缺五块:语义收口(@53/$55)/ 资源层 / Scenario 解释器 /
  渲染后端 / 音频输入存档

**总目标(M0)**:`yuris-cli run <游戏目录>` 一键启动,兼容内核完整游玩本作。
里程碑链:**M1 出画面 → M2 能玩(对话+选择肢)→ M3 存读档 → M4 长线稳定 → M5 OP 视频**。

### 历史计划归档(全部达成,详见对应成果条目)

| 旧计划项                                               | 成果              |
| -------------------------------------------------- | --------------- |
| 表达式层解释器展开 / VM 组级重构 / Golden 框架                    | 26 / 28-33 / 29 |
| YSTD / YYSER / part4==4G / U4 密钥种子串有界侦查            | 34 / 35 / 36    |
| P0 跨脚本跳转 + 启动链端到端                                  | 37 / 38 / 44    |
| P1 Golden Test 接引擎真值(对拍预言机)                        | 45 / 46         |
| P2 高频命令补全(第一梯队)                                    | 47              |
| P3 Runtime backend 起步(trait + Null/Mock + CG 映射草案) | 48              |
| P4 明文剧本解释器侦查                                       | 49              |

### 新计划(P5-P10;每项落地 = 反编译引用 + 合成测试 + 真实样本断言 + 可复现验证)

#### P5 · 语义收口 —— 对拍零分歧 + 长尾命令(可与 P6 并行)

- **P5.1 @53/$55 系统数组定性**(对拍唯一分歧 keystone):扩展
  `engine_trace.py --watch`(硬件监视点)至**读取点**回溯调用栈;排查
  FUN\_00463714/004637d8(建表者,已证非写入者)之外及 WINDOWINFO/FONTINFO/
  DIALOG 处理器群的写入路径。已知现场:desc\[53]={type=INT,dim=1,bounds=\[17]},
  data\[2] 事件 1-6000 恒 0 但引擎 IF 判真 → 引擎读取不经 desc+0x30 布局
  (别名/重定位路径存疑);$55\[1] 引擎侧 8 字符串、VM 空,同族 Unknown。
  **验收**:对拍 100k 零分歧,或分歧逐项定性并记录等级;@53 定性后
  trace 后续引擎专属路径命令自动纳入 VM 覆盖(缺口表 VM=0 项解锁)。

- **P5.2 长尾命令按需补全**:0x65 VAR / 0x16 FLASH / 0x3e MENU(boot 直方图
  频次 0;trace 走远后按 `probe_runtime_hist.py` 缺口表补)。

- **P5.3 golden 基线扩展**:trace 深入标题画面/新游戏流程(依赖 P8 出画面
  后人工推进采集更长真值)。

#### P6 · 资源层 —— 多包挂载 + 格式侦查解码(yuris-resource)

样本现状(pac/ 12 个 ypf 共 \~1.86GB + 4 个 ymv):cg 828MB / update1 491MB /
op+op\_c 250MB / vo 107MB / cgsys\_ec 84MB / bgm 63MB / se 32MB / sysvo 5.5MB /
bn 1.4MB / sc 327KB / sysse 41KB;mv001-004.ymv 共 13MB。

- **P6.1 多包挂载 + 免封包优先级**(FILEPRIORITY 机制 **Confirmed**,汇总表)。

- **P6.2 格式侦查**:probe 各包魔数分布。线索:游戏目录 YSPNG.DLL/YSZLB.DLL
  暗示 PNG+zlib;YSCM 参数名区含 `BMP PNG JPG GIF AVI PSB WEBP` / `WAV OGG`
  (资源格式行 **Likely**)。

- **P6.3 解码落地**:image crate(位图)+ symphonia(音频);PSB/WEBP 若实测
  出现 → 专项逆向;ymv 可占位跳过,不阻塞 M0。

- **验收**:抽样条目「挂载→读取→解码」全链路成功 + 与引擎运行截图对照;
  缺资源走 ResourceNotFound 语义(对齐成果 42「CG 不存在」路径)。

#### P7 · Scenario 解释器 —— 游戏内容载体(实现优先级最高)

依据成果 49:36 文件 32 命令全表(频次定序)、参数解析器 FUN\_004fa579、
宏预处理器 FUN\_004cc10c、处理器群 0x4cb000-0x4fb000(精确派发链 Unknown)。

- **P7.1 联动机制定性(部分达成,成果 62)**:数据注入面已实证
  (scenario → GOSUB 帧局部 → s9 引擎;CG 状态注册表落地,对拍
  [137544]→[294329] 残 4 处单根因);**TEXT(0x62) 在 boot 期执行 0 次**
  —— 行推进调用链需输入注入(engine_trace.py 扩展 SendInput)或
  TEXT 处理器静态逆向,P7.1 续。

- **P7.2 解析器**:段标签表 + 宏预处理(对照 FUN\_004cc10c:`#`/`##`/`#@`)+
  参数解析(对照 FUN\_004fa579;空槽跳位/修饰符链 .D/.G.IF/.CMXYZ)+
  注释剥离(`//`、`/* */`)。

- **P7.3 命令按频次梯队落地**(全库频次,成果 49):

  1. `\T`(3507)推进 + `\LE`/`\LT`(×4515 恒成对双语)对话行 `(ID:)`
  2. `\VO`(2376)/`\VO2`/`\VO4` 语音 + `\WA` 等待
  3. `\BG`(311)/`\EV`(292)/`\S`(219,含 .D 处置)图像
  4. `\BGM`(248,空首槽 = 保持曲目仅调音量)/`\SE`(138)/`\SE2` 音频
  5. `\FOUT`/`\FIN`(141)转场 + `\C`(209)/`\F` 文本样式 + `\FACE`(86)
  6. `\GO`(41,含 .G.IF 全局变量条件跳转)/`\SEL` 选择肢
  7. 长尾:`\WINDOWMODE`(27)/`\QUAKE`(26)/`\EYECATCH`/`\SP`/`\FLASH`/
     `\RP`/`\LOGO`/`\TITLE`/`\END`/`\MOVIE`/`\ENDROLL`

- **P7.4 派发链校准**:逐命令在 0x4cb000-0x4fb000 处理器群断点采引擎真值
  (复用成果 45 hook 链路)。

- **验收**:scenario\_start.txt 全 103 行全链跑通(diff 为空或分歧逐项定性);
  对话行显示 + 点击推进 + 选择肢分支生效。

#### P8 · 渲染后端 —— M1:出画面(yuris-render)

- **P8.1** 窗口 + 设备初始化(winit + wgpu;MSVC 工具链已就绪,成果 47 附记;
  选型为实现选择,非引擎约束)。

- **P8.2** Scene/Layer 图层合成(z 序/位置/缩放/alpha;SceneBridge 已就绪,成果 48)。

- **P8.3** SJIS 文本渲染(字体选型;对照 YSTCH.DLL 行为;换行细节以截图对拍)。

- **P8.4** 效果族:淡入淡出(`\FOUT`/`\FIN`)/ QUAKE / IFBLEND(引擎侧 104 次)。

- **验收 M1**:启动 → LOGO → 注意事项图 → 标题画面;截图 vs 引擎截图逐图层
  对照。CG 槽位语义从 Confirmed 项(ID/位置)起步,\~55 个 Unknown 槽渐进
  升级(截图对拍驱动)。

#### P9 · 音频/输入/存档 —— M2:能玩(yuris-audio / yuris-input / yuris-save)

- **P9.1 音频**:BGM 循环 + 音量淡变(`\BGM(,800)` 空首槽语义)/ SE 并发 /
  VO 流播(vo 107MB);cpal + symphonia(选型为实现选择)。

- **P9.2 输入**:点击推进 / WAIT CLICK / MOUSE / ONMOUSE / Ctrl 跳过。

- **P9.3 存档**:自有格式(全局变量 + scenario 位置 + VM 状态快照);
  `\GO.G.IF` 全局槽持久化。

- **验收 M2**:新游戏 → 首句对话(文本+语音+BGM)→ 点击推进 → 首选择肢
  分支生效;存读档往返一致。

#### P10 · 整合通关验收 —— yuris-cli 收口(M0 总目标)

- **P10.1** `yuris-cli run <游戏目录>` 一键启动(挂载全部 12 包)。

- **P10.2 里程碑链验收**:M1 标题 → M2 开场 → M3 存读档往返 → M4 共通线
  ≥1 小时零 Unsupported / 零 panic / 配额单调 → M5 OP 视频(ymv 逆向,
  可占位)。

- **验收 M0**:兼容内核完整游玩本作。

### 关键路径与风险登记

**关键路径**:P5.1 ∥ P6.2(并行侦查)→ P8.1/8.2(首像素)∥ P7.1(联动定性)→
P7.3(文本出来)→ M2 → M0。**P7.1 为单点最大 Unknown,最先侦查。**

| 风险                   | 等级          | 影响      | 缓解                      |
| -------------------- | ----------- | ------- | ----------------------- |
| ~~@53/$55 写入者未明~~    | **已关闭**(成果 50-51) | — | 小 id 变量无独立写入者:53/55=帧局部族,48=LOOP 计数 |
| YSTB↔scenario 合流点 | **数据注入面已实证**(成果 62) | 行推进链路(TEXT)未达 | boot 无 TEXT;需输入注入或静态逆向 |
| scenario 派发链 Unknown | Unknown     | 命令语义猜测  | P7.4 逐命令引擎真值校准,禁止猜      |
| CG \~55 槽语义 | **SX/SY/COLOR/EXIST 已落**(成果 62) | 画面保真 | 余槽截图对拍逐槽升级;RECTPAINT3 动画归 P8 |
| 引擎帧驱动 CG 颜色动画 | Unknown(确定性实证) | 对拍残 4 处分歧 | P8 效果族建模(两数据点禁猜公式) |
| PSB/WEBP/ymv 专有格式 | **PSB/WEBP 未出现**(成果 63);ymv/ASF 待定性 | 解码成本 | PNG/OGG 明文直读;OP=ASF;ymv 占位不阻塞 |
| SJIS 字体/换行细节         | Likely      | 文本观感    | 引擎截图对拍                  |

### 无限期搁置

- U4 密钥种子串（密钥已知，学术性）

- 版本 Profile 扩展（v4xx 系；先跑通 v555）

- YPF tail 8 字节 / 虚拟根前缀 `$`/`%`/`9`/`-` 语义（不影响运行）

- YSTD f8/f12 字段（引擎语料无引用）

- YSVR kind3 死数据（永不应用，成果 44）

***

## 2026-09-04 — P1:Golden Test 接引擎真值 —— 对拍预言机建成(成果 45-46,测试 33/33)

### 成果 45:引擎真值采集器 —— Windows 调试器 hook 处理器表 —— **Confirmed**

工具链(全部入库,可复现):

| 文件                                     | 作用                                                                                                                                                            |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `scripts/ghidra/hook_trace_setup.py`   | PyGhidra:反编译表初始化器 FUN\_0046305c,**按反编译文本字面量赋值**提取 121 项处理器表(旧「扫 MOV」路线产出恒 0 项,已废弃);补建+反编译 GO/GOSUB/RETURN/task\_activate → `scripts/engine_trace_config.json` |
| `scripts/engine_trace.py`              | ctypes 调试器:对 89 个处理器入口下 int3,命中即读 pc/脚本号/cmd 输出 JSONL(桌面会短暂弹游戏窗口,采集完即杀)                                                                                       |
| `crates/yuris-vm/examples/vm_trace.rs` | Rust 侧:真实 bn.ypf 跑 Bootstrap,事件流按 `yuris_vm::event_json` 导出 JSONL                                                                                             |
| `scripts/diff_engine_vm.py`            | 对拍:两侧归一到 (script,pc,cmd) 序列,前缀对齐+重同步聚类                                                                                                                        |
| `scripts/probe_group_windows.py`       | 组窗口/表达式字节 dump(分歧定位)                                                                                                                                          |

关键逆向结论(hook 所需,**Confirmed**):

- **运行期处理器数组**:loader FUN\_00450dfd 反编译 130-131 行实锤
  `X[1][g] = (&DAT_0078b020)[cmd_g]` —— 处理器表按组展开,处理器地址 ↔ 命令 id
  一一对应(共享 no-op FUN\_00423080 除外)→「断点命中 = 引擎执行一条命令」

- **运行时对象**:pc = `*(u32*)(DAT_00872404+0x20)-1`(先取后增);
  **脚本号 =** **`*(u16*)(DAT_00872404+0x3c)`**(GO/GOSUB 压帧/RETURN 三处反编译交叉验证)

- **启动链单任务**:FUN\_0046b63c 只建一个脚本任务(FUN\_0040ca7d),无并发交错,
  引擎/VM 流可逐事件对齐

- 三道环境关卡(后续复现必读):

  1. 64 位 Python 调 32 位引擎必须 `Wow64Get/SetThreadContext`(普通
     GetThreadContext 按 x64 CONTEXT 写入 → 调试器自身 0xC0000005);
     `EXCEPTION_RECORD.ExceptionInformation` 为 ULONG\_PTR\[15]\(64 位下 8B/项)
  2. **WOW64 int3 上报码 =** **`STATUS_WX86_BREAKPOINT 0x4000001F`**(单步
     0x4000001E),不是 0x80000003!只认 0x80000003 会把断点当未知异常
     NOT\_HANDLED → 引擎 SEH 二次机会处死(退出码 0x4000001F 曾被误判为反调试)
  3. PEB.BeingDebugged/NtGlobalFlag 需清(ProcessWow64Information 取 PEB32);
     清后不下断点进程存活 → 引擎**无**调试端口检测,自弃由 1. 的误判引起

- 对照实验:--no-arm(挂调试器不下断点)进程存活 25s+;arm 后若 WX86 码
  正确处理,12.3s 采 10 万事件无异常

### 成果 46:首批真值对拍 —— 99,999/100,000 组一致,1 处定性 —— **Confirmed/Unknown**

**对拍**:引擎 100,000 组(12.33s 实时采集,yst00273 启动链起)vs VM 300,012 组
(budget 截断),前缀对齐 100,000 组:

- **✅ 99,999 组 (script, pc, cmd) 完全一致** —— 含全部 GO/GOSUB/RETURN 跨脚本
  切换(23,110 次经重建核验)、IF/ELSE/IFEND、LOOP 族、LET、WAIT、VARINFO、
  引擎执行到 script 156(游戏主循环深处),与 VM 无一错位

- **❌ 唯一分歧(index 23)**:script190 g28 `INT @6292 = @53[2]` → g29
  `IF @6292 > 0`。引擎真(@53\[2]>0),VM 假(@53\[2]=0)。
  **定性:Unknown** —— `@53` 在 YSVR **无条目**(探针 3362/3362 过滤验证),
  亦非对齐前缀内任何已实现命令写入 → 引擎内部维护的系统数组,来源待 P3
  runtime 侦查。分歧后两侧流在 190 pc480 重新汇合(重同步验证)。
  `$55[1]`(VARINFO LENGTH 对象)同族:引擎侧非空、VM 空,同 Unknown。

对拍驱动的 VM 修复(全部 Confirmed):

1. **运行期 INT(0x32) 语义落地**(反编译 `00443538_CMD_INT_runtime.c` +
   窗口形态 + 分歧现象三方一致):w0 = 变量引用,后续窗 = 初值表达式求值写入。
   实证:script190 g28 `INT @6292 = @53[2]`、g30 `INT @6293 = 0`、
   script12 g11 `INT @1749 = 0`。此前 no-op → 条件恒假分歧。
   同族 **0x5c STR** 实测同形(`STR $1748 = $55[1]`,12 g10)且引擎 pc 顺序
   推进;其处理器反编译(0x451598,LAB\_ 边界)形似「按名切脚本」不可靠,
   暂按 no-op 对拍(**Uncertain**);0x19 FLT 处理器 0x4411fc 存在未定性
   (**Unknown**)。
2. **跨脚本 GO 事件归属**:GroupExecuted 须在 switch\_script **前**发
   (归属派发点脚本/pc;原实现 from/from\_script 在切换后捕获 → 事件记到
   目标 pc、ScriptSwitch from==to)。
3. **跨脚本 RETURN 补发 ScriptSwitch**(原缺失 → 事件流重建脚本号错位)。
4. **LOOPEND 回跳路径补发 GroupExecuted**(LOOPBREAK/LOOPCONTINUE 同步补;
   引擎每派发必有一事件,实测 script39 pc118 每次迭代都有)。
5. `event_json` 收编进 lib(`pub fn event_json`,含补齐 Save/CgEnd 序列化;
   golden.rs 预存的 non-exhaustive match 编译错误一并修复),golden 与
   vm\_trace 共用同一 schema。

**测试**:yuris-vm 33/33 全过(golden 2 + vm 30 + boot 1)。
`cargo test --workspace` 被预存环境问题阻塞:`windows-sys` 编译需
`dlltool.exe`(GNU 工具链组件)缺失 —— 与本成果无关,另行处理。

**验收(P1 原定)**:「引擎流 vs Rust 流 diff 为空,或差异逐项定性(记录等级)」
—— **达成**:99,999/100,000 一致,唯一分歧定性为 Unknown(引擎系统态 @53\[2])。

**验证方式(可复现)**:

```text
python scripts/ghidra/hook_trace_setup.py                  # 生成配置(已入库)
python scripts/engine_trace.py --timeout 60 --max-events 100000
cargo run -p yuris-vm --example vm_trace -- "AnimalTrailGirlishSquare 2/pac/bn.ypf" \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl 300000
python scripts/diff_engine_vm.py crates/yuris-vm/tests/golden/engine/engine_trace_boot.jsonl \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl
```

***

## 2026-09-04 — P2 第一梯队:运行期命令补全(成果 47,测试 42/42)

### 成果 47:直方图定序 + 声明族/子系统族落地 —— **Confirmed(形态)/Unknown(单点)**

**定序工具**:`scripts/probe_runtime_hist.py` —— 用 P1 引擎真值 trace 做**运行期**
命令直方图(比静态组表精确),并对照 VM 事件分布输出缺口表。

**直方图实况**(引擎 100k 组):LET 32876 / IF 14202 / IFEND 13480 /
LOOPEND 13089 / VARACT 5377 / 共享 no-op 4603 / VARINFO 4525 / GOSUB 3160 /
RETURN 3154 / INT 1226 / ELSE 1064 / STR 993 / FONTINFO 714 / LOOPCONTINUE 713 /
LOOP 300 / CGINFO 159 / IFBLEND 104 / MATH 83 / SOUND 80 / LOAD 26 / 其余 ≤7。

**落地内容**(每项:反编译引用 + 合成测试 + 真实样本断言,见
`crates/yuris-vm/tests/p2_commands.rs` 9 测试):

1. **运行期声明族 INT(0x32)/FLT(0x19)/STR(0x5c)**:w0 = 变量引用,
   后续窗 = 初值表达式求值写入。反编译引用:`00443538_CMD_INT_runtime.c`、
   `004411fc_CMD_FLT_0x19.c`(两者逐行对称)、STR 同形。
   真实样本:script190 g30 `INT @6293 = 0`、script301 g0
   `FLT @7558 = 1080*100.0/1200 + 0.5`(分辨率自适应布局,实测 = 90.5)、
   script12 g10 `STR $1748 = $55[1]`。
2. **子系统配置/查询族事件化**(新 `VmEvent::Subsystem`,ev=sub):
   0x1b FONTINFO/0x15 FILEINFO/0x14 FILEACT/0x6b WINDOWINFO/0x45 MOUSE/
   0x1a FONT/0x5d SYSTEM/0x0e ERROR/0x1c FPS/0x31 INPUT/0x3c MATH。
   引擎侧均为子系统状态操作(字体层表/文件/输入模式/数学函数派发,
   反编译 `00441c08/0045dd4/0043eb28/0043da80/004583bc/0044955c/00441338/
   0044f968/004699cc/004426a4/00443480_CMD_*.c`),无后端 → 求值全部参数
   事件化记录;**求值失败不传播**(引擎对未声明读取容忍,实测)。
   真实样本:script45 g26 FONTINFO、script190 g342 MATH(@53\[1]/@53\[2] 操作数)。
3. **event\_json JSON 转义修复**:引擎 CG 名含字面引号(`"CGS"100`)→
   Cg/Sound/CgAct/CgInfo/CgEnd/Save 的字符串字段未转义曾产出**非法 JSON 行**
   (P2 对拍实测发现);新增 `json_escape` + `ev_arr`(替换 Debug 格式化 ——
   后者对非 ASCII 产出 `\u{...}` 非 JSON 转义)。
4. 测试夹具迁移:vm.rs 两个 Unsupported 模式测试的夹具 0x0e ERROR(已实现)
   → 0x16 FLASH / 0x3e MENU(仍未逆向)。

**验收进度(P2 原定)**:上表 11 + 3 命令均满足「≥1 反编译引用 + 1 合成测试 +
1 真实样本断言」(真实窗口字节取自引擎 trace 实际执行组)。

**对拍回归**:引擎 100k vs VM 300k,差异仍 = **1 处已知**(index 23,
@53\[2] Unknown);新事件/新语义未引入任何新分歧;VM trace 43.9 万事件
JSON 全合法。缺口表中剩余「VM=0」项均为分歧点之后引擎专属路径的命令
(已实现,VM 自身流未到达;@53 定性后自动解锁)。

**@53\[2] 深挖记录(2026-09-04,未定性)**:硬件写监视点(Dr0,经 Wow64
上下文布防 `scripts/engine_trace.py --watch 53:2`)实测:

- desc\[53] = {cat=0, type=1(INT), dim=1, bounds=\[17], data\@desc+0x30}

- data\[2] 在事件 1-6000 **恒为 0**,零写入命中;但引擎 g28
  `INT @6292 = @53[2]` 的 IF 判定为真 → 引擎对 `@53[2]` 的读取**不经过
  desc+0x30 布局**(或存在别名/重定位路径)

- 对照实证:desc\[6293] 在 VARINFO 后 type=1、data\[0]=8(= $55\[1] 长度)——
  desc+0x30 布局对标量成立,数组读取路径存疑

- `$55[1]` 引擎侧为 **8 字符**串(VM 空)——同样 Unknown

- 结论:引擎系统数组(@53/$55)的存储布局与初始化来源需 P3 runtime
  侦查(候选:FUN\_00463714/004637d8 已反编译 = 建表,非写入者)

**测试**:yuris-vm 42/42(golden 2 + p2\_commands 9 + vm 30 + boot 1)。

### 附记:工具链切换 MSVC —— dlltool 阻断解除(2026-09-04)

- 根因:默认工具链原为 `stable-x86_64-pc-windows-gnu`,编译 `windows-sys`
  需 `dlltool.exe`(MinGW binutils)生成导入库,机器缺失 → workspace 全量
  构建失败(链路:windows-sys ← anstyle-\*/nu-ansi-term ← anstream/
  tracing-subscriber ← yuris-cli/yuris-tools)

- 处置:winget 安装 VS Build Tools 2022(C++ 工作负载 + WinSDK,
  `--installPath D:\VS2022BuildTools`,MSVC 14.44)+
  `rustup default stable-x86_64-pc-windows-msvc`

- 结果:`cargo test --workspace` **39 组全 ok、0 失败、93 测试通过**(含 P3);
  P3 渲染/音频链接 d3d/xaudio 的工具链前置已就绪

***

## 2026-09-04 — P3:Runtime backend 起步 —— trait + Null/Mock + CG 映射(成果 48)

### 成果 48:L4 trait 落地 + mock 空循环跑通 + CG 映射草案 —— **Confirmed(结构)/草案(Likely/Unknown)**

**交付**(每项验收对照见下):

1. **`yuris-scene`(L5 纯数据)**:`Scene { layers, text }` / `Layer { id, z,
   visible, x, y, scale_x/y, alpha, rotation, resource }` / `TextLayout {
   sjis, clear }` / `ResourceId(u64)`;`upsert_layer/hide_layer`(同 id =
   更新语义)。
2. **`yuris-runtime`(L4 trait)**:`RuntimeApi`(graphics/audio/input/storage/
   clock/emit)+ `GraphicsBackend`(begin\_frame/draw\_layer/draw\_text/end\_frame/
   load\_image)+ `AudioBackend` + `InputBackend` + `StorageBackend` +
   `RuntimeEvent`(serde 预留)+ `BackendError`(ResourceNotFound =
   引擎「CG 不存在」语义对齐)。
3. **`NullBackend`/`NullRuntime`**:全空实现;load\_image → ResourceNotFound
   (CgInfo「无后端 ⇒ CG 不存在 ⇒ LET=0」语义对齐,成果 42)。
4. **`MockBackend`/`MockRuntime`**:录制帧/图层/文本/音频,供断言。
5. **`yuris-vm::bridge::SceneBridge`**:VmEvent → Scene 消费桥(增量游标)。
6. **集成测试** `crates/yuris-vm/tests/runtime_loop.rs`:

   - `mock_backend_runs_game_loop`(真实 bn.ypf):Bootstrap → 每帧边界
     (`VmSuspend::Wait`)begin/draw/end + resume → **120 帧空循环跑通**;

   - `cg_event_maps_to_scene_layer`(合成 CG→WAIT→CGEND):图层映射 +
     CGEND 隐藏全链断言;

   - `scene_upsert_and_hide` 回归。

**CG 事件 → 渲染对象映射草案(等级标注)**:

| VmEvent 字段                  | Scene/Layer 落点                     | 等级                                | 依据                                    |
| --------------------------- | ---------------------------------- | --------------------------------- | ------------------------------------- |
| `Cg.id`(槽 0 字符串)            | `Layer.id`/`resource` = FNV-1a(id) | Confirmed(槽=ID)/Likely(哈希派生为实现选择) | YSCM 首参数名 `ID`;真处理器 0x423864:93 字符串读取 |
| `Cg.position` 槽 4/5/6       | `Layer.x/y/z`                      | Likely                            | 引擎槽位消费(00423864 a9/aa 族)+ 数值实测        |
| `Cg.param_count`(其余 \~55 槽) | 渐变/缩放/效果族                          | Unknown                           | 处理器 22942B 未逐段定性                      |
| `CgEnd.id`                  | `Layer.visible = false`            | Likely                            | CGEND 显示结束语义                          |
| `Text.file/let/clear`       | `Scene.text`                       | Likely                            | TEXT 透传,SJIS 解码归字体后端                  |
| `CgInfo` 空结果                | 无后端 ⇒ CG 不存在 ⇒ LET=0               | Confirmed                         | 引擎「不存在」路径(成果 42)                      |

**勘误(引擎处理器归属)**:cmd 0x01 CG = **0x423864**(DAT\_0078b024 表实锤);
0x43c984 = cmd 0x0a **DIALOG** —— PROGRESS 691/1411 行与 lib.rs CG 注释的
旧标注有误,已更正(lib.rs 注释 + 反编译
`00423864_CMD_CG_real_0x01.c`、`0043c984_CMD_DIALOG_0x0a.c` 入库)。
另:YSCM 参数名区为混淆存储(非明文空终止,解析 derail),CG 槽名改以
处理器反编译定位,放弃名表解码。

**测试**:workspace **39 组全 ok、0 失败、93 测试通过**(MSVC 工具链)。

**验证方式(可复现)**:

```text
cargo test -p yuris-vm --test runtime_loop        # 3 测试(mock 空循环 + 映射)
cargo test --workspace                            # 93 全过
```

***

## 2026-09-04 — P4:明文剧本解释器侦查(成果 49)

### 成果 49:scenario 语法全表 + 解释器定位 + 逐行定性 —— **Confirmed(数据侧)/Likely/Unknown(引擎细节)**

**工具**:`scripts/probe_scenario.py`(sc.ypf 解包 → SJIS 解码 → 命令频次/单文件
dump)。**数据侧结论**:sc.ypf 36 个 scenario\*.txt 全部 zlib 压缩、**无加密**,
SJIS 明文直读;32 种 `\命令`;文件名前缀 `# $ - :`(虚拟根前缀族,分组标记)。

**全库频次**(前 16):\LE ×4515、\LT ×4515(34/36 文件,恒成对)、\T ×3507、
\VO ×2376、\BG ×311、\EV ×292、\BGM ×248、\S ×219、\C ×209、\FOUT/\FIN ×141、
\SE ×138、\FACE ×86、\VO2 ×42、\GO ×41(36/36 全文件)、\WINDOWMODE ×27、
\QUAKE ×26;长尾:\EYECATCH/\SP/\FLASH/\SE2/\RP/\VO4/\LOGO/\SEL/\TITLE/\END/
\MOVIE/\ENDROLL。

**语法结构(数据侧,Confirmed)**:

```
#标签                     段/跳转目标(SCENARIO_MAIN、RELEASE/DEBUG/OPMOVIE/TITLE…)
\CMD(arg,arg,...)         逗号参数;空槽跳位(\WA(500,,,1)、\BG(white,200,0,,,,1))
\CMD.MOD(...)             修饰符链(\SP.ANMV/\S.D/\LOGO.VOICE/\GO.G.IF/\BG.CMXYZ)
\GO(标签) / \GO.TITLE     跨文件跳转(\GO(maho2_23a))
\GO.G.IF(1,"==",1,标签)   全局变量条件跳转(G=全局槽、"=="=比较串)
(ID:3481)\LE("英文")\LT("日文")   对话行:行号 id + 双语文(本样本=英化版)
\SEL.GO(标签,…) \SEL("…") 选择肢(目标标签 + 显示文本)
// 行注释  /* */ 块注释   \END 结束
```

**解释器定位(引擎侧,区域级 Confirmed / 精确派发链 Unknown)**:

- 参数解析器 = **FUN\_004fa579**("invalid use of '%s'" @0x58dcd8 唯一 xref;
  调用 FUN\_004cbd5f(0x14/0x1c/0x20/0x30)= token 期待族)——**\~40 个调用者**
  全部落在 **0x4cb000-0x4fb000 = 命令处理器群**

- **FUN\_004cc10c = 宏展开器**(`#`/`##`/`#@` 操作、"not enough actual
  parameters for macro '%s'")→ 引擎在加载期对 scenario 文本做**宏预处理**
  (\SP.ANMV 等修饰链即宏产物)

- 精确命令派发链未定位:二进制无命令名明文表、无 4 字节立即数比较特征
  → 判定 intern-token id 比对(Unknown,后续可经运行期监视点补)

- 反编译入库:`docs/reverse/decompiled/engine/p4_interp/`(38 文件)

**v5xx plaintext 分发判定**:**Likely** —— 本 v555 商业版 scenario 以
zlib(无加密)明文分发 + 引擎内置直译解释器(宏/分词/参数解析全套)。
单样本不能升 Confirmed;YU-RIS 同族引擎大概率同构。

**验收:scenario\_start.txt 全 103 行逐条定性**(启动链情景):

| 行           | 内容                                                                | 定性                                        | 等级                         |
| ----------- | ----------------------------------------------------------------- | ----------------------------------------- | -------------------------- |
| 2           | `#SCENARIO_START_RELEASE`                                         | 段标签;RELEASE/DEBUG/OPMOVIE/TITLE = 构建模式段选择 | Confirmed(结构)/Likely(选择机制) |
| 4           | `\LOGO.NOSAVE`                                                    | LOGO 显示变体(不写存档标记)                         | 存在 Confirmed/细节 Unknown    |
| 6           | `\FOUT(0,0)`                                                      | 淡出(时长,槽)                                  | Likely                     |
| 12          | `\BG(white, 200, 0,,,, 1)`                                        | 背景设定(white=资源,空槽跳位)                       | Likely(CG 族对应)             |
| 14          | `\FIN(500)`                                                       | 淡入(时长)                                    | Likely                     |
| 16          | `\LOGO.VOICE(-600)`                                               | LOGO 变体(语音偏移参数)                           | Unknown 细节                 |
| 20-24       | `/* \SP.ANMV… */`                                                 | 块注释禁用的 SP 序列(宏展开产物)                       | 结构 Confirmed/SP 细节 Unknown |
| 28          | `\S(logo, item/logo_wp, 1100,0,0,199,0,0,167)`                    | 静态 Sprite(name, 路径未引号, 8 数值槽)             | Confirmed(结构)/槽义 Unknown   |
| 30          | `\WA(3000)`                                                       | 等待 ms                                     | Likely(WAIT 对应)            |
| 32          | `\S.D(logo, 800)`                                                 | Sprite 处置(.D=dispose,淡出 800)              | Likely                     |
| 38/42       | `\S(attention,…)` / `\S.D(attention, 800)`                        | 注意事项图显示/处置                                | 同上                         |
| 64          | `\FOUT(800, 0)`                                                   | 淡出                                        | Likely                     |
| 71/75       | `\LOGO.SVOEND` / `\LOGO.FLAG`                                     | LOGO 变体                                   | Unknown 细节                 |
| 77/81/89    | `#SCENARIO_OPMOVIE` / `#SCENARIO_START_DEBUG` / `#SCENARIO_TITLE` | 段标签                                       | Confirmed(结构)              |
| 91          | `\TITLE`                                                          | 标题画面                                      | Confirmed(结构性存在)           |
| 95-96       | `\GO.G.IF(1, "==", 1, SCENARIO_MAIN)`                             | 全局变量条件跳转(G=全局槽、"=="=比较串)                  | Likely                     |
| 100         | `\GO(SCENARIO_MAIN)`                                              | 无条件跳转                                     | Confirmed(数据侧:标签存在+全文件使用)  |
| 103         | `\END`                                                            | 剧本结束                                      | Likely                     |
| 7/9/18/27/… | `//` 与空行                                                          | 注释/空白                                     | Confirmed                  |

**对话行格式(maho2\_22.txt 实证)**:`(ID:3481)\LE("英文")\LT("日文")` ——
本样本为**英化版**(双语行);`(ID:)` 关联语音命名;`\SEL.GO(se01,…)` +
`\SEL("…")` = 选择肢(目标标签 + 文本);`\BGM(,800)` 空首槽 = 保持曲目仅调音量
(槽跳位同族);`\BG.CMXYZ(276,0,-51)` = 相机 XYZ 修饰。文本行 = 裸行,
由行 id 与 VO 语音联动。

**测试**:无代码改动(侦查任务);探针入库 `scripts/probe_scenario.py`。

***

## 2026-09-04 — P5.1:小 id 系统变量家族定性 + mock 循环越过 s190 卡点(成果 50-51,测试 95/95)

### 成果 50:帧局部变量模型(继承自上一会话,补记)

- id<1000 系统变量族的**帧局部路由**:`@53`=INT 局部 / `@54`=FLT 局部 /
  `$55`=STR 局部 / `@60`/`@61`/`$62`=返回值族;GOSUB 实参按 B0 槽号
  (0x00-0x0f→INT / 0x10-0x1f→FLT / 0x20-0x2f→STR)写入帧局部数组。
- VM 落地:`read_var_value`/`write_var_value` 帧局部优先、`seed_frame_locals`
  按 B0 槽播种、VARINFO LENGTH 帧局部读取、RETURN 经虚拟族回写调用者帧。
- 回归:`gosub_str_frame_local_roundtrip`(GOSUB STR 实参 "ES.FIRST" →
  callee $55[1] 读回)。

### 成果 51:@48 = 内层 LOOP 迭代计数 —— VARACT 越界 keystone 关闭 —— **Confirmed**

**驱动**:mock 循环测试在真实脚本 s190(yst00190.ybn) pc=333 报
`VARACT 槽 3 LENGTH=1 越界`(8 字节串 "ES.FIRST" 上 POS=9)。此前疑 $55[1]
播种为空 —— 实测该值**正确**;真因是 POS 表达式 `@6350-@48-@6351+2` 中的
**@48=0**(应为当前 LOOP 迭代)。

**定性证据链**(全反编译,`docs/reverse/decompiled/engine/p5_sysvar/`):

1. `00447ebc_sysvar_read_int.c` case 0x30:@48 读自脚本对象 `obj+0x244`
   嵌套栈**栈顶记录 +0x10**(栈空 = 0)—— 不经变量存储,读即计算。
2. LOOP `00445a00`:压记录时 `rec+0x10 = 1`(迭代从 1 起)。
3. LOOPEND `00445ce8`:counter < count → counter++ 并跳回体顶(rec+4);
   否则弹栈。LOOPBREAK `00445b9c`/LOOPCONTINUE `00445c54`:LV = 回退层数。
4. GOSUB `004428c0` **不压该栈**(仅存栈深到帧 +1,RETURN 恢复)→
   子程序内无循环时 @48 = 调用者当前迭代,跨帧可见。
5. 语料三处独立印证(s190):正向搜索 `POS=@6347+@48`(g317)、反向搜索
   `POS=@6350-@48-@6351+2`(g333,即卡点)、文本折行 `行号=@6312*(@48-1)+1`
   (g119)——三处语义均为「1 基迭代计数」方可成立。
6. 同 switch 顺带定性:@49=恒 999、@50=间接读变量、@56-59=局部槽有效性、
   @63-66=返回值槽有效性、@67=脚本对象+0x348 数组、@70=变量类型查询
   (详见 `docs/engine/command-layer.md` §9c case 表)。

**VM 修复**(不猜,全部按反编译):

- `Evaluator` 增 `loop_counter` 字段,`load_auto`/aload 对
  `@48`(At,48)直返计数(引擎对下标不敏感,case 0x30 不读下标);
  VM 在 `eval_window_condition`/`eval_window_lvalue` 注入
  `loops.last().counter`(无循环 = 0)。
- `GroupVm::read_var_value` 同步 @48 直读(VARINFO/读路径)。
- LOOPBREAK 建模差异(引擎清零记录留栈 + 经 LOOPEND 弹栈 vs VM truncate):
  语料 0 例在 BREAK 与配对 LOOPEND 之间读 @48,可观测面等价(已记录 §9c)。

**验证**:新回归 `sysvar_at48_is_loop_iteration`(顺序/循环外回落 0/嵌套取
内层三断言);mock 循环 120 帧全过(越过 s190 pc=333);全仓 95/95 绿
(MSVC 工具链)。

**P5.1 收口状态**:@53/$55/@60 族(成果 50)+ @48 LOOP 计数(成果 51)+
@49/@50/@56-59/@63-67/@70 语义(case 表)逐项定性 —— 计划风险表
「@53/$55 写入者未明」关闭:小 id 变量**本无独立写入者**,
53/55 = 帧局部/返回值族,48 = 引擎计算型系统变量。

### P5 剩余

- P5.2 长尾命令(0x65 VAR / 0x16 FLASH / 0x3e MENU)按缺口表补全。
- P5.3 golden 基线扩展(依赖 P8 出画面)。

***

## 2026-09-04 — P5.2:基帧模型 + 长尾命令第一批 —— 对拍推进 [2091]→[24592](成果 56-58,测试 96/96)

### 成果 56:基帧 record[0] + RETURN depth 语义 —— **Confirmed**

**驱动**:对拍 [2091](s22 g38 `IF @60[1]`)。VM 顶层脚本**无帧**执行,
g37 GOSUB 的 callee RETURN 弹帧后 `frames.last_mut()=None` → 返回值写入
静默丢弃 → IF 读到 0。而同形的 s13 g2 读者(有调用者帧)一直正确。

**定性证据链**(全反编译,`p5_sysvar/`):

1. GOSUB `004428c0`:恢复点(pc+1/脚本号)写入 `obj+0x144+depth*4` = **record[depth]
   (调用者记录)**;新帧 `obj+0x148+depth*4`(懒分配 0x328B)写实参
   (INT +8 / FLT +0x90 / STR +0x120,槽 1 基)+ 维数计数(+0x2BC/0x2CD/0x2DE)。
2. RETURN `0044b418`:返回值写 `obj+0x140+depth*4` = 读侧基 0x144 的
   **record[depth-1](调用者记录)**(+0x160/+0x1E8/+0x278 值区,
   +0x2EF/0x300/0x311 计数)→ 弹帧 → 按同一记录 +4/+8 恢复。
3. @60 读(case 0x3c)= 当前帧记录 +0x168+idx*8,计数 = +0x2EF —— 与
   RETURN 写位对齐(slot i → rec+0x160+8i,读 rec+0x168+(i-1)*8)。
4. `if (0 < depth)` 不成立 = **基帧上 RETURN → 脚本结束**(FUN_00451724)。
   任务创建即有 record[0](depth 0 的当前帧),顶层脚本的帧局部/返回值
   区都落在它上面。

**VM 修复**:`GroupVm::load` 预置基帧;RETURN `frames.len()<=1` → 脚本
结束,`>1` → 弹帧写调用者帧。删除「LET 帧局部无帧挂起」死分支
(引擎 depth 0 写 record[0],无挂起路径);新增 `frame_locals_mut()`
(测试回放引擎当前帧状态用)。

### 成果 57:变量标签 GOSUB + 全局读越界容忍 + VARACT/VARINFO/FILEINFO 补全 —— **Confirmed**

- **GOSUB 变量标签**([2104] s22 g42 `var($1909)`="ES.FIRST.LOOP"):引擎
  004428c0 对 B0=0 窗**按表达式求值** → murmur2 查表(载入期预解析缓存
  仅是加速);VM `resolve_label_in` 从「只认 M-串字面量」改为求值。
- **全局数组读越界容忍**([2104]续 s37 g284):`FUN_00459490/00459418`
  实锤 —— `idx<0 || idx>=count → 返回 0/0.0`,**不报错**(旧注释
  「全局数组仍严格报错」被证伪)。aload/load_auto/read_var_value 三处
  越界 → 类型默认;未声明变量仍报错。
- **VARACT UPPER(7)/LOWER(9)**(s190 g267):`FUN_004546cc/FUN_00464dac`
  (各 57B)= SJIS 感知 ASCII 大小写原地转换(双字节首区连跳 2);
  槽值(push 1)= 使能标志不消费(Likely)。UPPER2/LOWER2 = CRT locale
  表驱动全角转换,语料未用,不实现。
- **VARACT DIMSIZE + VARINFO DIMSIZE/DIMNUM 的裸引用**(s37 g50/g51):
  `76 $2459` 无 aload = **整个数组**引用;引擎按 id 取 desc 直接
  resize/查维数。`FUN_00454710`:desc+4 = **元素个数**(分配 n*8、
  拷 min(old,new))—— 与声明「上界引用」区分。
- **FILEINFO(0x15) EXIST**(s207 es.R18Check):`0043eb28` —
  FILE 分支 → `FUN_0043f5f0`(VFS 挂载表解析;7 挂载项 + 6 前缀松散探测,
  R18/语言标志影响挂载顺序)。VM 实现 `YpfIndex`(yuris-format,仅索引
  区 seek 解析,cg.ypf 级大包不整载)+ `PacFileIndex`(扫 pac\*.ypf
  名字集 + 松散根;头加密包如 op.ypf 跳过记录)。

### 成果 58:oracle 环境复现 —— R18Check 文件分歧定性 —— **Confirmed(分歧归因)**

对拍 [19112](s279 g34 `IF @7391==1`):`@7391 = @60[1]` = es.R18Check
(s207)= FILEINFO EXIST `cg/thumb_cg/A_HAN_2002_a.png`。引擎 trace 时刻
该文件**存在于真实 FS**(引擎 FindFirstFileA/VFS 命中 → 1 → R18 流);
现安装包内无此文件(a_han CG 仅 2001/1005 系,无 2002)= 采集后清理的
解包残留。**输入环境差异,非语义 bug**。处置:`PacFileIndex::add_virtual`
复现 oracle 输入(仅存在性判定,内容从不读取);vm_trace 与 mock 循环
测试注入。对拍推进 [2091]→[2104]→[19112]→**[24592]**。

### 剩余分歧 [24592](下一项)

s47(按钮执行)在 `$2729`(标签表)中搜 `$2753`(="backlog",来自
s47 g26 GOSUB 返回值 `$62[1]`)不中 → 错误对话框 → 脚本链提前完结。
引擎同点命中(搜索循环 g30 `IF $2753 == $2729[@48]` 为真)→ `$2729`
内容与引擎不一致 —— 需追注册侧写入(注册链:标题脚本按钮注册 GOSUB)。

### 工具链

- pyghidra 反编译:`scripts/ghidra/p5_varact_case.py`(大小写族)、
  `p5_varact_resize.py`(resize/VARINFO)、`p5_global_read.py`(全局读)、
  `p5_fileinfo.py`、`p5_vfs_exists.py`、`p5_vfs_archive.py`(VFS)、
  `p5_decl_int.py`(声明运行期)。YpfIndex 的字节级名字兼容 SJIS 包;
  头加密包(op.ypf/op_c.ypf,magic 非 `YPF\0`)跳过记录。

### 成果 59:按钮链收口 —— 对拍零分歧至 [36588],VM 独立推进 126410 组

**[24592] 根因**(s47 按钮表错位一槽):

1. **VARACT DIMSIZE 复合算符**(s47 g42 `DIMSIZE(+=1)` = 压栈惯用法):
   引擎 00453178 DIMSIZE 分支调用 FUN_00425224(全文件唯一调用点;
   op=B3:0=赋值/1=加/2=减/3=乘/4=除)**以当前元素个数为基**;
   负值报错;结果==cur 不 resize。VM 原按绝对值 resize → 表永不增长。
2. **YSVR kind2 覆盖时机**:`$2729` YSVR bounds=[1](probe_ysvr_query 实证),
   引擎侧压栈从 [1] 起、搜索 "backlog" 于 [9] 命中;VM 声明期 [expr+1]=[2]
   未被 YSVR 覆盖 → 压栈从 [2] 起、[10] 才命中 → 4 次搜索全部错位一槽。
   根因 = boot 全量消费声明后**未标记**,switch_script 重消费把声明期边界
   刷回,覆盖 YSVR 终态。修:boot 标记 `declared_scripts`。

**连带修复**(追链过程中逐一定性):

- **GOSUB 标签字节保真**([29208] s147 g788):标签窗求值结果须保留原始
  字节(YSLB 键 = SJIS;`str_as_string()` 的 UTF-8 lossy 把
  `es.日本語…` 变 U+FFFD → 查表 miss)。
- **@114 = 显示色深**(s40 色深检查):sysvar case 0x72 = 原生全局
  DAT_00872374(bpp);VM 无显示栈 → 计算型系统变量返 32(oracle 环境)。
- **WINDOW(0x69) = 共享 no-op stub**(0045c4d4):引擎 trace 被过滤为
  伪事件 → VM 完全静默(发事件破坏对拍域)。
- **LOAD 引用窗**:槽 3 目标 = 0x76/0x56/0x48 延迟引用,不可按条件窗求值
  (s35 g705 实证);引用窗跳过求值记 "(ref)"。
- **数组写类型收敛**:引擎按描述符类型转存(FLT 数组收 Int → f64;
  INT 收 Float → round)—— VM 的严格报错被证伪(VariableStore::set_elem)。
- **YSVR kind 消费时机**(FUN_00451348 反编译):boot(-1) 应用 **kind 1+3**;
  kind 2 在脚本**首次加载**时应用(`apply_ysvr_for_script` + switch_script
  钩子)。@5256(kind3, FLT[101,5])实证 kind3 必须应用(旧「不猜跳过」
  被证伪);否则 s177 2D 访问踩 declared-1。
- **VFS save 根**:s25/s35 的 `*.sd` 存在性查询命中 `save/`(引擎 VFS
  原生前缀探测;`$1045` 引擎侧亦为空串 = $105,不种子)。

**当前对拍态**(成果 60 收官):对齐区间扩至引擎全量 **95397/95397 组零分歧**
(旧 [0,36588) 的 5 处分歧全部关闭);VM 独立推进 129245 组,停于
s190 pc472 TASKINFO(0x61)—— 与成果 59 时点相同的下一卡点(任务系统查询,
需任务模型定性)。

***

## 2026-09-05 — P5.2:LOAD/YSSD 装载 + 子系统查询输出 —— 对拍零分歧(成果 60)

### 成果 60:YSSD/SNP 全格式定性 + LOAD 落地 + WINDOWINFO/FONTINFO 输出 —— **Confirmed**

**驱动**:成果 59 收官时残余 5 处分歧 + 停点 s190 g472 全部归于
LOAD(0x36/YSSD)未实现。本成果闭环:格式逆向 → 装载实现 → 输出族补全 →
**引擎全量 trace(100k 事件/95397 组)对拍零分歧**。

#### 1. YSSD 格式 —— **Confirmed**(样本 = save/ 六文件 28 块全解码)

```
[Header 0x1010B]  magic "YSSD" + ver u32(0x1E0) + 块数 u32
                  + 偏移表容量 u32(0x400) + 偏移表 0x400×u32
                  (表项[i] = 块号 i+1 的绝对偏移;0 = 空)
[Block]           u32 块号(=DNO,1 基) + u8 类型(0=普通/1=0x690 配置/
                  2=多子块,语料仅 0) + u8 严格维数标志 + u16 目标变量 id
                  + u32 压缩长 + u32 未压长 + SNP 载荷
[载荷(解压后)]     u32 类型(1=INT 2=FLT 3=STR) + u32 维数 + u32[维数] 边界
                  + u32 数据长 + 数据(INT/FLT = 8B/元素;STR = 逐元素
                  {u32 长度 + 字节},元素数 = 维数积)
```

块头 var_id 与脚本槽 3 引用恒一致(26 组实证);数据侧佐证:config.sd
块 1 = @1174 INT(256,256) 524288B,块 2 = $1173 STR(256);seld.sd =
@2337 INT(2001)(**勘误**:成果 59 所记「seld.sd → @1174」实为 config.sd;
@1174 来自 config.sd 块 1,seld.sd 装载的是 @2337)。

#### 2. SNP 编码 = snappy 变体 —— **Confirmed**(YSSNP.DLL 全逆向)

- 压缩器指针 `DAT_00872504 = YSSNP.DLL!YSSnp_Uncompress`(初始化
  FUN_00467488:LoadLibraryA + GetProcAddress 实锤;`DAT_008724d8` =
  YSZLB.DLL 序数 #1 = zlib)。本构建 `DAT_0059b718 != 0` → .sd 全走 SNP
  (载荷 zlib 头校验失败、SNP 全量闭合反推)。
- 解压主体 `FUN_10001d40` + 派发表 `DAT_1000ba40`(u16[256])/
  `DAT_1000ba2c`(u32 掩码):头部 varint 未压长度(7-bit LE base-128);
  元素 tag 低 2 位 00=字面量 / 01=copy1 / 10=copy2 / 11=copy4;
  **字面量长度 = (tag>>2)+1**(非标准 snappy 的 tag>>2 —— 本格式最小
  字面量长度 1,无空字面量);长字面量(tag>>2 ∈ 60..63)后随 1..4 字节
  小端长度(同样 +1);copy 族与标准 snappy 完全一致(copy1 len=4+((tag>>2)&7)、
  offset=((tag>>5)<<8)|下一字节;copy2/4 offset 为 u16/u32 LE)。
- STR 块解码字节级验证:config.sd 块 2 的 256 串中 [11] = 游戏安装路径
  (真实值,非巧合)。

#### 3. LOAD(0x36)装载实现 —— **Confirmed**(引擎 FUN_00444648 + FUN_0044564d)

- 槽位:0=FILE(求值;无扩展名 → 补 `.sd`)、1=MEM(语料未出现 → 显式
  Unsupported)、2=DNO(1 基 → 偏移表)、3=写回目标(**延迟引用**,左值
  解析不求值)、4=ID 模式(0x690 配置块,语料未出现 → 显式 Unsupported)。
- 写回校验:类型一致(引擎 0x18ee8)+ strict 维数精确匹配(0x1a63a)+
  非 strict 须标量(0x1a630)+ 文件/块缺失(0x18ed4/0x18ede)。
- INT 数组元素 = **8B/元素 i64**(engine desc+0x30 区 memcpy;FLT 读路径
  `FUN_00459418` = f64@idx*8 交叉验证)。
- 交付:`yuris_format::yssd`(YssdFile/YssdBlock/YssdPayload/snp_uncompress)、
  `host::PacFileIndex::read_loose`(save/ VFS 根松散读)、
  `yuris_value::load_array_data`(整块写回)、lib.rs `load_yssd`。

#### 4. 子系统查询输出族(对拍收口必要条件)—— **Confirmed(结构)/oracle 常量**

引擎 FUN_0045baf8 族「查询 → LET 槽写回」(VM 原为纯事件化):

| 命令 | 槽位 | 查询 | VM 实现 | 等级 |
| --- | --- | --- | --- | --- |
| WINDOWINFO(0x6b) | SX(槽10) | 窗口宽 → LET(槽1) | 1920 | oracle(引擎 trace 时刻全屏;s41 g77 范围检查 ±32 反推) |
| WINDOWINFO(0x6b) | SY(槽11) | 窗口高 → LET(槽1) | 1080 | 同上(±18 反推) |
| FONTINFO(0x1b) | NUM(槽11,值=1) | 注册字体数 → LET(槽18) | 713 | oracle(引擎 s45 循环圈数 713 实测) |
| FONTINFO(0x1b) | LANG(槽13,值=1) | 字体语言 → LET(槽18) | 1 | oracle(713 字体全部非零 → LOOPCONT×713) |

另:@115/@116(sysvar case 0x73/0x74,DAT_0087236c/70)= 屏幕宽/高 →
1920/1080(oracle;同 @114=32bpp 先例)。
**注意**:以上 oracle 常量随引擎 trace 采集环境变化(分辨率/字体注册数),
对拍重采集时须同步校准。

#### 5. 对拍终态 —— **Confirmed**

- 引擎 95397 组(100k 事件全量)vs VM:**前缀逐元素零分歧**;
- VM 独立推进 129245 组(无后端预算内),停于 s190 pc472
  TASKINFO(0x61)未实现(预期卡点,成果 59 同点);
- 过程记录:LOAD 落地后首拍 4 处分歧(s41/s45/s177×2),其中 s177 两处
  为 diff 工具 RSYNC 重同步伪影(两侧 pc 计数逐项一致),真实分歧 2 处
  = WINDOWINFO/FONTINFO 输出缺失;补全后 s41 余 1 处 → @115 屏宽;全补后
  归零。diff 工具的语义事件双计入(group + load 伪组)在重同步后自愈,
  未修工具(对拍域不变)。

**测试**:yuris-format `tests/yssd.rs` 10 测(SNP 五形态 + 载荷头 + 六文件
全块结构 + config 数值抽样);yuris-vm `tests/p5_load.rs` 6 测(INT/STR/
标量装载 + strict/类型/缺文件三错误路径);`p2_commands.rs` 的
subsystem_fontinfo_real_sample 改新语义(NUM 写回 713)。全仓
workspace 测试全绿(MSVC 工具链)。

**验证方式(可复现)**:

```text
python scripts/probe_yssd.py                        # save/*.sd 全块解码(SNP 变体)
cargo test -p yuris-format --test yssd              # 格式 + 真实样本断言
cargo test -p yuris-vm --test p5_load               # 装载链路(合成 + 错误路径)
cargo run --release -p yuris-vm --example vm_trace -- \
    "AnimalTrailGirlishSquare 2/pac/bn.ypf" \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl 300000
python scripts/diff_engine_vm.py \
    crates/yuris-vm/tests/golden/engine/engine_trace_boot.jsonl \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl
# → ✅ 对齐区间内 diff 为空(引擎 95397 vs VM 129245;VM 停于 s190 pc472 TASKINFO)
#   注:2026-09-05 基线已换代(400k trace,见成果 61);本验证的 100k 引擎
#   trace 已被覆盖,复现须按成果 61 流程重采。
```

### P5 剩余(更新)

- **成果 62 已收口 s9 十处分歧**(CG 状态注册表落地;残余 4 处单根因
  = 引擎帧驱动 CG 颜色动画,已定性归 P8 效果族,详见成果 62)。
- P5.2 长尾命令(0x65 VAR / 0x16 FLASH / 0x3e MENU 等)按缺口表补全;
  FONTINFO 其余查询(NAME/SX/SY/COLOR/COLOR2)与 WINDOWINFO 其余查询
  (X/Y/WSX/WSY/EXIST/ACTIVE/FULLSCREEN 族)语料各 1-10 组,到访时补;
  TASKINFO 其余查询槽(FILE/FILENO/LINE/TEXTLINE/NEXTVOICE/GOLABEL/
  GOSUBLABEL/IFNEST/GOSUBNEST/LOOPNEST/SCRIPTPOS/TEXTPOS)语料未执行,
  到访时逐项逆向(引擎 FUN_0045baf8 族)。
- P5.3 golden 基线扩展(依赖 P8 出画面)。
- YSSD SAVE 方向(0x56 写 .sd)归 P9 存档系统(SNP 编码器届时落地)。

***

## 2026-09-05 — P8.1/P8.2 + P6 勘误:wgpu 渲染后端 + YpfIndex 判别修正(成果 65,进行中)

### 成果 65:WgpuBackend 落地 + 双勘误 —— **代码交付/验收未完(见交接状态)**

#### 1. P8.1/P8.2 交付(yuris-render 空壳 → 落地)

- **`WgpuBackend`**(winit 0.30 + wgpu 24):
  - 窗口/surface/adapter/device 初始化(`Arc<Window>` 持有,event loop 走
    `resumed()`/`ApplicationHandler` 0.30 模型);
  - **图层合成**(P8.2):引擎逻辑分辨率 1920×1080(WINDOWINFO SX/SY
    oracle,成果 60)→ 视口 letterbox 等比缩放居中;Layer 的
    x/y/scale_x/scale_y/alpha/z(升序绘制;预乘 alpha 混合);WGSL
    shader + 动态 uniform(256B 对齐,MAX_LAYERS=1024);
  - `load_image`:PNG(image crate 直读,成果 63)→ wgpu 纹理
    (Rgba8UnormSrgb);`image_size` 查询 API;
  - `draw_text`:P8.3 待实现(SJIS 字体),记录后跳过;
  - `resize`(surface 重配置)。
- **`examples/first_pixel.rs`**(M1 首像素演示):挂载 cg.ypf →
  seek 读 `cg\eyecatch\st\logo.png` + `cg\item\attention_1.png`
  (scenario_start.txt 启动序列的 LOGO/注意事项图)→ 时间轴驱动
  Scene(logo 0.5s 淡入 → 3s 后切注意事项图全屏)→ wgpu 合成。
  **lib + example 均编译通过;未做窗口实测**(见交接状态)。
- workspace 新增依赖:wgpu 24 / winit 0.30 / pollster 0.4 /
  bytemuck;残留:wgpu 24 deprecation 警告 2 处(ImageCopyTexture/
  ImageDataLayout → TexelCopy*;25.0 移除,升级前改)。

#### 2. 勘误 A:成果 63 的 YpfIndex flag 判别式错误 —— **已修**

- **错**:按「名字尾字节 < 0x20」判 se 型 —— 但类型码是 **XOR 后**
  的值(PNG=0xCB、OGG=0xCF,均 ≥ 0x20)→ cg.ypf 全部条目被误判为
  bn 型,flags 全为垃圾(dbg_index 实测 flags[:10]=[125,220,...])。
- **修正**(probe_census.py 全包普查实证,复合判别):
  1. 名字尾(XOR 后)∈ {0xCB=PNG, 0xCF=OGG} → se 型(剥尾码);
  2. byte@NUL ∈ {0,1} 且 bn 解释的 `off@np+9` 落界
     (`first_data_off ≤ off ≤ file_len-comp`)→ bn 型;
  3. 否则按 se 处理不剥尾(se.ypf 4 个 2 字节怪条目,Unknown)。
- **修正后实测**:cg.ypf 首条 uncomp=comp=15485, off=0x233edc5e
  (与 probe 逐字段一致);flags 全 0xCB。
- **结构终型**(probe_entry_prefix.py):名字字段 = **虚拟根字节(1B)
  + 路径 + [类型码(1B,仅 se 型)] + NUL**。虚拟根字节各包不同且
  同包内不恒定(bn:`$`/`%`/`9`;cg:`"`/`\x11`/`:`;vo:`#`/`+`;…),
  成果 63「怪前缀是解析伪影」的表述**错误**——它们是名字段第一字节。
- **双索引**:`map` 同时登记带根全名(`$ysbin\...`)与剥根名
  (`ysbin\...`)—— 引擎侧两种查询形态(YpfScriptHost 用带根;
  FILEINFO/资源读取用剥根)。

#### 3. 勘误 B:P6 样本测试静默跳过 —— **已修**

- **错**:`p6_archives.rs` / `p6_read.rs` 用相对路径
  `"AnimalTrailGirlishSquare 2"`,而 **cargo test 的 CWD = crate 根**
  → `pac_dir()` 恒 None → 断言全部静默跳过(6/6「通过」是假绿,
  成果 63/64 的样本断言从未真正执行!)。
- **修正**:`CARGO_MANIFEST_DIR` 上溯 5 级定位工作区根 +
  `YURIS_SAMPLE_PAC` 环境变量覆盖;修后测试真跑,暴露勘误 A。
- **影响面**:成果 63/64 的测试断言已按真实语义重写
  (flag=0xCB/0xCF、剥根名查询、首条字段精确值);**yuris-resource
  的 p6_read.rs 已同步修 CWD,但其断言尚未在修后状态跑过**
  (被 workspace 构建阻断,见交接状态)。

#### 4. 交接状态(⚠ 下一位接手者必读)

- **workspace 当前不可构建**:`crates/yuris-scenario`(P7.2)由并行
  子代理开发中——Cargo.toml 已挂进 workspace members 但 src/lib.rs
  尚未落盘(`cargo` 报 "no targets specified in the manifest")。
  两条路:(a) 等子代理产物就位(若会话已中断,检查该 crate 是否有
  半成品,补 `src/lib.rs` 或先从 members 移除以恢复构建);
  (b) 恢复构建后立即跑:
  ```text
  cargo test -p yuris-format --test p6_archives   # 勘误后断言(未验证!)
  cargo test -p yuris-resource                    # CWD 修后(未验证!)
  cargo build -p yuris-render --example first_pixel
  cargo run --release -p yuris-render --example first_pixel  # M1 首像素实测
  ```
- **first_pixel 实测要点**:窗口弹出 → logo 淡入 3s → 注意事项图;
  Esc 退出。若黑屏排查:(1) `Layer.id` 与 `load_image` 的
  `ResourceId` 一致性;(2) letterbox 矩阵(`logical_to_ndc` 的
  Y 翻转/偏移);(3) wgpu surface format 是否 sRGB。
- **P7.2 子代理已产出的语法事实**(其侦查输出,未见入库,接手时
  若丢失需重做):引号内逗号 2741 次(参数分割必须引号感知);
  引号字符串内的反斜杠是 Big5/SJIS 双字节尾字节(解码后消失);
  纯 CRLF 行尾;行内括号平衡;块注释内无引号;无转义字符、引号平衡。
- **P8 后续**:P8.3 SJIS 文本渲染(字体选型 + YSTCH.DLL 对照)、
  效果族(淡入淡出/QUAKE/RECTPAINT3 动画 = 对拍残余 4 处分歧根因)、
  与 VM SceneBridge 的真实对接(mock 后端 → WgpuBackend)。

***

## 2026-09-05 — P6.3:多包内容读取 API + PNG/OGG 解码链路(成果 64)

### 成果 64:YpfReader(seek 读取)+ ResourceStack(多包优先级)+ 解码直读 —— **Confirmed(链路)/Likely(包间顺序)**

#### 1. 交付

- **`yuris_format::ypf::YpfReader`**:File + 索引 seek 式随机读取 ——
  cg.ypf(827MB)不整载,内存只留索引;条目按 flag 解压(flag=1 → zlib,
  se 型类型码 0x02/0x06 → stored 原样)。`YpfIndex` 扩展条目级
  `(name, flag, uncomp, comp, offset)` + 名字→下标 map。
- **`yuris_resource::ResourceStack`**(空壳 → 落地):
  - `mount_game_dir(dir)`:游戏根+pac 全部 ypf 按文件名排序挂载
    (update\* 自然最后);op.ypf/op_c.ypf(ASF/WMV)BadMagic 跳过不阻断;
    松散根 = 游戏根/pac/save(引擎 VFS 原生前缀,成果 59e)。
  - `read(path)`:**松散根优先**(FILEPRIORITY,Confirmed)→ 封包
    LIFO(**后挂载优先,Likely** —— 更新包覆盖工程默认;引擎真值
    Unknown,留 P8 截图对拍校准,API 显式可控);`/`-`\` 归一。
  - `read_image`:image crate 直读(**标准 PNG**,成果 63);
    `read_audio_header`:symphonia probe(OGG → 声道/采样率)。
  - `ResourceError::NotFound` 对齐 `BackendError::ResourceNotFound`
    (引擎「CG 不存在」语义,成果 42)。
- workspace 新增依赖:image 0.25 / symphonia 0.5(ogg+vorbis)。

#### 2. 验收(原 P6.3 定义:「挂载→读取→解码」全链路成功)

`yuris-resource/tests/p6_read.rs` 6 测全过(真实样本):

| 测试 | 链路 |
| --- | --- |
| cg_png_decode_chain | mount cg.ypf → seek 读 PNG 条目 → image 解码(尺寸>0)+ 正斜杠 exists 归一 |
| bgm_ogg_probe_chain | mount bgm.ypf → symphonia probe(2ch,≥44100Hz) |
| sc_zlib_text_read | mount sc.ypf → zlib 解压 → #SCENARIO 段标签断言(成果 49 复核) |
| update1_overrides_cg_on_same_name | 同名条目 LIFO = update1 数据(逐字节相等) |
| mount_game_dir_skips_asf | 全包挂载 + op.ypf 跳过 + PNG 链路仍通 |
| not_found_semantics | ResourceNotFound 路径 |

「与引擎运行截图对照」归 P8 出画面(渲染管线就绪后逐 CG 对拍);
「缺资源走 ResourceNotFound」已落(CGINFO「不存在」路径语义对齐)。
回归:workspace 45 组全绿(含 P6/P7 新增);对拍 4 处分歧不变(零退化)。

**验证方式(可复现)**:

```text
cargo test -p yuris-resource          # 6 测(挂载→seek→解码全链路)
cargo test --workspace                # 45 组全绿
```

### P6 剩余(更新)

- **P6.5(原 P6.4)引擎挂载顺序定性**:运行期 watch 挂载表构造
  (FUN_0043f5f0 上游)或 P8 截图对拍同名条目(当前 API 显式可控,
  LIFO 为 Likely 工程默认)。
- ymv(mv001-004)容器定性(M5 OP 视频前;op=ASF 已定性)。

***

## 2026-09-05 — P6.1/P6.2:资源层侦查 —— 条目布局 + 魔数全解 + 多包挂载(成果 63)

### 成果 63:YPF 双布局条目级自适应 + 资源明文实证 + op=ASF —— **Confirmed(全样本)**

**工具**:`scripts/probe_pac_magic.py`(首版,布局错位发现)、
`probe_pac_layout.py` / `probe_pac_layout2.py`(双模板假说验证)、
`probe_pac_magic2.py`(修正版全量统计)、`probe_update1_hex.py`
(混合包定性);Rust 落地 `yuris_format::ypf::YpfIndex`。

#### 1. YPF 条目双布局(条目级,同包并存)—— **Confirmed**

首版魔数侦查发现 se/cg 系包「名字带首尾残差 + offset 错位」,经双模板
假说验证与 update1 索引区 hexdump 定性:

| | bn 型(脚本/文本) | se 型(资源) |
| --- | --- | --- |
| 布局 | `name NUL flag uncomp comp off zero tail8` | `name flag NUL uncomp comp off zero tail8` |
| flag 位置 | NUL **后** | 紧贴名字尾(rcs 吞进名字尾缀) |
| flag 值 | 0=stored / 1=zlib | **内容类型码**:0x02=PNG / 0x06=OGG |
| 数据 | zlib(78 da)/原始 | **明文 stored** |
| 样本 | bn 全部 / sc 全部 / update1 的 txt | se/cg/bgm/vo/cgsys/sysvo/sysse + update1 的 ogg/png |

- **两种条目总开销均为 len+26**(flag 位置互补)→ 全包索引闭合不变;
- **同一包内并存**(update1.ypf:zlib txt×18 + PNG×194 + OGG×1094);
- 判别式:se 型名字尾字节为控制码(<0x20,合法路径名不含);实证零误判。
- 勘误线索:旧「虚拟根前缀 `$`/`%`/`9`」之外的怪前缀(`\x17`/`/`/`"`
  等)实为**上一条目 tail 后的解析位移伪影** —— 名字规范化后消失。

#### 2. 资源内容格式 —— **Confirmed(魔数实证)**

| 包 | 条目 | 数据头 |
| --- | --- | --- |
| cg.ypf / cgsys_ec.ypf | 5655 / 4708 | **标准 PNG**(`\x89PNG\r\n\x1a\n…IHDR`,明文 stored) |
| bgm/se/sysse/sysvo/vo | 32/742/6/275/2715 | **OggS**(明文 stored) |
| update1.ypf | 1306 | 混合(OGG×1094 + PNG×194 + zlib txt×18) |
| bn/sc | 309/36 | zlib(YSTB/YSER/YSCF…脚本族) |

**P8/M1 直接利好:图像资源 = 标准 PNG,无需 YSPNG.DLL 逆向,image
crate 直读**;音频 = 标准 OGG(symphonia 直读)。YSPNG/YSWBP/YSTCH
推测为引擎自身解码器(兼容层),非专有格式封装。

#### 3. op.ypf / op_c.ypf = ASF/WMV —— **Confirmed**

头 16 字节 = ASF header GUID(`30 26 B2 75 8E 66 CF 11 A6 D9 00 AA
00 62 CE 6C`)+ 后续 ASF 结构 —— **非加密 YPF,是 Windows Media 视频**
(125MB OP 视频,op_c = 英化版字幕轨?)。M5 OP 播放走 ASF 解复用,
ymv(mv001-004)另行定性(P6.3)。

#### 4. P6.1 多包挂载 —— **机制 Confirmed / 包间顺序 Unknown**

- **免封包优先级**:FILEPRIORITY\*(YSCM 键族)+ 免封包文章 = 松散文件
  优先于封包(Confirmed,成果 59e 已用)。
- **包间同名冲突实证**:update1.ypf 与 cg.ypf 的 `cg\ev\*` 族同名条目
  ≥100(测试 `multi_archive_name_overlap_update1_overrides_cg` 定量)
  —— 更新包覆盖基础包的资源面。**引擎挂载顺序 Unknown**(FILEINFO
  EXIST 对同名不敏感,无法从对拍取真值;读取语义留待 P6.3 内容读取
  + 截图对拍,禁止猜)。
- `PacFileIndex::scan_game_dir` 经新 `YpfIndex` 自动获益(名字规范化);
  op/op_c 走 BadMagic → skipped 记录(既有路径)。

#### 5. Rust 交付

- [`YpfIndex`](格式层):条目级自适应布局 + 名字规范化(se 型剥尾缀)
  + `flags` 输出(bn=压缩标记/se=类型码)。
- 测试 `yuris-format/tests/p6_archives.rs` 6 测:bn/se 布局 + update1
  混合精确定量(18/194/1094)+ **cg.ypf 数据头 = 标准 PNG**(seek 实读
  断言)+ op BadMagic + 跨包同名冲突 ≥100。
- 回归:workspace 全绿;对拍 4 处分歧不变(P6 改动零退化)。

**验证方式(可复现)**:

```text
python scripts/probe_pac_magic2.py                    # 全包模板探测 + 魔数统计
python scripts/probe_update1_hex.py                   # 混合包布局 hexdump
cargo test -p yuris-format --test p6_archives          # 6 测(真实样本断言)
cargo run --release -p yuris-vm --example vm_trace -- \
    "AnimalTrailGirlishSquare 2/pac/bn.ypf" <输出> 600000
python scripts/diff_engine_vm.py <引擎trace> <vm输出>  # → 4 处(不退化)
```

### P6 剩余(更新)

- **P6.3 解码落地**:多包内容读取 API(优先级语义)+ image crate(PNG)
  / symphonia(OGG)直读;ymv/ASF 解复用(OP 视频,M5 前);
  PSB/WEBP 实测未出现(全语料 PNG/OGG)—— 风险表降级。
- **P6.4 引擎挂载顺序定性**(可选):运行期 watch 挂载表构造
  (FUN_0043f5f0 上游)或截图对拍同名条目。

***

## 2026-09-05 — P7.1:CG 状态注册表 + CGINFO 查询落地 —— s9 十处分歧收口(成果 62)

### 成果 62:CG 注册表模型 + FILE 门控创建 + CGINFO SX/SY/COLOR —— 对拍 [137544]→[294329],残余 4 处单根因定性 —— **Confirmed(实证)/Likely(默认值一般化)/Unknown(动画)**

**驱动**:成果 61 收官时 s9 十处对拍分歧(pc1093/pc628/pc233 族)。
经 flow 级并排对比(`scripts/probe_s9_flow.py`)证明 **10 处全部同根**:
首个真分歧 = [137545] s9 g1092 IF,其余为 diff 工具 RSYNC 重同步伪影
(pc230/pc628 族两侧逐 run 一致)。

#### 1. 分歧根因定性 —— **Confirmed**(窗口 dump + 引擎 watch oracle)

- **g1088 CG 装载** `$1710`(按钮纹理,名如
  `ES.GAMEMAIN.TIP.MESWM\x03\x00"."BT.OFF=0=1`)→ g1089-91 **CGINFO**
  查询 SX(槽13)/SY(槽14)/COLOR(槽24)写回 @1705/@1706/@1707 →
  g1092 IF `@1705==1 && @1706==1 && @1707==8421504(0x808080)`:
  引擎**判真**(执行 g1093 LET @1301[@1704]=256 + g1094 CGACT),
  VM 无 CG 状态恒假 → 分歧。
- **watch oracle 扩展**(`engine_trace.py` 监视点命中时打印
  i64/f64 值 + 当前 script/pc;`--watch 1707:0`):@1707(COLOR)写入值
  逐次实证 —— BT.OFF(pc1091)=8421504.0(找到路径,writer 0x45bb6b,
  栈上立即数 0x808080);BT.OVER/ON/ONOV/MASK(pc1156/1213/1258/1346)
  =0.0(**未找到路径**,writer 0x45469e=FLT 接收器写清零结果缓冲
  DAT_005c0840);BT.NA(pc1315)=8421504.0。**两次独立运行逐值一致**
  (确定性,帧驱动非墙钟)。

#### 2. CG 状态注册表落地 —— **Confirmed(创建门控)/Likely(默认值)**

VM 新增 `cg_registry: HashMap<名(SJIS 字节), CgState>`(x/y/z/sx/sy/color):

- **CG(0x01)注册门控**:`FILE` 槽(46)求值**非空字符串 → 创建/注册**
  (引擎 0x423864:`DAT_006624ce != 0 → FUN_00466ff1` 创建哈希节点);
  **无 FILE / FILE="" → 静默不创建**(处理器 `return 0` 路径),已注册者
  仅按已指定槽 patch。实证:s9 g1072 CG(BT.OFF, FILE=$1227[@1704])
  → 可查;BT.OVER 族仅 CG(FILE="")(空串字面量 `4d 02 00 22 22` 解码
  实证)+ CGACT COPY2 → 不可查。
- **默认值**:SX=1 / SY=1 / COLOR=0x808080(新建未指定槽)。
  等级:值本身 Confirmed(watch oracle 分支实证);「新建 ⇒ 默认」的
  一般化 Likely(构造器链 0x423864→0x466ff1 为哈希/链表插入,初始化区
  未定位;boot 语料全部新建 CG 均未携带 SX/SY/COLOR 槽,与实证一致)。
- **CGINFO(0x04)查询应答**:注册名 → EXIST(槽2)=1 / X(5)/Y(6)/
  SX(13)/SY(14)/COLOR(24) = 状态字段;未注册名 → **全查询写 0**
  (引擎「不存在」路径 Confirmed);未建模查询(ONMOUSE/TRIM/LINT 族,
  语料 ONMOUSE2×19 未在 boot 执行)在 CG 存在时不写回(不猜)。
- **CGEND(0x03)**:按名移除注册(显示结束 ⇒ 后续查询走「不存在」)。
- CGACT(0x02)COPY2/RECTPAINT 族不触碰注册表(拷贝注册面 Unknown,
  boot 语料无反向实证)。

#### 3. 残余 4 处分歧 = 帧驱动 CG 颜色动画 —— **Unknown(归 P8 效果族)**

修复后对拍 65 处 → **4 处**,对齐区间 [137544]→[294329](VM complete)。
残余 4 处([144055] g1092 / [144407] g628 / [144801] g263 / [144818]
g266)仍同根:引擎侧 BT.OFF 的 COLOR 随迭代演化
(8421504 → 8421504 → **4016232(0x3D3D48)** → 8421504,g1094
CGACT(RECTPAINT3=1, SET=0) 触发的按钮闪烁动画中点),VM 静态注册表
不可复现 —— 需 RECTPAINT3 帧动画模型(P8 效果族;两数据点不足以
猜公式,禁止)。其后两侧自动重汇合,余下 ~15 万组零分歧。

#### 4. P7.1 联动机制补充定性

- **boot 期 TEXT(0x62)命令执行 0 次**(引擎 400k trace 全扫):trace
  停在标题画面主循环(等待输入),scenario 行推进调用链(TEXT →
  scenario 装载/推进)在被动 trace 下不可达 —— P7.1 续需输入注入
  (engine_trace.py 扩展 SendInput)或 TEXT 处理器静态逆向。
- s9 = 游戏 UI/按钮/精灵引擎(ES.GAMEMAIN.TIP.* 纹理族),由 GOSUB
  帧局部实参($55[1]/@53[2] ← s266 → s9 入口链)驱动 —— scenario
  数据 → YSTB 的注入面已实证(成果 61 + 本成果)。

**测试**:`p71_cginfo.rs` 4 测(默认值三元组 / 无 FILE 与空 FILE 不创建 /
CGEND 移除 / X-Y 往返);全仓 workspace 测试全绿(MSVC)。

**验证方式(可复现)**:

```text
python scripts/probe_s9_flow.py                        # flow 级并排(10 处同根证明)
python scripts/probe_yscm_params.py CG CGACT CGINFO     # YSCM 槽位名表
python scripts/engine_trace.py --timeout 75 --max-events 200000 \
    --out <临时路径> --watch 1707:0                       # COLOR 写入值 oracle(两次比对)
cargo test -p yuris-vm --test p71_cginfo
cargo run --release -p yuris-vm --example vm_trace -- \
    "AnimalTrailGirlishSquare 2/pac/bn.ypf" \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl 600000
python scripts/diff_engine_vm.py \
    crates/yuris-vm/tests/golden/engine/engine_trace_boot.jsonl \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl
# → 4 处分歧(单根因:帧驱动 CG 颜色动画,成果 62 §3);对齐至 VM complete
```

***

## 2026-09-05 — P5.2 续:TASKINFO/任务模型 + VARACT PUSH/POP —— VM 跑完 boot 链 complete(成果 61)

### 成果 61:任务注册表模型 + scenario 合流点实证 + 对拍推进 [137024]→[137544] → VM complete —— **Confirmed**

**驱动**:成果 60 收官时唯一停点 s190 pc472 TASKINFO(0x61)。
为取 oracle 重采引擎 trace(**400k 事件/40.88s/335069 组**,旧 100k trace
内引擎从未执行过 TASKINFO)。

#### 1. 任务模型定性 —— **Confirmed**(反编译 + 400k trace 运行时实证)

- **TASKINFO(0x61,引擎 00451838)**:槽 0=ID(任务名)/槽 1=LET(写回
  目标,延迟引用)/槽 2=EXIST → 名字查任务注册表:命中写 1,未命中写 0
  (引擎:FUN_0040ca7d 按名查找,查不到 → LET=0 直接返回)。其余查询槽
  (E/A/Z/FILE/../NEXTVOICE/GOLABEL/…/SCRIPTPOS/TEXTPOS)走 FUN_0045baf8
  族 → VM 显式 Unsupported(语料未执行,不猜)。
- **TASK(0x5f,引擎 0044fe40)**:槽 0=ID(名)、槽 1-3=E/A/Z(优先级)、
  槽 4-13=TINT、14-23=TFLT、24-33=TSTR、**槽 34='#'(入口标签)**、
  35/36=SCRIPTPOS/TEXTPOS、37=RESTART。引擎:查/建任务对象 → 标签查表
  (murmur2)装载入口脚本 → 参数写入任务对象(+0x350..0x420 区)→
  初始化帧局部区。VM:**注册名字**(TASKINFO EXIST 查询面),不 spawn。
- **运行时实证(400k trace)**:s24 g195/196 `GOSUB("es._task", "es.
  IDSubTask", "ES.SCENARIO.SubLoop", …)` → s190 pc471-473
  `TASKINFO(ID=$55[1], EXIST, LET=@6374)`(两处均 EXIST=0)→ pc393
  `TASK(名, 标签, @53[3..7])` 创建任务。
- **引擎任务错误路径对齐**:无 ID 且无标签 → 0x18c18;PUSH 前提违反 →
  0x879b80/0x879be0(VARACT 同族)。

#### 2. YSTB→scenario 合流点实证(P7.1 关键前置)—— **Confirmed**

TASK 创建的任务(es.IDSubTask,入口标签 ES.SCENARIO.SubLoop)的**实际
执行 = 主线 GOSUB 标签链顺序调起**:s2 pc45 GOSUB → s190 pc474-476
(LABELINFO 检查)→ s2 pc47 GOSUB → s266 pc0-2 GOSUB → **s9 pc0**
(scenario 文本引擎入口)。boot 段无并行任务交错(trace 全序);单任务
GOSUB 模型可完整复现。TASK 命令的参数(TINT 族)是否需传递进任务
脚本帧局部 —— 待 script 9 分歧定性时验证(当前零分歧 = 不需要)。

#### 3. VARACT PUSH(14)/POP(15)—— **Confirmed**(汇编级,0x454333-0x454620)

- **PUSH(槽 14)**:目标须 1 维数组(desc+2==1);槽值 = 操作位置 pos。
  `for i in (pos+1..count).rev(): arr[i] = arr[i-1]`(元素后移),
  `arr[pos] = 类型默认值`(INT 0 / FLT 0.0[0x579ca8] / STR ""[0x87899c])。
  pos 钳制(>= count-1 时只写末位);无 LET 写回、无 resize。
- **POP(槽 15)**:`for i in pos..count-1: arr[i] = arr[i+1]`(前移),
  `arr[count-1] = 默认值`。同样无 resize。
- 反汇编证据:FUN_0045baf8(数组元素写,EAX=desc/EDX=下标/栈=64 位值,
  越界静默 -1)、FUN_00454630(FLT)/FUN_0045b9f8(STR,值=指针常量);
  插入常量直读 PE(0x579ca8 = 0.0,0x87899c = "")。
- 语料:s8 g4-7(boot:文本行队列 $1240/$1241/$1242/@1243 STR/INT[401]
  各 PUSH @53[1] 实参);语料直方图 [SET,PUSH]×93、[SET,POP]×113。

#### 4. 对拍终态 —— **Confirmed**

- **引擎 400k trace(335069 组)vs VM:前 137544 组零分歧** —— 全 boot
  链 + TASKINFO/TASK + VARACT PUSH/POP + scenario 入口(s9 pc0);
- VM 独立推进 **293864 组跑完 complete**(整个启动脚本链);10 处残余
  分歧全部在 script 9(scenario 文本引擎内部:pc1093 IF 分支、pc628
  循环圈数、pc233 族)—— 新域,归 P7.1;
- **对拍基线换代**:engine_trace_boot.jsonl 100k→400k(335069 组);
  **字体数 oracle 713→783**(环境依赖实测:两次采集值不同 —— 注册字体
  数随运行环境变化,对拍基线必须与引擎 trace 同批采集,常量已注明)。

#### 5. 附带修复

- `event_json` 的 Unsupported reason 未转义(VARACT 槽 Debug 格式含
  `"` → 非法 JSON 行)+ vm_trace.rs done 行同族 → 双双修复。

**测试**:`p5_taskinfo.rs` 3 测(EXIST 往返/未注册 0/非 EXIST 槽挂起);
全仓 workspace 测试全绿(MSVC)。

**验证方式(可复现)**:

```text
python scripts/engine_trace.py --timeout 150 --max-events 400000 --out <dir>   # 引擎 oracle
cargo run --release -p yuris-vm --example vm_trace -- \
    "AnimalTrailGirlishSquare 2/pac/bn.ypf" \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl 600000
# → 293864 组 complete
python scripts/diff_engine_vm.py \
    crates/yuris-vm/tests/golden/engine/engine_trace_boot.jsonl \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl
# → 前 137544 组零分歧;残余 10 处全在 script 9(scenario 域)
cargo test -p yuris-vm --test p5_taskinfo
```

***

## 2026-09-05 — P1 对拍收口:335,069 组唯一 1 处分歧(定性)—— CGINFO 尺寸真值 + 三勘误 + 栈深恢复(成果 66,workspace 47 测试套件全绿)

> 承成果 65 交接状态:workspace 已恢复可构建(yuris-scenario 的
> `src/lib.rs`/`src/parser.rs` 已由 P7.2 线落盘,缺失的 `src/tests.rs`
> 按 lib.rs 模块文档重建最小回归集 —— 原文件在环境迁移中丢失);
> 成果 65 遗留的「p6 测试未验证」项全部真跑通过。
> 环境迁移备注:本机(Windows)新装 rustup 1.98.1 GNU 工具链;
> PyGhidra 需把 JDK21 的 `MSVCP140.dll` 拷至 Python 目录(`_jpype` 加载);
> Ghidra 12.1.3 项目 `D:\Dev\GhidraUser\yuris_p1` 已含全量分析。

### 成果 66:对拍 [144055]→[335069] 全覆盖,唯一残余分歧定性 —— **Confirmed(实证)/Unknown($55[1] 写入者)**

#### 1. CGINFO 槽13/14 = CG 装载图像的**真实宽/高**(勘误,推翻成果 62「静态默认 1/1」)

- **引擎值级 oracle**(engine_trace.py 新增 `--watch-arm-ev N`:延迟到
  第 N 个事件才布防硬件写监视点,绕开 boot 期描述符未初始化):
  - s9 g1089(第 1 次出现,seq 149454):引擎写 **@1705 = 1.0**(FLT 接收器
    writer 0x45bb6b)= `tip_meswindow.png`(**1×1** 占位图);
  - 同组第 2 次出现(seq 158048):引擎写 **@1705 = 1350.0** =
    `tip_meswindow_txspace.png`(**1350×200** 消息窗纹理)。
  - 两图均经包扫描实证存在(`yuris-resource` 新增 `examples/dims_probe.rs`)。
- g1092 IF `@1705==1 && @1706==1 && @1707==0x808080` = **「是 1×1 占位图」
  判定**。旧「静态默认 SX=1/SY=1」模型在第 2 次出现起偏离引擎;COLOR
  =0x808080(occ1 实证)保留。
- **落地**:CgState 增 `img_w/img_h`;CG 命令 FILE 槽(46)非空 →
  `PacFileIndex::image_dims()` 解析 stored PNG 头(仅 24 字节 seek 读)。
- **VFS 路径形态勘误**:脚本 FILE 参数**无扩展名**且用 `/`
  (`cgsys/main/button/type1/tip_meswindow`)→ 候选名 = 归一路径 +
  {.png,.jpg,.bmp,.gif};`main/button/btn_skip_bt4` →
  `type1/btn_skip_bt4n.png` 形态(皮肤子目录 + n 后缀)走 basename
  前缀模糊匹配(**Likely**,引擎 VFS 逻辑名映射待 P6 逆向)。

#### 2. 勘误 C:YpfIndex bn/se 判别式取反(成果 65 勘误 A 的遗留)—— **已修**

- **错**:`is_bn = !(bn 解释落界)` —— 真实 bn 条目(b0∈{0,1} 且落界)
  全部被错判成 se → **sc.ypf 自第 2 条起逐条漂移 1 字节**,
  `$scenario\start.txt` 读出垃圾/NotFound。
- 被双巧合掩盖:se 包(cg/bgm)走类型码路径不经此判别;bn 包读取走
  YpfArchive(独立解析器)。
- **修正**:去掉取反(`is_bn = b0∈{0,1} ∧ 落界`)。修后 sc_zlib_text_read
  真跑通过(start.txt 实为 54 字节跳转表,长度断言已按实测修正)。
- **教训(铁律 2 再+1)**:「判别式修正」也要全包型验证 —— 成果 65 只
  用 cg.ypf(纯 se)验证了勘误 A,恰好测不到 bn 分支。

#### 3. 勘误 D:extract_var_target 变量下标形态写错位 —— **已修(8 调用点)**

- **错**:`56 @1265 | 48 @1704 | 29` 形态(LET 槽 = 数组元素、下标为变量)
  被解析成 base=@1704(下标变量!)、空下标 → CGINFO/VARINFO/VARACT/SAVE/
  LOAD 的写回**写错变量**。与成果 44 修过的 LET 左值窗同族语义,
  但引用槽路径漏同步。
- **修正**:VarTarget::Indexed 携带**下标指令序列**(引擎 kind 2 延迟
  求值),写/读时经 Evaluator 活求值(栈序 = 维度序;常量下标走快路径)。
- 效果:对拍首个分歧点 [144055] → 消失;后续 [144408](IFBLEND 嵌套栈
  形态差)亦随之消失 —— 证实均为本根因的下游数据级联。

#### 4. GOSUB 帧 IF/循环栈深恢复(成果 51.5 落地)

- GosubFrame 增 `if_nest_depth`/`loop_depth`:GOSUB 记录、RETURN 恢复
  (引擎:帧 +1 字节存 IF 栈深;子程序内循环记录随 RETURN 整体丢弃)。
- 效果:消除 IFBLEND 跳转目标分歧(s190 es.BT 按钮链)。

#### 5. 验收结论(P1)

- **引擎 vs VM:335,069 组(引擎 40.88s 全量采集)对齐,唯一 1 处分歧**:
  - 位置:s190 g559 `IF @409 == "MOUSE_L"`(else-if 按键链首环);
  - 定性:**Confirmed(分歧归因)** —— `@409 = $55[1]`(输入系统串)引擎
    确定性为 "MOUSE_L",VM 无输入子系统 → 空。证据:引擎**两次独立采集**
    249,724 组**逐组一致**(含 "MOUSE_L" 同一事件序号)→ 确定性引擎行为,
    非采集噪声;写入者(DIALOG 0x0a 返回串/输入子系统)未逆向 →
    **Unknown**,归 P9.2(输入)。
- VM 侧自身以完整预算(200 万组)跑完 boot 链并推进主循环,零 panic。

**验证方式(可复现)**:

```text
python scripts/engine_trace.py --timeout 150 --max-events 400000 --out <dir>
cargo run --release -p yuris-vm --example vm_trace -- \
    "AnimalTrailGirlishSquare 2/pac/bn.ypf" \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl
python scripts/diff_engine_vm.py \
    crates/yuris-vm/tests/golden/engine/engine_trace_boot.jsonl \
    crates/yuris-vm/tests/golden/engine/vm_trace_boot.jsonl
# → 对齐 335,069 组,差异 1 处(228368,$55[1] 输入串,见定性)
cargo test --workspace   # 47 测试套件全绿
```

#### 6. 下一步影响

- **P9.2(输入)首案明确**:DIALOG 返回串/$55[1] 写入路径逆向
  (0043c984),或输入注入注入oracle(键名表 es.BT.KEY.SET 已解码)。
- 引擎侧工具:engine_trace.py 增 `--watch-arm-ev N`(延迟布防);
  `yuris-resource` 增 `examples/dims_probe.rs`(包内 PNG 尺寸普查)。

***

## 2026-09-05 — M1 出画面 + M2 首个可玩切片:yuris-cli run 启动游戏、scenario 驱动、点击推进对话(成果 67,workspace 47 测试套件全绿)

### 成果 67:播放器集成 —— 双脚本系统并行 + wgpu 渲染 + 可玩对话流 —— **Confirmed(实测截图)**

#### 1. M1 渲染后端实测通过(成果 65 遗留验收项关闭)

- 修 2 处 wgpu 装配错误(WGSL `vec4×vec3` 验证错;uniform bind group
  声明 dynamic offset 但绘制未传)→ **first_pixel 实测出图**
  (logo 淡入 → 注意事项页真实渲染;窗口截图验证)。
- 勘误:render 的纹理查找键 = **layer.resource**(Scene 语义),
  原 `layer.id` 实现被 first_pixel「id==resource」的巧合掩盖。

#### 2. 播放器集成(`yuris-cli run <游戏目录>`)

- **双脚本系统并行**(架构落地,印证成果 20/49 判定):
  1. YSTB VM:Bootstrap(SYSTEM_START)→ 帧节拍驱动(WAIT FRAME/TIME
     挂起恢复)→ 维护系统状态(变量/流程/输入注入面);
  2. scenario 播放器(P7.3):sc.ypf 明文剧本(Big5)→ 背景/立绘/淡入
     淡出/双语台词。
  取舍:VM 的 CG 图层(含引擎 debug 覆盖层)不进场景 —— 视觉由
  scenario 层驱动。
- **P7.3 scenario 播放器**(`crates/yuris-cli/src/scenario.rs`):
  语法表驱动(成果 49),实现 `\BG`(颜色名/`cg\bg\*.png` 解析)、
  `\S`/`\S.D`(path 相对 `cg\`)、`\T`(立绘淡入 + 等待,
  `L_NYA_1A0100` → `cg\stand\*\*\*\{name 小写}.png` 模糊解析)、
  `\WA`、`\FOUT`/`\FIN`(覆盖层动画)、`\VO`(事件记录,音频待接)、
  `\LE`/`\LT`、`(ID:n)`、`\GO`(跨文件)、`\GO.G.IF`(全局槽条件)、
  `\END`;惰性解析(坏文件不阻塞,跳过记录)。
- **文本渲染**(P8.3 首步):fontdue + 系统 CJK 字体(msjh.ttc)栅格化
  → 原始 RGBA 上传(新增 `WgpuBackend::load_image_rgba`;load_image
  期望编码 PNG,运行期生成图须走直传)。
- **输入面**(P9.2 首步):光标逻辑坐标注入(@133/@138 = 引擎
  sysvar case 0x85/0x8a = 窗口对象 +0x2ac/+0x2b0,
  `recon_00403f2c_input_poll.c` 实锤);`$55[1]` 键名广播注入
  (全帧+全局:`$55` 属帧系统变量族,单点写会被帧槽遮蔽)。
  点击 = scenario 推进 + VM 键注入双路。
- **场景切换清理**:跨文件 \GO 后清旧精灵层(scene_reset 标记)。

#### 3. 游玩测试(实测截图记录)

- 启动 → scenario_start(RELEASE 段):LOGO/FOUT/BG(white)/FIN/attention
  序列 → \TITLE(切片跳过)→ \GO(SCENARIO_MAIN) → maho2_01。
- **第一幕可玩**:bg52(办公室)背景渲染 ✓;`\T` 立绘(咪姆/猫角色)
  按指令出场 ✓;`\LT` 繁中台词白字渲染 ✓;**点击推进对话**
  (voice 0001→0002→0003→… 逐行前进,实测截图)。
- 台词层位置/尺寸仍需对引擎截图校准(窗口底缘,Likely 微调)。

#### 4. 剩余(通向 M2 完整)

- 音频(P9.1:voice/BGM/SE,symphonia+cpal/rodio)、文本窗 UI 底框、
  立绘分层合成(cg\stand 多部件)、标题菜单(内置 \TITLE)、选择肢
  (\SEL)、存档(P9.3)、CMXYZ 相机/FLASH 等效果命令。

**验证方式(可复现)**:

```text
cargo run --release -p yuris-cli -- run "AnimalTrailGirlishSquare 2"
# 窗口:LOGO → 注意事项 → bg52 第一幕;点击推进台词,立绘按脚本出场
cargo test --workspace   # 47 测试套件全绿
```

***

## 2026-09-05 — 成果 67「剩余」六项清零:音频/文本窗/标题/选择肢/存档/坐标(成果 68,47 测试套件全绿)

### 成果 68:可玩切片补完 —— 全部六项落地并实测

| 项 | 实现 | 等级 |
|---|---|---|
| 音频(P9.1) | `crates/yuris-cli/src/audio.rs`:rodio + vorbis 三通道 —— BGM(循环,\BGM(,800) 空首槽=仅调音量)/ voice(独占新替旧)/ SE(一次性);音频名 case-insensitive 解析(`voice\{name}.ogg` 族);无声设备降级不 panic | Confirmed(实测 bgm10 循环 + voice 逐行播放) |
| 文本窗底框 | `show_text` 时挂 SC_WIN 层(z=80):txspace 纹理(`cgsys/main/button/type1/tip_meswindow_txspace`)拉伸至底部 1920×250;未命中回退半透明黑 | Confirmed(渲染)/Likely(位置) |
| 标题菜单 | `\TITLE` → 白底 + 居中 logo_wp + 清旧精灵 → 等待点击 → 自然推进 `\GO(SCENARIO_MAIN)`(引擎原生菜单的近似) | Likely(近似) |
| 选择肢 | `\SEL.GO(标签…)` 登记目标;`\SEL("EN"×n,"","TW"×n,"")` 后段为本地语;竖排文本按钮 + 暗底板(z=94/95);`poll_choice` 帧级命中测试(cursor_logic 1.5× letterbox)→ 跳转 | Confirmed(实测:选择 1 → se02 分支跳转生效) |
| 存档读写 | F5 快存 / F9 快读:`save/yskernel_qsave.json` = {scenario 文件, 标签锚点, @50 全局槽×64};读档恢复锚点 + 全局 + 清场景 | Confirmed(实测:F5 → 推进一句 → F9 回滚到存点重播) |
| 坐标微调 | 台词层按内容定宽(canvas 不再固定 1700)+ 窗内定位;选择肢文本内容定宽后居中(修复被挤出屏);立绘保持中心 x 公式 | Likely(按渲染截图迭代) |

**附带修复**:
- 渲染纹理查找键 = `layer.resource`(原 `layer.id` 被 first_pixel 巧合掩盖);
- `WgpuBackend::load_image_rgba`(运行期生成图直传;load_image 期望编码 PNG);
- 文本 canvas 上限(200 字符/6 行)——Big5 解码漂移产生超长串曾致 4GB 分配挂起;
- 选择肢点击命中:`frame_clicked` 帧级标记(旧 `clicked` 被 tick 先消费,poll 恒 false);
- 合成点击不产生 CursorMoved → 用最近 CursorMoved 像素位做命中。

**已知残余勘误(2026-09-05 深夜)**:「Big5 解码字符漂移」**证伪** ——
100px 单字/整串 fontdue 栅格化实测字形完全正确(`examples/font_test.rs`),
解码串日志逐字正确(`台词层就绪 "【男性7@大牌製作人】「嗚咕」"`),此前
判断系低分辨率截图误读。台词文本已按内容定宽 + 窗内垂直居中(250 逻辑高
窗内居中);剩余 = 与引擎截图的逐像素对照(需引擎侧同场景截图,后置)。

**验证方式(可复现)**:

```text
cargo run --release -p yuris-cli -- run "AnimalTrailGirlishSquare 2"
# LOGO → 注意事项 → 标题(点击)→ maho2_01:BGM 循环、voice 逐行、台词点击推进
cargo run --release -p yuris-cli -- run "AnimalTrailGirlishSquare 2" --at "maho2_22"
# 调试跳转:点击至选择肢(想到天寅/白雪/球美),点选 → 分支跳转 se01/02/03
# F5 快存 → 点击推进 → F9 读档(回滚到存点,voice 重播验证)
cargo test --workspace   # 47 测试套件全绿
```

***

## 2026-09-05 深夜 — 标题分层组合落地 + 播放器稳定性瓶颈清单(成果 69,进行中/未完)

### 成果 69:厂商 CG/标题重构 —— \S 语义勘误 + eyecatch 分层素材定位 —— **Confirmed(素材/语义)/未完(稳定性)**

#### 1. \S 参数语义勘误(厂商 CG「错位与不播放」根因)—— **Confirmed**

- **错**:旧实现把 `\S(logo, item/logo_wp, 1100,0,0,199,0,0,167)` 的 param[2]=1100
  当 X 坐标 → 厂商 logo 画到屏幕外(「不播放」表象)。
- **正**:param[2] = **淡入毫秒**(logo 1100 / attention 1000);param[3..5] =
  **(x,y) = 左上角**(0,0 = 全屏图原位);param[5] = z。素材本身 = 1920×1080
  全屏透明 PNG(logo_wp 内嵌居中 Whirlpool 标志;attention_1 = 注意事项整页)。
- `\T(name, ms, x, y, z)`:x = 中心偏移、y = 底部偏移(Likely,渲染效果自洽)。
- `\S.D(name, ms)` = 淡出处置(此前误当显示 → 「资源未命中: 800」假日志)。

#### 2. 标题分层素材定位(cg\eyecatch\st\,全部 1920×1080)—— **Confirmed**

`bg01a.png`(粉黄菱形纹背景)/ `logo.png`(游戏标题字)/ `logo_c.png` /
`sir.png`(白雪·银发兔耳)/ `han.png`(羽琉·黑双马尾皇冠)/ `tet.png`(天寅·红发猫耳)/
`kum.png`(球美)。按钮 = `cgsys\title\btn_start_on / btn_load_on / btn_lastload_on /
btn_extra_on / btn_end_off`(317×76 / 146×47)。

标题组合(对齐真机截图):bg01a 全屏 → tet/sir(-1058,0)/han(-86,0) 层叠 →
logo(1046,-255) → 按钮列(1355,479/582/670 原生尺寸)+ EXTRAS/EXIT(1319/1611,784)。

#### 3. 渲染/输入修复(本轮)

- **纹理查找键勘误**:`render` 应按 `layer.resource` 查纹理(原 `layer.id` 被
  first_pixel id==resource 巧合掩盖)——此前黑屏直接根因;
- `load_image_rgba`(运行期生成图原始 RGBA 直传;load_image 期望编码 PNG);
- 文本 canvas 上限(200 字符/6 行):Big5 解码漂移超长串曾致 4GB 分配挂起;
- 选择肢命中改 `frame_clicked` 帧级标记(`clicked` 被 tick 先 `take`,poll 恒 false);
- 合成点击不一定产生 CursorMoved → 用最近 CursorMoved 像素位;
- 文本按内容定宽(选择肢文本不再被挤出屏)+ 台词窗内垂直居中;
- 场景重置统一 `reset_title_layers`(精灵/标题层/按钮/文本窗/fade 全清)。

#### 4. 「Big5 解码字符漂移」证伪 —— **Confirmed(证伪)**

100px 单字 + 整串 fontdue 栅格化(`examples/font_test.rs`)字形完全正确;
解码串日志逐字正确。此前「漂移」系低分辨率截图误读。但注意:
**GOSUB 字符串参数的 UTF-8 lossy 乱码仍在**(成果 59 记录的另一问题,未混同)。

### 当前瓶颈与问题清单(⚠ 下位接手者按序处理)

| # | 瓶颈 | 现象/根因 | 建议 |
|---|---|---|---|
| B1 | **播放器渲染退出**:窗口最小化/恢复后 `get_current_texture` 报 "surface has changed" 连刷错误 → `event_loop.exit()` 播放器退出(实测发生) | render 失败即退出;Resized 事件未覆盖 surface 失效路径 | render 失败时先 `resize(surface_size)` 重配 swapchain 重试一次;0 尺寸跳帧 |
| B2 | **标题层残留(统一后未实测)**:reset_scene 已统一为 reset_title_layers,但点击 START 进入 maho2_01 后标题层是否全清**未复测** | 上次实测:START 后标题层残留覆盖 bg52 | 复测:标题点击 → bg52 + 台词,无标题残留 |
| B3 | **VM CG 图层抑制的取舍**:标题/消息窗/界面 CG 全部走 VM CG 事件,当前为让 scenario 视觉可见而整体抑制;引擎 debug 覆盖层(cgsys\debug\btn_*)同样走该通道 | 长期 = 按 CG 名过滤(debug 族跳过);或 VM 出图 + scenario 只出对话层 | P8.2 SceneBridge 过滤规则 |
| B4 | **引擎对照缺失**:标题分层偏移(-1058/-86/1046)与 \T 坐标语义(中心偏移/底部偏移)均按用户截图目测,无引擎侧数值对照 | Likely 级 | 引擎同场景截图对拍 or \S/\T 处理器逆向(明文解释器 0x4cb000 族) |
| B5 | **\GO.G.IF 全局槽语义**:`G=n → @50[n]` 映射未验证(新游戏 slot1 应为 1 的设定来源未明) | 跳过不猜(记录日志) | VARINFO/SAVE 写入路径逆向 |
| B6 | **台词窗边距/裁切**:文本窗内垂直居中已做,但多行台词(2 行以上)底部仍可能越界;台词窗 250 高 vs 引擎实际(引擎台词窗 UI 由 VM CG 事件绘制的消息窗部件组成) | 视觉微调 | P8.3 文本渲染精调 |
| B7 | **音频细节**:SE 日文名文件 case-insensitive 匹配可能未命中(`se\{Script名}.ogg` 为日文);BGM 淡出/淡入过渡、voice 打断语义未实现 | 部分未命中日志 | P9.1 |
| B8 | **GOSUB 字符串参数 UTF-8 lossy 乱码**(成果 59 遗留,勿与 Big5 漂移混同) | s190 等处调试日志可见 | 与 B5 同族逆向 |
| B9 | **Big5 解码个别行仍可能漂移**:font_test 证伪了「整体漂移」,但 maho2_22 等文件渲染台词的逐字对照未做(P7.2 解析器对引号内逗号/双字节尾字节的分词边界未逐字验证) | P7.2 遗留 | 对照 probe_p72 语法事实重验分词 |

### 测试与状态

- **47 测试套件全绿**;`--at <标签>` 调试跳转参数已加入(选段实测入口)。
- 播放链路实测:启动 → 厂商 CG(logo 1.1s 淡入/3s/0.8s 淡出)→ 注意事项 →
  FOUT → 标题(分层组合 + 真实按钮)→ 点击 START → maho2_01(BGM + voice +
  台词推进)✓;选择肢命中跳转 ✓;F5/F9 回滚 ✓。

***

## 2026-09-05 — 跨游戏验证 #1:NEKO-NIN exHeart(E-ris 555)实测 + 四处修复(成果 70,47 测试套件全绿)

### 成果 70:第二游戏样本接入 —— **Confirmed(实测,macOS arm64)**

样本:`NEKO-NIN exHeart`(猫忍之心,官方中文版,E-ris 555 / YPF 500 / YSTB key `58fffb91`,
17 个外置 txt 剧本,Big5 编码)。与既有样本(AnimalTrailGirlishSquare 2)同引擎不同游戏,
用于验证内核的**跨游戏通用性**。本条目所有修复均以「样本 A 能跑、样本 B 报错」的差异为证据。

#### 1. scenario `parse_args` 逗号双重递进 —— **Confirmed(严重,已修)**

- 现象:NEKO 剧本所有含空槽/无空格参数的行报「括号未闭合」,如 `\SE(seno_029,,180,)`;
  整文件解析失败 → 双脚本系统 scenario 侧全灭。
- 根因:`parse_args` 的 `b','` 分支内 `i += 1` 之后,外层 `else` 的 `i += 1` 再次执行
  → **每个逗号吞掉其后 1 字节**。样本 A 语料恰为「逗号+空格」风格(`a, 260`),
  跳过的恰是空格,歪打正着;NEKO「逗号紧跟参数」风格(`a,260`)即参数错位。
  单行最小复现:`\SE(a,)` 修复前 FAIL、`\SE(a,,)` 解析成 `[Str("a"), Str(",")]`(静默错值)。
- 修复:删逗号分支内 `i += 1`,参数起点改为 `cur_start = i + 1`;两种风格 + `(,)`/`(,800)`
  空槽形态(模块文档既有断言)全部验证通过;47 套件全绿。
- ⚠ 关联:样本 A 的 VM 侧素材坐标若曾由 scenario 参数驱动,需重验(B4 的
  「无引擎侧数值对照」风险可能部分源于此)。

#### 2. `#=name` 标签形态 —— **Confirmed(已修)**

NEKO 剧本存在 `#=TR_2A` 标签行(与其余 `#TAM01` 并存);docs/opcode/opcode-table.md
0x23 `#` 前缀记录(源码 `#=es.BT.CG.SET` 即 es 族跳转目标)与本实证一致。
解析器原仅接受 `#name` → 直接报非法标签行。修复:`#` 后可选 `=`,标签名取 `=` 之后。

#### 3. YPF 名字边界:NUL 候选结构化验证 + WAV 类型码 —— **Confirmed(已修)**

- 现象:`PacFileIndex::scan_game_dir` 对 NEKO `se.ypf` 解析失败,**静默跳过**
  → 全部 SE 音频不可用(684 条目)。其余 cg/cgsys/st/sn/sysse/sysvo/vo/update3 全闭合。
- 根因:SE 条目名为 Shift-JIS(日文文件名),其中合法 SJIS 尾字节 0xC9 与 name_key
  异或后恰为存储态 0x00(实证:`se\グラスに氷.ogg` 的「に」= SJIS 0x82C9,
  存储态 `4B 00` —— 次字节即 0x00),cstring 扫描
  在名字中途提前命中 NUL → 索引逐条漂移(delta 104B / 684 条)。
  bytes.rs 的「0x00 不参与」假设对非 ASCII 名不成立。
- 修复:`YpfIndex::from_path` 对每个 0x00 候选做结构化校验(se 型 stored 特征
  `uncomp == comp` + bn 型 offset 落界),取首个通过者,全部失败回退首个 NUL;
  顺带补 WAV 类型码(存储 0x05,解码 0xCC;host 侧 `read_stored_bytes` 同步)。
- 附带修复:scan_game_dir 跳包由静默改为 eprintln 留痕(可观测性;op.ypf 报
  bad magic 属预期,实为 ASF/WMV)。

#### 4. `\LC` 台词命令 + 跨平台字体 —— **Confirmed(已修)**

- NEKO 台词形态为 `\LE(英文)` + `\LC(中文)` 成对;播放器只实现样本 A 的
  `\LT(中文)` → NEKO 全部台词不上屏且不阻塞。修复:`LC` 与 `LT` 同路处理。
- 字体路径原硬编码 `C:\Windows\Fonts\msjh.ttc`(Windows-only)→ 改为按平台
  候选回退(macOS:`STHeiti Light.ttc`/`Songti.ttc`/`PingFang.ttc` 等;Linux Noto CJK)。

#### 5. 调试入口(不进核心路径,后续可一键移除)

- `yuris-cli run` 新增 `--key-hex <8hex>`(YSTB 4 字节 XOR key,跨游戏必备;
  正式方案为启动链自动 guess-key,见 yuris ystb guess-key)、`--lenient`
  (VM `set_strict(false)`:Unsupported 命令记录后继续;NEKO 启动链 s184 pc=74
  有 `DEBUGLIST`(0x09)未实现,strict 下 VM 卡 Error 态)、`--at` 失败原因打印。
- 播放器现状:启动 → 标题(素材路径仍为样本 A 硬编码,NEKO 标题层未命中,
  属 B3/P8.2 范畴)→ `--at 07` 跳入第一幕:BGM 循环 ✓、逐句 voice ✓、
  BG 命中 ✓、台词上屏 ✓、点击推进 ✓。
- 已知残余:`\BG.CMXYZ`/`\S.CLXYZ`/`\SP.*` 等定位/动画命令未实现(BG.CMXYZ 被
  当 `\BG` 处理,首参数被当作资源名 → 纯色兜底);`\SE2/\SE3` 未实现;
  VM Error 态下播放器逐帧重试刷日志(lenient 下未复现,但 Error 恢复策略仍欠)。

### 测试与状态

- `cargo build --workspace` / `cargo test --workspace`(47 套件)macOS arm64
  Rust 1.98 全绿,零平台特化代码改动即跨平台。
- 复现:`cargo run --release -p yuris-cli -- run "<NEKO-NIN exHeart>" --key-hex 58fffb91 --lenient --at 07`

***

## 2026-09-06 — 播放器核心抽取 + 文本渲染三修复(成果 71,49 测试套件全绿)

### 成果 71:refactor player-core + 文本渲染修复 —— **Confirmed(实测)**

#### 1. 平台无关播放器核心 `yuris-player-core`(refactor)

- 播放器 glue(PlayerCore/Player/scenario 播放器/rodio 音频/letterbox/字体回退)
  从 `yuris-cli` bin 迁出为独立 crate;`yuris-cli` 变薄壳(winit 事件循环/窗口/
  键鼠翻译)。桌面与 Android 壳共用同一核心(winit 0.30 官方支持
  android-activity)。行为零变化(冒烟回归一致);补全库一致 lint 属性。
- 验证:`cargo test --workspace` 全绿;实机 BGM/voice/台词/存读档一致。

#### 2. 前导零标签跳转失败(\GO(01) → "1") —— **Confirmed**

- 参数分类器把数字形态参数 Int 化,`01`→`1`,而标签 `#01` 按串匹配 → 查无。
- 修复:前导零数字按裸词 Str 保留(宽度即语义;NEKO-NIN exHeart `\GO(01)`
  实证)。验证:测试 + 实机标题 → START → 01 场景跳转成功。

#### 3. \GO.G.IF 全局槽跳转落地 —— **Confirmed(形态)/Likely(映射)**

- 原被当普通 \GO 跳到 s(0)。实现六算子比较跳转;槽 n ≈ @50[n] 为 Likely
  (PROGRESS B5:未真机对照),按纪律加 `// UNVERIFIED` 标注 + 对照测试
  (命中/未命中 × 六算子);未命中走后续 \GO(SCENARIO_MAIN) 兜底。

#### 4. 文字下半截被裁真根因:fontdue Metrics.ymin 语义 —— **Confirmed(实测推翻旧认知)**

- 实测 STHeiti 40px:「猫」ymin=-4 height=37、ascent=34.4 —— fontdue 的
  ymin 是**从行顶(ascent 线)向下**的偏移,非基线系。旧实现按基线解释 →
  整字下坠 34px,底部被 canvas 裁掉(「文字只显示一半」)。
- 修复:字形位 = 行顶 + ymin;canvas 高 = pad + 行盒 × 行数。新增
  `text_metrics_tests` 实测探针固化语义假设。单行/多行台词完整显示。

#### 5. 繁→简显示转换(显示层特性)

- show_text 接入 fast2s(仅显示层,剧本数据不动;繁中语料下的简体显示偏好)。

#### 6. \LC 台词命令 + CJK 字体跨平台回退 + 调试参数(--key-hex/--lenient)

- \LC 与 \LT 同路处理(第二样本 \LE 英文 + \LC 中文成对,\LT 为本样本形态);
- 字体路径按平台候选(Windows/macOS/Android/Linux);--key-hex(过渡,正式路径
  guess-key)/--lenient(strict 旁路)/--at 失败原因打印;README 用法段同步。

### 勘误关联

- 03-phase1-plan.md「0x00 不参与 XOR」表述与 ypf.md 单布局描述已在
  成果 70 配套文档提交中补勘误注。
- fontdue ymin 语义推翻「基线系」直觉认知,实测数据见 §4(可复现:
  text_metrics_tests)。

### 测试与状态

- **49 测试套件全绿**(新增 GO.G.IF 对照测试 3 例 + 字体 metrics 探针);
  macOS arm64 / Rust 1.98。

***

## 2026-09-08 — 精灵淡入动画未接线修复:厂商 LOGO/注意事项页/立绘不可见(成果 72)

### 成果 72:sprite_fades 动画驱动缺失 —— **Confirmed(实测)**

#### 根因

- `yuris-player-core` 的 `update_sprite_fades()`(lib.rs)定义后**从未被任何
  tick 调用**;tick 循环只接了背景黑场的 `update_fade()`。
- `\S(logo, item/logo_wp, 1100,…)` 的 `fade_ms>0` 路径把图层 `alpha` 初始为
  **0.0** 并登记 `sprite_fades` → 动画永不推进 → **厂商 LOGO/注意事项页
  (item/attention_1)/一切带淡入的 \S 与 \T 图层卡在 alpha=0 永不可见**。
- 外观:LOGO 段 WA 等待正常流逝,但视觉空转——白屏 ~9s → 黑 → 标题,
  「厂商 LOGO → fade」全程无画面。

#### 修复

- `Player::tick` 在 `update_fade()` 后接入 `core.update_sprite_fades()`
  (yuris-player-core/src/lib.rs)。一行接线;行为面:sprite_fades(精灵淡入/
  淡出)与 update_fade(背景黑场)两条动画时钟从此并行推进。

#### 验证(可复现)

- `cargo build --release -p yuris-cli && target/release/yuris-cli run
  "AnimalTrailGirlishSquare 2"`。
- **无点击**启动 → 标题实测 **14s**,与剧本时序逐项吻合:FIN 0.5 + logo 淡入
  1.1/停 3/淡出 0.8 + attention 淡入 1/停 3/淡出 0.8 + FOUT 0.8 + WA 0.5×2
  ≈ 13.5s(scenario_start.txt LOGO 段全序列)。
- 日志零「资源未命中」:item/logo_wp、item/attention_1 均解析命中。
- 点击语义实测:单次 MOUSE_L 仅跳过一个 WA/Fading 等待(连点 6s 内到标题即
  此机制,与真机「点击跳过」一致,非缺陷)。

### 关联

- CONTEXT.md 同日建立(全项目唯一术语表;YSER=错误消息池定性同步 README,
  勘正 README 旧称「资源条目表」)。
- 本次为接线遗漏,非逆向结论变更,结论汇总表不变。

***

## 2026-09-08 VARACT 停摆破案 — VARINFO LENGTH=字符数 终证 + VM 越过 s190 pc=36(成果 73)

### 成果 73:VARINFO LENGTH-on-STR 语义勘误(字节→字符)—— **Confirmed(汇编+实测)**

#### 现象与立案

- s190 pc=36(VARACT COPY)POS 越界报错每帧重试,33 分钟会话 12.2 万条,
  系统 UI 脚本链永久停摆(docs/known-issues.md 问题 1)。
- 补 sid/pc 上下文后钉实单点:s190 pc=36,`POS=串字节−4,LENGTH=5`,
  曾呈「取串尾 5 字节」算术闭合 → H2(字节偏移语义)假说一度强支持。

#### 终证过程(三层证据链)

1. **VARACT 守卫反编译复核**(CMDH_00453178 COPY 分支):步进循环
   `off += DAT_0059b0c0[ch]+1` 按字符宽度表走 POS−1 次,越界才报错
   0x1d4ca → **POS=1 基字符序数、LENGTH=字符数**。H2(字节偏移)证伪 ——
   「POS=串字节−4」闭合纯系 ASCII 串字节≈字符的巧合。
2. **引擎可恢复性证伪**(H3):`FUN_0046bea4(0x1d4ca,1)` →
   `FUN_0046befc` 末尾 `DAT_008725dc=1`(与 WM_CLOSE 处理器 FUN_00410de4
   同一标志)→ 帧驱动 FUN_00404164 返回 1 → 主循环 FUN_00403bec 调
   FUN_00410de4 → 引擎退出。**该守卫在真引擎是致命错误**,真引擎从未命中
   → 引擎侧 LENGTH 查询值必然使 POS 合法。
3. **VARINFO LENGTH 汇编终证**(0x4551dc-0x455208):strlen 只作循环边界,
   入栈结果 = 宽度表步进循环的 EAX 计数器(每字符恰 +1,双字节首区额外跳
   1 字节)→ **LENGTH-on-STR = 字符数,非字节数**。推翻 varact_varinfo.md
   旧「Likely 字节数」。

#### 根因

- 实测 hex 取证:失败串 = `config/sound_3/btn_01` + SJIS「ブルードラゴ」
  + `_bt4`(37 字节 / 31 字符,即问题 2 的 UI 请求路径;s190 pc=36 在提取
  按钮资源名尾缀做命名派生)。
- 本实现 VARINFO 槽 13(LENGTH)返回 `sjis_byte_len`=37 → 脚本
  POS=LEN−LENGTH+1=33 > 31 字符 → 字符步进越界。
- 引擎 LENGTH=31 → POS=27,取尾 5 字符「ゴ_bt4」,永不越界。

#### 修复

- `exec_varinfo_query` 槽 13 与 fallback 改用新增 `sjis_char_len()`
  (yuris-vm/src/lib.rs,步进计数与引擎 0x4551dc 循环逐指令同构);
  删除失途的 `sjis_byte_len`。
- VARACT POS 越界报错补 `目标`(SET 引用变量)与 obj 串 hex 转储
  (本次破案的决定性取证手段,留作回归诊断)。

#### 验证(可复现)

- `cargo build --release -p yuris-cli && target/release/yuris-cli run
  "AnimalTrailGirlishSquare 2"`(100 秒窗口)。
- VARACT 错误 **0 条**(修复前同窗口 10,124 条);全日志 **0 错误**。
- VM 越过 s190 pc=36,系统 UI 链整体复活:设置页/音量/标签页等控件
  请求序列大量出现(tip_cgauge、btn_votest、btn_tab_system 等 —— 即
  问题 2 的素材请求,其失败为独立未决问题,不变)。
- 游戏推进到标题画面,主流程不受影响。

### 关联

- known-issues.md 问题 1 结案(1.5 排查计划第 2-4 步由本次终证一并完成;
  H1 上游状态分歧证伪 —— 变量内容与引擎一致,分歧在 LENGTH 语义)。
- varact_varinfo.md §3/§5 LENGTH 语义同步勘误。
- 结论汇总表:VARACT 行等级更新,新增 VARINFO LENGTH 行。
- 问题 2(UI 素材路径派生)仍开放 —— pc=36 的尾缀提取逻辑现已可执行,
  为其逆向提供了可单步观测的运行时。

***

## 2026-09-08 UI 素材路径破案(一) — `_N` 变体剥离回退 + C 类真缺失字节级终证(成果 74)

### 成果 74:resolve_entry `_N` 变体剥离回退 —— **Likely(脚本↔封包对拍拟合,引擎侧代码未取证)**

VM 越过 s190 pc=36(成果 73)后,系统 UI 链复活的素材请求全面暴露。
本轮解决其中「请求形态 ≠ 包内形态」的可解子集。

#### 破案过程(三层证据)

1. **请求源实证**:sid 253/254 系统剧本(YSTB content 原文)以字面量
   `config/sound_2/tip_cgauge`、`config/back_sound_2`、
   `config/btn_tab_sound_2_bt3` 下发 `es.BT.CG.SET`(`M<长度>` 参数窗
   原样可见);`cgsys/` 前缀为 es.BT 子程序运行期字符串拼接(VARACT),
   即**引擎 VFS 收到的就是带 `_2/_3` 的路径**。
2. **封包对照**(鲁棒 YpfIndex 全量 + 字节级 XOR-0xC9 检索):
   `sound_2/tip_cgauge` ↔ 实存 `cgsys\config\sound\tip_cgauge.png`;
   `back_sound_2` ↔ `back_sound_.png`(尾下划线);
   `btn_tab_sound_2_on/_2_bt3` ↔ `btn_tab_sound_on/_bt3.png` ——
   剥 `_N` 形态 **6+ 例全中、0 反例** → 引擎 VFS 未命中时按
   「从右向左逐层剥离 `_单数字` token」回退(**Likely**;引擎解析代码
   不在反编译子集,无法升 Confirmed)。双位数不剥(`_08` 防
   btn_page08_over 误伤)。
3. **C 类真缺失终证**:12 包原始字节 XOR-0xC9 检索,
   `btn_all_mask`、`btn_c01~c16_bt4`、`other/*`(btn_check/btn_show)、
   `back_other`、`btn_tab_other_on`、`sound_3/btn_01~65〈日文〉_bt4`、
   `text/tip_mes_preview_1..3`(包内仅 `_4`)**全部不存在** → 引擎
   同样命中失败,按钮空图为引擎原生表现,不再是本实现的分歧点。

#### 代码落点

- `yuris-vm/src/host.rs` `resolve_entry`:候选链 = 精确(原/归一+扩展名)
  → `_N` 剥离变体逐层+扩展名 → basename 前缀模糊(原链不变,纯增量,
  零回归面)。`read_image_bytes`/`read_cg_bytes`/`read_stored_bytes`
  共用此入口,一并生效。
- 顺带查清 **cgsys_ec.ypf 名字首字节现象**(known-issues 2.4 遗留):
  原始索引区 dump 证实每条名字盘上自带 1 个额外字节(0x10~0x3B),
  可打印时即「前导杂字节」(`(cgsys` 等);非解析漂移、非名字哈希
  (fnv/djb2/sdbm/crc32 全不中)——**语义 Unknown**;查找已被
  YpfIndex 剥根双索引(name[1..])免疫,不阻断。

#### 验证(可复现)

- `cargo build --release -p yuris-cli && target/release/yuris-cli run
  "AnimalTrailGirlishSquare 2"`(32 秒窗口)。
- CG 解析失败 **208 → 151 条**;可解族全部消失:tip_cgauge×32、
  btn_votest/sysvoice/cslider/cmute×16 族、back_sound_2/3、
  btn_tab_system/sound/text _1/_2/_3 全系、btn_tab_other_on 以外
  的标签页族。
- 剩余 151 条与 C 类终证清单一一对应(引擎同表现,不再修)。
- VARACT **0 条**;场景流(LOGO→BG→fade→标题画面)与修复前逐行一致。

### 关联

- known-issues.md 问题 2 部分结案(2.3 A/B 类经 `_N` 回退消解;
  C 类终证为包内不存在;2.4 前导杂字节定性更新)。
- 结论汇总表:资源层新增 `_N` 变体剥离行(Likely)、
  cgsys_ec 名字首字节行(Unknown)。
- 临时探针 tmp_probe_roots/rootbyte/rawidx/crc/nonpng 用毕已删。

***

## 2026-09-08 UI 素材路径破案(二) — 剩余 151 条根因全景:剧本字面量↔失败计数算术闭合 + 引擎原生容错锚定(成果 75)

### 成果 75:剩余 151 条 CG 解析失败 = 产品未打包的可选 UI 素材,引擎原生容错 —— **Confirmed(算术闭合)/Likely(引擎同执行)/Unknown(加载时机)**

成果 74 消解可解族后剩余 151 条(97 唯一路径)。本轮回答遗留的
「为什么剧本会请求不存在的文件」。

#### 1. 逐组来源与算术闭合 —— **Confirmed**(剧本原文 ↔ 失败计数)

全剧本扫描(bn.ypf 全部 yst*.ybn 字节级检索)把 97 个失败路径全部钉到
5 个系统 UI 剧本的 `es.BT.CG.SET`/`es.BT.MAP.CG.SET` 字面量,且
**失败发生次数 == 剧本字面量个数**(全部定义点无条件执行):

| 剧本 | 页面 | 失败路径族 | 条数 |
| --- | --- | --- | --- |
| s251 | config「other」页 | other/btn_show_bt4×18、btn_check_bt4×16、btn_allon/alloff_bt3×2、btn_tab_other_on×2、back_other×2 | 40 |
| s252 | config「sound」页 | sound/btn_c07~09_bt4×3、btn_all_mask×3 | 6 |
| s253 | config「sound_2」页 | sound_2/btn_c01~16_bt4×16、btn_all_mask×16 | 32 |
| s254 | config「sound_3」页(音声作品页) | sound_3/btn_01~65〈日文商品名〉_bt4×65、btn_allon/alloff_bt3×2 | 67 |
| s250 | config「text」页 | text/tip_mes_preview_1/2/3×2(字面量 4 处,`_4` 实存) | 6 |
| **合计** | | | **151**(与日志实测精确一致) |

#### 2. 结构发现:包内是「通用钮替代 per-channel 钮」的素材裁剪 —— **Confirmed**

- s253 每声道定义**两个按钮角色**:`es.BT.SET "VOL.CHARA.GAUGE"` →
  `btn_cslider_bt3`(**包内实存**)与 `es.BT.SET "VOL.CHARA.MUTE.ON"` →
  `btn_cNN_bt4` + `es.BT.MAP.CG.SET btn_all_mask`(**包内无**)。不是
  同钮双皮肤重定义,是不同按钮角色;包内实存 `btn_cslider_*`/
  `btn_cmute_bt4`/`btn_cmute_r_bt4` 通用钮 → **产品用通用钮替代了
  per-channel 钮,per-channel 素材从未打包**。s252 同构
  (`VOL.CHARA.SYSVOICE2`→`btn_cmute_r_bt4` 实存,c07/08/09 缺失)。
- cgsys_ec.ypf `config\` 下仅 `sound\system\text` 三个子目录,
  标签钮仅 `btn_tab_{sound,system,text}_*` 三族 —— 无 `other\`/
  `sound_2\`/`sound_3\` 目录;`update1.ypf`(1,306 条)仅
  `voice\*.ogg` + `cg\ev\*.png`,无 UI 补丁 → 三个页面的素材
  **在任何包中都不存在**(与成果 74 的 12 包字节级终证一致)。
- s254 的 65 个日文按钮名(ブルードラゴ 等)= 音声作品页商品表,
  由 VARACT 运行期拼串(成果 73 hex 取证),作品表在剧本数据里,
  按钮图未打包。

#### 3. 引擎原生容错锚定 —— **Likely**(引擎同执行)/Unknown(加载时机)

- 成果 62 watch oracle 已实证引擎 CGINFO 对「未找到」走 FLT 接收器
  写 0.0 路径(writer 0x45469e)——这是引擎**设计的**未找到路径,
  非异常。引擎执行同一剧本的全部定义点(控制流无分支跳过;
  本实现控制流有 15 万组对拍零分歧背书)→ 引擎同样对这 97 路径
  命中失败 → 空按钮继续运行,不报错不阻断。
- 引擎加载时机(命令点即读图头 vs 首绘时)未取证(Unknown),
  但两种时机下缺失文件的结局相同(空图),不影响结论。

#### 4. 结论与处置

剩余 151 条 = **引擎标准系统剧本引用了本产品未打包的可选 UI 素材,
引擎原生容错(空按钮),与本实现零分歧**。不再修;唯一差异是我方
player 每次未命中打一行日志(引擎静默),如需可降为 debug 级/
每路径去重(未做,保持取证可见性)。

#### 验证(可复现)

- `python3 /tmp/scan_all_scripts.py btn_cslider btn_c01 btn_all_mask
  btn_tab_other back_other /other sound_3 btn_show btn_check
  mes_preview`(bn.ypf 全剧本字节级扫描,字面量计数表)。
- `cargo run -p yuris-format --example tmp_probe_cgsys --
  pac/cgsys_ec.ypf "config\\" 500`(config 子目录清单)+
  对 update1.ypf 全量抽样(无 UI 条目)。
- 失败分组:`grep "CG 图像解析失败" run.log | sed … | sort | uniq -c`
  与上表逐组核对,合计 151。

#### 5. 同日补充:失败日志补 sid/pc —— 宏体执行点钉在 s9(问题 1 排查第 1 步手法复用)

按 known-issues 问题 1 排查第 1 步(「补上下文后钉实单点」)同款手法,
`VmEvent::Cg` 新增 `script_id` 字段,player 失败行补 `(s{sid} pc={pc})`。
实跑 45 秒窗口,151 条全部带上定位:

| 执行点 | 宏体 | 条数 | 路径族 |
| --- | --- | --- | --- |
| s9 pc=978 | es.BT.CG.SET | 118 | btn_show×18+btn_check×16+c01~16×16+c07/08/09×3+sound_3×65 |
| s9 pc=1329 | es.BT.MAP.CG.SET | 19 | btn_all_mask×16+3 |
| s9 pc=1072 / pc=1297 | 文本页两调用点 | 各 5 | tip_mes_preview_1/2/3、btn_tab_other_on、back_other |
| s9 pc=1006 | allon/alloff 族 | 4 | other/sound_3 的 btn_allon/alloff_bt3 |

两点细化(不改结论):**es.BT.* 宏体实现在 s9 系统宏库**,s250~s254
为调用侧字面量(「字面量数==失败次数」闭合在调用侧成立);预览/标签/
背景的 ×2 重复 = pc=1072 与 pc=1297 **两个不同调用点**各发一次,
非同点时间重复。VARACT 0 条,无回归。

### 关联

- known-issues.md 问题 2 全案终结(2.7 新增根因全景;2.6 遗留项
  tip_mes_preview 归入同类)。
- 成果 73/74 的后续:VARACT hex 取证中的 `sound_3/btn_01` 拼串、
  `_N` 剥离后残余,均在本成果收口。

***

## 2026-09-08 标题菜单点击路由(成果 76)

### 成果 76:标题按钮按功能路由(START/LOAD/LASTLOAD/EXTRA/END) —— **Confirmed(命中路由)/Likely(START 落入开场)**

**现象**(用户报告):标题界面无论点哪个按钮都进入游戏。

**根因**(Confirmed,代码链路):
- VM 层无 click 通道(全 crate 检索 0 命中);点击仅被 scenario 驱动消费。
- `\TITLE` 处理为 `title_screen()`(内置硬编码标题,5 个按钮为纯贴图)
  + `Wait::Line` 盲推进 → **任意点击**(含按钮/背景)清等待 → scenario
  继续执行后续行 → 进游戏开场。按钮从未参与命中判定。

**修复**:
- `Wait` 新增 `TitleMenu` 变体;`\TITLE` 绑定之(点背景不推进,对齐
  引擎原生菜单语义)。
- `ScenarioHost` 新增 4 默认方法:`poll_title_menu`(命中→
  `TitleMenuAction{Start,Load,LastLoad,Extra,End}`)/`title_load`/
  `title_extra`/`request_quit`;测试桩零改动。
- `title_screen()` 按钮层(id 0x5C_7000_0005..9)原生尺寸记录命中区
  `title_buttons`;`reset_title_layers()` 同步清空。
- 路由:START → 清等待落入执行循环(开场);LOAD/LASTLOAD →
  `request_title_load` 请求,`Player::tick` 消费走 `quick_load()`
  (scenario/core 分裂借用所致;成功 → `start()` 重置等待,失败(无快存)
  → **保持标题等待不落穿**);EXTRA → 日志「未实现」留在标题;
  END → `request_quit`,主循环 `event_loop.exit()`。
- 防落穿双保险:LOAD 失败不预清 wait;快读消费帧清 `frame_clicked`
  (防同一次点击推进快读后首行)。

**实机验证**(40 秒窗口,用户实际点击):
- Load(0x06)×3、LastLoad(0x07)×4 → 「无快存」→ 留在标题 ✓
- Extra(0x08) → 「未实现」留在标题 ✓
- **End(0x09) → 进程 exit 0 干净退出** ✓
- 背景点击 ×2 → 无按钮日志、无推进(正确忽略)✓
- 不同按钮命中不同 id,矩形判定准确 ✓
- START 落入开场路径未实测(本次未点击);同一 poll 机制,仅差
  wait 清空 → **Likely**。
- VARACT 0 条,无回归。

**遗留**(不在本成果范围):LOAD 存档槽列表屏(引擎 cgsys/load/* 原生
UI)、按钮悬停 _off/_on 换图、EXTRA 鉴赏菜单。

***

## 2026-09-08 标题按钮机制取证与问题计划(成果 77)

### 成果 77:`\TITLE` 经 G1 传递按钮选择;按钮三态素材实证;问题计划成文 —— **Confirmed(剧本流/素材态)/Unknown(sse 映射)**

**取证**(工具:`/tmp/dump_sc.py` 剧本转储、`crates/yuris-vm/examples/tmp_extract.rs`
播放器同路径提取素材):

1. **剧本流**(sc.ypf `scenario\scenario_start.txt #SCENARIO_TITLE`):
   `\TITLE` → `\GO.G.IF(1,"==",1, SCENARIO_MAIN)` → `\GO.G.IF(1,"==",2, ARA)`
   → 兜底 `\GO(SCENARIO_MAIN)`。按钮选择写入**全局变量 G1**
   (1=START→maho2_01 开场;2=OUTLINE/あらすじ→ara.txt 前情回顾);
   **未写 G1 的任何路径经兜底必进开场** —— 成果 76 前旧实现
   (`Wait::Line` 不写 G1)"点哪个都进游戏"的机制根因。
2. **按钮素材三态 + 不可用态**(cgsys_ec.ypf 实存,已提取目验):
   `_off`=常态(粉底紫字)/`_on`=高亮(金字星光)/`_over`=按下(近 _on)/
   `_na`=灰化不可用(CONTINUE;lastload/extra 有)。按钮全集 9 个:
   start/load/lastload/arasuji/extra/end/config/manual/web。
3. **音效面**:sysse.ypf 实存 `sse01~06.ogg` 6 个系统音效;播放器标题
   流程无任何 `play_se` 调用(代码检索实证);sse 语义映射 Unknown。
4. **当前实现差距**:默认绘制 `_on` 高亮态(原生应默认 `_off`);无
   悬停/按下/灰化切换;无 OUTLINE 按钮(剧本已支持 G1==2);Start 依赖
   兜底路径未写 G1;`btn_confirm_title_bt4` 提示 END 可能有确认对话框
   (Hypothesis)。

**验证方式**:
- `python3 /tmp/dump_sc.py start.txt` 复现剧本原文;
- `cargo run -p yuris-vm --example tmp_extract -- <游戏目录> /tmp/title_btn`
  复现素材提取与目验;
- `grep -n play_se crates/yuris-player-core/src/lib.rs` 确认标题流程无 SE。

**产出**:`docs/title-menu-plan.md`(差距清单 G1~G7 + 行动计划
P1 保真度核心/P2 反馈层/P3 功能补全)。

***

## 2026-09-08 标题按钮三态绘制 + G1 写入(P1,成果 78)

### 成果 78:按钮 `_off/_on/_over` 三态切换 + START 写 G1 走原生分支 —— **Confirmed(单测+素材)/Likely(实机目测待确认)**

**实现**(差距 G1/G2/G3,计划 P1-1/P1-2):
- `TitleButton{rect,id,rids[3],shown}`(`title_buttons` 元素升级,原
  二元组弃);`title_screen()` 预载 5 钮 × 3 态 = 15 张
  (`cgsys/title/btn_{start,load,lastload,extra,end}_{off,on,over}`,
  单态失败 = None 不切该态),默认绘 `_off`(旧实现误绘 `_on` 高亮)。
- `PlayerCore::update_title_buttons()`(固有 impl):每帧按
  `cursor_logical` 命中 + `frame_clicked` 定态(悬停 `_on`/按下帧
  `_over`/其余 `_off`),仅换资源不动几何;`Player::tick` 在
  scenario.tick 后调用;`reset_title_layers()` 清 `title_buttons`
  (补成果 76 漏项:命中区曾不清)。
- `ScenarioHost::set_global(slot,value)` 默认方法(测试桩零改动);
  scenario `TitleMenuAction::Start` → `host.set_global(1,1)` 后
  `wait=None`,流程经 `\GO.G.IF(1,"==",1,SCENARIO_MAIN)` 落开场
  (成果 77 剧本实证路径;不再依赖兜底 `\GO(SCENARIO_MAIN)`)。
  PlayerCore 写 `@50[slot]`(`store_mut().set_elem`,与 `global()`
  同槽;@50 未声明时记日志退回兜底,不劣于旧态)。

**验证**:
- `cargo test -p yuris-player-core` 4 通过,含新增回归
  `title_start_writes_g1_and_branches`(\TITLE → Start → G1=1 →
  GO.G.IF 命中 MAIN,兜底未走);
- release 构建 + 启动:标题 15 张三态素材全命中(无「未命中」),
  VARACT 0 条;**实机目测悬停高亮/按下闪烁、START 日志
  `GO.G.IF @[slot 1]=1 == 1 → SCENARIO_MAIN` 待用户点击确认**。

**实机验证 + 勘误(同日)**:
- 实机:三态绘制生效(用户确认按钮特效变化);START 点击暴露
  **`写 G1=1 失败:idx[0]=1, bound=1 (array 6450=@50)`** ——
  **B5 的 `G=n → @50[n]` 映射被运行时证伪**(@50 声明 dims=[1],
  idx≥1 越界;旧 global() 静默吞错恒读 0,故此前未暴露)。
  流程仍正确落开场 = 兜底 `\GO(SCENARIO_MAIN)`(Q:测试通过仅指
  单测自洽;引擎真值存疑)。
- **勘误修复**:G 槽改 PlayerCore 内部 `globals: HashMap<usize,i64>`
  (global/set_global 同源自洽;quick_save/quick_load 同步改挂,
  JSON `globals[0..64]` 形状不变 —— 原"@50 全局槽×64"实为死数据,
  仅 idx0 有效)。GO.G.IF 日志改 `G[slot]` 措辞,注释同步勘误。
- **附带取证**(es 系统宏库,YSTB 字符串 XOR 循环密钥解码):
  1. 全语料 `\GO.G.IF` 仅 scenario_start.txt 两处,均槽 1;
  2. **标题菜单为 YSTB 脚本驱动**:`yst00259.ybn` 以 es.BT.* 宏族
     (CG/XY/Z/SE/SET/NAME/GROUP.SET)构建按钮,引用
     `title/btn_start` + `BTN.START`;
  3. **`es.BT.SE.SET` 绑定 `sysse/sse02`+`sysse/sse03`**(按钮悬停/
     决定音候选,P2-1 映射取证直接命中);
  4. 引擎反编译(~/ghidra_all/kemonomichi2.exe)无明文命令串
     (哈希/分词匹配),G 真值实体仍 Unknown(B5 保持,待 es.BT
     宏链或解释器定向逆向)。
- 勘误后测试 4 通过;G1 落地路径:`写 G1=1` →
  `GO.G.IF G[1]=1 == 1 → SCENARIO_MAIN`(日志措辞已更新)。

***

## 2026-09-08 OUTLINE 按钮 + CONTINUE 无存档灰化(P1 收尾,成果 79)

### 成果 79:标题按钮原生坐标/绑定全解码(yst00259 静态解码)+ OUTLINE 落 ARA + `_na` 态 —— **Confirmed(脚本静态解码+素材)/Likely(实机目测待确认)**

**先导确认**:成果 78 实机验证通过(用户确认三态特效变化),其
Likely(实机目测)升 Confirmed。

**新取证**(`yst00259.ybn` 全量静态解码,临时工具 `/tmp/dump_ystb_groups.py`:
YSTB header→part1 命令组→槽位表→content 窗口 `[op:u8][len:u16][operand]`
逐窗解码;修正点名:bn.ypf 条目名带 `$ysbin\` 前缀,须后缀匹配):
1. **SCENE1 原生布局/绑定真值**(es.BT.XY.SET 坐标 + es.BT.SET 绑定,
   LOGICAL 1920×1080 一比一;按钮 PNG 可见 bbox 无透明边距,
   坐标 = 图层左上角):

   | 按钮(NAME.SET Shift-JIS) | XY | CG | 绑定 |
   |---|---|---|---|
   | ★あらすじ | (1393,384) | btn_arasuji(312×29) | **BTN.START,参数 2** |
   | ★スタート | (1393,439) | btn_start(317×76) | BTN.START,参数 1 |
   | ★ロード | (1393,533) | btn_load | BTN.LOAD |
   | ★前回からの続き | (1393,627) | btn_lastload_{off,over,on,na} 显式五参 | BTN.LLOAD |
   | (同名,无存档) | (1393,627) | btn_lastload_na | **BTN.LLOAD.NA** |
   | ★おまけ | (1314,745) | btn_extra | BTN.CGMODE(另注册 VOMODE 同位) |
   | ★コンフィグ | (1477,745) | btn_config | BTN.CONFIG |
   | ★終了 | (1640,745) | btn_end(146×47) | BTN.END |

2. **BTN.START 参数即 G1 写入值**(arasuji=2/start=1)——\TITLE 按钮选择
   写 G1 的引擎侧机制闭环:arasuji(前情回顾)点击 → G1=2 →
   `\GO.G.IF(1,"==",2,ARA)` 落前情回顾(成果 77 剧本侧证据对上)。
3. **CONTINUE 双按钮条件注册**:ct=44 条件组按存档存在性在
   BTN.LLOAD(btn_lastload 三态)与 BTN.LLOAD.NA(btn_lastload_na 单张)
   间二选一,同位 (1393,627)。
4. **素材面勘误(计划 G3/P1-4 修订)**:`btn_load` **无** `_na` 变体
   (cgsys_ec.ypf 全扫),LOAD 恒可用;`_na` 仅 lastload(+extra 有
   `btn_extra_na` 但 SCENE1 未绑定)。manual/web 仅 off/over 双态。
5. 旧布局(成果 78 截图目测 1355,479 等)与原生坐标有 **5~38px 非常数
   偏差** → 全列改原生坐标。
6. BTDEF.SCENE1/2/3 三套定义:SCENE2/3(通关后)无 start/arasuji,
   换 btn_true/btn_scenejump(+na)(记录不实装)。

**实现**(计划 P1-3/P1-4):
- `title_screen()` 按钮表改 6 钮原生坐标(+arasuji,层 id
  `0x5C_7000_000A`);无快存文件(`save/yskernel_qsave.json`,以本实现
  快存为判据;引擎原判据 save/*.sd 未知悉)时 lastload 改载
  `_na` 单素材。
- `TitleButton` 增 `active: bool`:`_na` 态 = 单素材、`update_title_buttons`
  跳过三态切换、`poll_title_menu` 跳过命中(点击无效)。
- `TitleMenuAction::Outline`:与 Start 同路由,写 `set_global(1,2)` →
  剧本流 `\GO.G.IF` 命中 ARA(前情回顾 ara.txt 18061 B,直接可播)。

**验证**:
- `cargo test -p yuris-player-core` 5 通过,含新增回归
  `title_outline_writes_g2_and_branches`(OUTLINE → G1=2 → GO.G.IF 命中
  ARA,兜底未走;原 START 回归参数化共存);
- `cargo check --workspace` / `cargo test --workspace` 全绿;
- 实机(待用户):arasuji 显示于 START 上方细条(312×29)、点击落
  前情回顾;无存档启动 CONTINUE 灰化且点击无效。

***

## 更新约定

每次更新本文件时：

- 阶段状态表改状态

- 新增「成果 N」条目，必须包含**验证方式**（可复现的步骤）

- 结论汇总表同步增删

- 所有推测必须标注等级，**禁止把 Hypothesis 写成 Confirmed**

