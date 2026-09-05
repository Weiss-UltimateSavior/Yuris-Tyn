# YSTB 脚本容器格式规格

| 项 | 值 |
|---|---|
| 状态 | **Confirmed**（结构 + 加密机制 + 密钥 + content 编码，均有可复现验证） |
| 样本 | `bn.ypf` 内全部 302 个 `yst*.ybn`（引擎 v555；首测样本 `yst00000.ybn`，11,209 字节） |
| 样本密钥 | `2b904f93`（**全游戏统一**，278/278 可判定文件一致） |

> ⚠️ 加密**密钥逐游戏不同**。机制 Confirmed，具体 key 需按 §5 的方法确定。
>
> ⚠️ **2026-09-02 第二批勘误**（全语料 302 文件扫描后，详见 §10）：
> 1. §4.1 的 tag 字节序注记有误（`16 00 03 00` 的 u32 LE 是 `0x00030016`）；
> 2. §6 第一槽位内容 listing 误把下一槽位的 5 字节画了进来（27 字节 ≠ 22）；
> 3. §4.2/§5 的「偏移全程连续 / 连续性=1.0」**不是普适性质** —— 仅顺序型文件成立，
>    池式文件正确密钥下也只有 ~0.70；§5 约束 1 的字段下标有误（offset 在
>    cipher[8..12]，不在 [4..8]）。密钥判定请用 `key_score()`（连续性 ∨ 窗口闭合率）。

---

## 1. Header（0x20 字节，**不加密**）

| Offset | Size | 样本值 | 字段 | 说明 |
|---|---|---|---|---|
| 0x00 | 4 | `YSTB` | `magic` | |
| 0x04 | 4 | `555` | `version` | 引擎版本 |
| 0x08 | 4 | `202` | `unknown1` | **语义 Unknown**（样本值 202） |
| 0x0C | 4 | `808` | `part1_len` | part1 区长度 |
| 0x10 | 4 | `4824` | `command_len` | 槽位表长度（**必为 12 的倍数**） |
| 0x14 | 4 | `4737` | `content_len` | 内容区长度 |
| 0x18 | 4 | `808` | `part4_len` | part4 区长度 |
| 0x1C | 4 | `0` | `unknown2` | 实测恒 0 |

```rust
pub struct YstbHeader {
    pub magic: [u8; 4],   // b"YSTB"
    pub version: u32,
    pub unknown1: u32,
    pub part1_len: u32,
    pub command_len: u32,
    pub content_len: u32,
    pub part4_len: u32,
    pub unknown2: u32,
}
```

**必加断言**：
```rust
assert_eq!(0x20 + part1_len + command_len + content_len + part4_len, file_len);
assert_eq!(command_len % 12, 0);
```

---

## 2. 分区布局

```
[Header 0x20]          未加密
[part1     : part1_len       ]   ← 加密，语义 Unknown
[commands  : command_len     ]   ← 加密，定长 12 字节槽位表
[content   : content_len     ]   ← 加密，VM 字节码 / 文本内容
[part4     : part4_len       ]   ← 加密，语义 Unknown
```

> **命名说明**：既有工具 `YURIS_TOOLS-main/YSTB_FILE.py` 称第 3 区为 `strs`
> （字符串区）。在本样本（v555）中该区实际存放的是**变长 VM 字节码**，
> 不是明文字符串。本项目改称 `content`，避免误导。

**`part1` 与 `part4` 必须原样保留**，不要因为"不知道是什么"就丢弃。

样本中 `part1_len == part4_len == 808`，且 `808 = 202 × 4`，
而 `command_len / 12 = 402 = 202 × 2` —— 这个数量关系值得注意，但语义 **Unknown**。

---

## 3. 加密

**4 字节循环 XOR，跳过前 0x20 字节**：

```rust
pub fn xor_cyclic(data: &mut [u8], key: &[u8; 4], skip: usize) {
    for (i, b) in data.iter_mut().enumerate().skip(skip) {
        *b ^= key[i % 4];
    }
}
```

注意：`i` 是**绝对文件偏移**，不是相对偏移。因为 `0x20 ≡ 0 (mod 4)`，
各分区起始处的相位都为 0。

样本密钥：**`2b 90 4f 93`**（作为 u32 LE 读作 `0x934f902b`）。

---

## 4. Command 槽位表（定长 12 字节）

```rust
#[derive(Debug, Clone, Copy)]
pub struct CommandSlot {
    pub tag:    u32,   // 内容类型标签
    pub len:    u32,   // 内容字节数
    pub offset: u32,   // 相对 content 区起始的偏移
}
```

### 4.1 已确认的 tag 值

| tag (u32 LE) | 原始字节 | 语义 | 来源 |
|---|---|---|---|
| `0x00000000` | `00 00 00 00` | text（正文） | `YSTB_FILE.py` + 本样本 |
| `0x00010000` | `00 00 01 00` | **语义 Unknown** | 本样本出现 169 次 |
| `0x00030000` | `00 00 03 00` | may_be_opt（选项） | `YSTB_FILE.py` + 本样本 |
| `0x03160000` | `16 00 03 00` | sound（语音） | `YSTB_FILE.py`（本样本未出现） |
| `0x03210000` | `21 00 03 00` | name_def | `YSTB_FILE.py`（本样本未出现） |
| `0x03220000` | `22 00 03 00` | name_def | `YSTB_FILE.py`（本样本未出现） |

> 注意 tag 的字节序解读：`YSTB_FILE.py` 按 `opcode == b'\x16\x00\x03\x00'` 比较字节，
> 作为 u32 LE 即 **`0x00030016`**（~~0x03160000~~ 为错误读法，2026-09-02 勘误）。
> **记录时两种写法都要写清，避免混乱。**
>
> 2026-09-02 全语料补充：tag 的完整语义 Unknown，`tag >> 16` 的实测分布见
> `docs/opcode/opcode-table.md` §4。已证伪「tag 低字节 = YSCM 命令下标」假说
> （0x16 ↔ YSCM[0x16]=FLASH、0x21/0x22 ↔ G_INT/G_INT2，语义均不匹配）。

### 4.2 样本实测（前 16 条，密钥 `2b904f93`）

```
# 0: tag=0x00000000  len=22  off=0
# 1: tag=0x00030000  len=5   off=22
# 2: tag=0x00000000  len=22  off=27
# 3: tag=0x00030000  len=5   off=49
# 4: tag=0x00000000  len=22  off=54
# 5: tag=0x00030000  len=5   off=76
# 6: tag=0x00000000  len=22  off=81
# 7: tag=0x00030000  len=5   off=103
# 8: tag=0x00000000  len=22  off=108
# 9: tag=0x00030000  len=5   off=130
#10: tag=0x00000000  len=22  off=135
#11: tag=0x00030000  len=5   off=157
#12: tag=0x00000000  len=22  off=162
#13: tag=0x00030000  len=5   off=184
#14: tag=0x00000000  len=22  off=189
#15: tag=0x00010000  len=11  off=211
```

**关键性质（⚠️ 仅顺序型文件成立，见 §10）**：在 `yst00000` 这类「顺序型」脚本中
`off[n] + len[n] == off[n+1]` 全程成立。它**不是普适不变量**——池式文件
（约 200/302 个）窗口可重叠，正确密钥下连续性也只有 ~0.70。
跨脚本型的密钥判定用「窗口闭合率」（§10、`YstbFile::key_score`）。

### 4.3 全样本统计

| 项 | 值 |
|---|---|
| 槽位总数 | 402 |
| tag 种类 | 3 |
| `0x00000000` | 201 次 |
| `0x00010000` | 169 次 |
| `0x00030000` | 32 次 |
| 偏移越界 | 0 |

---

## 5. 如何确定密钥（自动猜测方法）

密钥错误的表现：`off[n] + len[n] != off[n+1]`，或 `off + len > content_len`。

因此可以用**偏移连续性**作为适应度函数做搜索：

```rust
/// 评估密钥质量：返回严格衔接的槽位比例
fn score_key(cmds: &[u8], content_len: u32, key: [u8; 4]) -> f32 {
    let mut dec = cmds.to_vec();
    xor_cyclic_at(&mut dec, &key, 0);   // cmds 区整体（相位从 0 开始）
    let n = dec.len() / 12;
    let mut ok = 0;
    let mut prev_end = 0u32;
    for i in 0..n {
        let (_, len, off) = read_slot(&dec, i);
        if off == prev_end && off + len <= content_len { ok += 1; }
        prev_end = off + len;
    }
    ok as f32 / n as f32
}
```

**约束可以大幅缩小搜索空间**（字段下标 2026-09-02 勘误：slot 内布局为
tag@0..4、len@4..8、offset@8..12）：

1. 第一条槽位的 `offset` 恒为 0 → `cipher[8..12] ^ key` 应为 0
   → `key[i] = cipher[8+i]`（i = 0..4）——**普适可用**
2. 若 tag 已知（如 text = `00 00 00 00`）→ `key[i] = cipher[i]`——仅当
   首槽位是 text 时可用

样本验证：cipher 首槽位 = `2b 90 4f 93 | 16 00 00 00 | 00 00 00 00`
- 按约束 1：`key = cipher[8..12] = 2b 90 4f 93` → **正确** ✓
- 按约束 2：`key = cipher[0..4] = 2b 90 4f 93` → **正确** ✓
- ~~旧文曾写「约束 1 = cipher[4..8] = 16 00 00 00 → 不对」~~——那是把 len 字段
  （`16 00 00 00` = 22）误当 offset，索引错误，已更正。

实际做法（Rust `guess_key` 已实现）：两个候选各算
**连续性与窗口闭合率的较大者**（`YstbFile::key_score`），取分高者；
低于阈值报 `Unimplemented`，不要猜。同游戏内密钥统一（278/278），猜出
一个文件的密钥后可直接验证其余文件。

---

## 6. Content 区：变长 VM 字节码（共享池 + 窗口注记）

### 6.1 指令编码 —— **Confirmed**（2026-09-02 全语料验证）

```
instruction = op(u8) + operand_len(u16 LE) + operand(operand_len 字节)
```

自描述变长编码,无需查表即可线性切分。全语料(278 文件 / 506,351 条指令 /
30 种 opcode)非 tag0 窗口**零失败**精确闭合。完整的 30 opcode 表、
变量引用操作数、M-串结构、Whirlpool 锚点比对结果 →
**`docs/opcode/opcode-table.md`**(P1.3 产出)。

首槽位（22 字节,offset 0..22）逐指令切分：

```
56 03 00 | 24 ca 04     push 变量(0x24 空间, 0x04ca)
57 02 00 | 90 01        pushint16 0x0090
42 01 00 | 01           pushint8 0x01
2b 00 00                (0 操作数)
29 01 00 | 00           (1 字节操作数, 语料中恒 0)
                        合计 6+5+4+3+4 = 22 ✓ 精确闭合
```

> ⚠️ 勘误：旧版本此节把下一槽位（#1, tag=0x00030000, len=5, off=22）的
> `4d 02 00 22 22` 也画进了第一槽位（列了 27 字节标 22 字节）。槽位 #1 的
> 内容是独立的 5 字节 M-串窗口 `4d 02 00 | 22 22`（载荷 `""`）。

### 6.2 共享池与窗口（架构修正）

content 区是**共享字节码池**,槽位是池中窗口的注记:

- **顺序型**(74/302,如 yst00000):窗口无缝覆盖全池,连续性 1.0
- **池式**(~200/302,如 yst00034):窗口重叠(同字节被 2~5 窗口引用),
  tag0 文本窗口常是其他窗口的前缀截断
- 非 tag0 窗口正确密钥下**全部**独立闭合;tag0 窗口不保证
- 3 个文件末槽位越界(off==content_len, len>0)——特殊记录,Unknown
- 12 个文件 command_len==0(空脚本)

### 6.3 控制串前缀 `M`（0x4D）

`4d 02 00 22 22` 的结构与 `YSTB_FILE.py` 里的 opt 起始标记一致：

```
YSTB_FILE.py:  4D 0C 00 22 45 53 2E 53 45 4C 2E 53 45 54 22
               M  len  "  E  S  .  S  E  L  .  S  E  T  "
                  ^^^^ = 0x0C = 12 = payload 长度

本样本:        4D 02 00 22 22
               M  len  payload（2 字节 ""）
```

**结构**（Likely）：`0x4D` + `len:u16 LE` + `payload[len]`

---

## 7. 验证方式（可复现）

```python
import struct
y = open('yst00000.ybn','rb').read()   # 已从 bn.ypf 解压
p1, cl, sl = (struct.unpack_from('<I', y, 12)[0],
              struct.unpack_from('<I', y, 16)[0],
              struct.unpack_from('<I', y, 20)[0])
p4 = struct.unpack_from('<I', y, 24)[0]

# 断言 1：分区闭合
assert 0x20 + p1 + cl + sl + p4 == len(y)
# 断言 2：槽位表定长
assert cl % 12 == 0

# 解密
K = bytes.fromhex('2b904f93')
full = bytearray(y)
for i in range(0x20, len(full)):
    full[i] ^= K[i % 4]

cmds = full[0x20+p1 : 0x20+p1+cl]
n = cl // 12
prev_end = 0
contiguous = 0
for i in range(n):
    tag, ln, off = struct.unpack_from('<III', cmds, i*12)
    assert off + ln <= sl, f"slot {i} out of bounds"
    if off == prev_end:
        contiguous += 1
    prev_end = off + ln

print(f"slots={n} contiguous={contiguous} (期望 {n}/{n})")
```

样本期望输出：

```
slots=402 contiguous=402
```

---

## 8. 与既有工具的关系

`YURIS_TOOLS-main/YSTB_FILE.py` 的结构定义与本规格**一致**：

| `YSTB_FILE.py` | 本规格 | 备注 |
|---|---|---|
| `magic` | `magic` | ✓ |
| `version` | `version` | ✓ |
| `unknown1` | `unknown1` | ✓ |
| `part1_len` | `part1_len` | ✓ |
| `command_len` | `command_len` | ✓ |
| `str_len` | `content_len` | **改名**（该区非纯字符串） |
| `part4_len` | `part4_len` | ✓ |
| `unknown2` | `unknown2` | ✓ |
| `YSTB_command.opcode`/`read_len`/`content_offset` | `CommandSlot.tag`/`len`/`offset` | ✓ |

**差异**：该工具在第 3 区按 SJIS 解码取文本。这对 4.xx/5.xx 的部分作品成立，
但对本样本（v555）不成立 —— 该区是字节码。实现时应按 `VersionProfile` 分支处理。

---

## 9. 未确认项（2026-09-02 全语料扫描后更新）

| 项 | 状态 | 影响 |
|---|---|---|
| `unknown1` 语义 | **Unknown**（但 `part1_len == part4_len == 4×unknown1` 302/302 成立） | 无 |
| `part1` / `part4` 语义 | **Unknown**（part1 为小值域 u32 表；part4 呈 4 字节周期模式；已原样保留） | 可能影响 VM |
| slot tag 语义 | **Unknown**（`tag>>16` 分布已实测；「低字节=YSCM 下标」已证伪） | 执行模型 |
| content opcode **名称** | 部分 Likely（见 opcode-table.md）；编码本身已 **Confirmed** | VM 语义 |
| tag0 文本窗口的引擎侧用途 | **Unknown**（对话文本实测在 sc.ypf 明文） | 文本管线 |
| 末槽位越界（3 文件） | **Unknown**（off==content_len, len>0） | 无（解析需容错） |
| 其他版本的槽位表宽度 | **Unknown** | 可能有非 12 字节的变体 |

---

## 10. 2026-09-02 全语料勘误与扩充（302 文件扫描）

**验证方式**：`python3 scripts/probe_opcode_scan2.py "pac/bn.ypf"`
（逐文件密钥恢复 → 逐槽位窗口切分 → 全语料统计）；Rust 侧
`cargo test --test sample`（顺序型/池式/空脚本三类断言）。

### 10.1 勘误

| # | 旧结论 | 更正 | 发现方式 |
|---|---|---|---|
| 1 | 「`16 00 03 00` 作为 u32 LE 即 0x03160000」 | u32 LE = `0x00030016` | 手算复核 |
| 2 | §6 首槽位 22 字节 listing 含下一槽位 5 字节（实列 27） | 22 字节止于 `29 01 00 00`；`4d 02 00 22 22` 是槽位 #1 | 按自描述编码逐指令闭合时暴露 |
| 3 | 「off[n]+len[n]==off[n+1] 全程成立」为密钥判定器 | **仅顺序型文件成立**；池式文件正确密钥下 ~0.70 | 固定密钥扫描 216 文件连续性≠1.0 |
| 4 | §5 约束 1「offset==0 → key=cipher[4..8]」 | offset 在 cipher[**8..12**]；[4..8] 是 len 字段 | 对 yst00034 逐字段验证 |

### 10.2 新增 Confirmed 事实

| # | 事实 | 证据 |
|---|---|---|
| 1 | content 指令编码 = `[op:u8][operand_len:u16 LE][operand]` | 506,351 条指令逐条闭合，0 失败 |
| 2 | 除 0x4d 外每 op 的操作数宽度全语料恒定 | 逐 op 宽度直方图 |
| 3 | 样本游戏密钥统一 `2b904f93`（非逐文件） | 278 文件恢复结果一致 |
| 4 | `part1_len == part4_len == 4 × unknown1` | 302/302 |
| 5 | `part1_len ≡ 0 (mod 4)` ⇒ 命令区 XOR 相位恒 0 | 由 #4 直接导出 |
| 6 | 12 个空脚本（command_len==0） | 逐文件实测 |
| 7 | 语料 30 种 opcode（收敛性检验通过） | 频次表 |
| 8 | 对话文本在 `sc.ypf` 明文 SJIS（本游戏） | 解包直接观察 |

### 10.3 架构修正：共享池 + 窗口注记

content 区是**共享字节码池**；槽位 `(tag,len,offset)` 是池中窗口的注记。
顺序型（74）与池式（~200）两种形态、tag0 前缀截断、窗口重叠等观察详见
`docs/opcode/opcode-table.md` §1。**对 VM 设计的影响**：执行单元不是
「每槽位一段独立程序」，需要按池 + 注记的模型重建脚本层（`yuris-script`
的 Decoder 设计需相应调整）。

### 10.4 U5 解除(2026-09-03):content+part4 = 连续池

- **结论(Confirmed)**:content 与 part4 是**逻辑上一大段** —— `content ‖ part4`
  才是完整的表达式/文本池;窗口的 offset 相对该**拼接段**,而非仅 content。
- **决定性验证**:全部 302 脚本、194,634 个窗口,在 content+part4 拼接后
  `off+len <= ctlen+p4len` **零越界**;744 个"溢出窗口"实为起点恰在 ctlen、
  尾部伸入 part4 的合法窗口(tag 全为 0x00000000=文本窗)。
- `part4_len == 4G` 的"双重性"统一为:part4 是 content 的逻辑尾部,其长度恰被
  分配为 `G×4` 字节 —— **不是**"每组一个 u32 的表"。
- 既有实现 `window_bytes_pooled_copy` 已按"跨 content/part4 边界拼接"读法,
  与本文结论一致,无需改动。
