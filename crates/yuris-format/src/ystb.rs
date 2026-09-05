//! YSTB 脚本容器解析。
//!
//! 规格：`docs/formats/ystb.md`（**Confirmed**，样本 v555）。
//!
//! 结构：
//!
//! ```text
//! [Header 0x20]      未加密
//! [part1]            加密，语义 Unknown（原样保留）
//! [commands]         加密，定长 12 字节槽位表
//! [content]          加密，变长 VM 字节码（既有工具称 strs，本 crate 改名）
//! [part4]            加密，语义 Unknown（原样保留）
//! ```
//!
//! ## 密钥自动猜测
//!
//! YSTB 的密钥逐游戏不同（同一游戏内统一：样本 278/278 可恢复文件同键）。
//! 槽位表有两个可用的正确性判定器：
//!
//! 1. **连续性**：`off[n] + len[n] == off[n+1]` 全程成立 —— 仅对「顺序型」脚本
//!    （如 yst00000）给出 1.0；「池式」脚本（如 yst00034）存在窗口重叠，正确密钥
//!    下也只有 ~0.7。
//! 2. **窗口闭合率**：content 区遵循自描述变长编码
//!    `[op:u8][operand_len:u16 LE][operand]`（v555 Confirmed，全语料 506,351 条
//!    指令零失败）。非 tag0 窗口在正确密钥下必须精确闭合。
//!
//! 候选生成（`part1_len == 4×unknown1` 恒成立 ⇒ 命令区相位恒为 0，无需旋转）：
//!
//! 1. 候选 1：`cipher[commands 起始 .. +4]`（假设首槽位 `tag == 0`）
//! 2. 候选 2：`cipher[commands 起始+8 .. +12]`（假设首槽位 `offset == 0`）
//!
//! 取两种评分的最大值更高者；都低于阈值则报错，**不要猜**。

use yuris_core::{xor_cyclic_skip, Error, Reader, Result};

/// YSTB magic：`YSTB`。
pub const YSTB_MAGIC: [u8; 4] = *b"YSTB";

/// header 长度（加密区从这之后开始）。
pub const HEADER_LEN: usize = 0x20;

/// 槽位表记录宽度（定长）。
pub const SLOT_SIZE: usize = 12;

/// 命令组表记录宽度（part1 区，每条 4 字节）。
///
/// 证据：引擎加载器 `FUN_00450dfd`（kemonomichi2.exe）+ 全语料 302/302 断言。
/// 详见 `docs/engine/command-layer.md`。
pub const GROUP_SIZE: usize = 4;

/// 已知 tag：正文文本。
pub const TAG_TEXT: u32 = 0x0000_0000;
/// 已知 tag：选项前导（may_be_opt）。
pub const TAG_MAYBE_OPT: u32 = 0x0003_0000;

/// 命令实例（part1 区的一条 u32，**Confirmed**：引擎 v555）。
///
/// - `command_type` = YSCM 命令下标（0..120），运行期索引命令处理器表
/// - `window_count` = 本组参数窗口数；commands 区按 `Σ count*12` 精确消费
/// - `param`（gparam）= 命令参数；声明/调用类命令中编码数组维数位图
///   （GOSUB/RETURN：`int=(u16&0xff)>>3`、`flt=(u16&7)*4+(u16>>14)`、
///   `str=(u16>>9)&0x1f`，引擎 CMDH_004428c0/CMDH_0044b418）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandGroup {
    /// YSCM 命令下标（运行期索引命令处理器表）。
    pub command_type: u8,
    /// 本组参数窗口数；commands 区按 `Σ count*12` 精确消费。
    pub window_count: u8,
    /// 命令参数 u16（声明/调用类命令中编码数组维数位图）。
    pub param: u16,
}

impl CommandGroup {
    /// 从 part1 区第 `i` 条解析。
    pub fn from_part1(part1: &[u8], i: usize) -> Self {
        let o = i * GROUP_SIZE;
        Self {
            command_type: part1[o],
            window_count: part1[o + 1],
            param: u16::from_le_bytes([part1[o + 2], part1[o + 3]]),
        }
    }
}

/// YSTB header（8 字段，样本 v555）。
#[derive(Debug, Clone, Copy)]
pub struct YstbHeader {
    /// 引擎版本（样本 555）。
    pub version: u32,
    /// 语义 **Unknown**（样本 202）。
    pub unknown1: u32,
    /// part1 区长度。
    pub part1_len: u32,
    /// 槽位表长度，**必为 [`SLOT_SIZE`] 的倍数**。
    pub command_len: u32,
    /// 内容区长度。
    pub content_len: u32,
    /// part4 区长度。
    pub part4_len: u32,
    /// 实测恒 0。
    pub unknown2: u32,
}

/// 槽位描述符（定长 [`SLOT_SIZE`] 字节）。
///
/// 命名说明：既有工具 `YURIS_TOOLS-main/YSTB_FILE.py` 把第一个字段叫 `opcode`。
/// 本 crate 改称 `tag`，因为实测表明它更像「内容类型标签」，
/// 真正的 VM 指令 opcode 在 `content` 区的变长字节码里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSlot {
    /// 内容类型标签。
    pub tag: u32,
    /// 内容字节数。
    pub len: u32,
    /// 相对 content 区起始的偏移。
    pub offset: u32,
}

/// 已解析的 YSTB 文件。
#[derive(Debug, Clone)]
pub struct YstbFile {
    header: YstbHeader,
    part1: Vec<u8>,
    slots: Vec<CommandSlot>,
    content: Vec<u8>,
    part4: Vec<u8>,
}

impl YstbFile {
    /// 从**已解压**的 YSTB 字节解析，`key` 为 4 字节循环 XOR 密钥。
    pub fn from_bytes(data: &[u8], key: [u8; 4]) -> Result<Self> {
        let mut r = Reader::new(data);
        let magic = r.u32_bytes()?;
        if magic != YSTB_MAGIC {
            return Err(Error::BadMagic {
                expected: YSTB_MAGIC,
                actual: magic,
            });
        }
        let version = r.u32_le()?;
        let unknown1 = r.u32_le()?;
        let part1_len = r.u32_le()?;
        let command_len = r.u32_le()?;
        let content_len = r.u32_le()?;
        let part4_len = r.u32_le()?;
        let unknown2 = r.u32_le()?;

        if command_len % SLOT_SIZE as u32 != 0 {
            return Err(Error::format(format!(
                "command_len {command_len} is not a multiple of {SLOT_SIZE} (版本变体? 不要猜)"
            )));
        }

        // 分区闭合
        let expect = HEADER_LEN as u64
            + part1_len as u64
            + command_len as u64
            + content_len as u64
            + part4_len as u64;
        if expect != data.len() as u64 {
            return Err(Error::format(format!(
                "section sizes do not sum to file length: {expect} != {}",
                data.len()
            )));
        }

        // 解密（绝对偏移取模，跳过 header）
        let mut dec = data.to_vec();
        xor_cyclic_skip(&mut dec, &key, HEADER_LEN);

        let p1s = HEADER_LEN;
        let p1e = p1s + part1_len as usize;
        let cs = p1e;
        let ce = cs + command_len as usize;
        let xs = ce;
        let xe = xs + content_len as usize;

        let part1 = dec[p1s..p1e].to_vec();
        let commands = dec[cs..ce].to_vec();
        let content = dec[xs..xe].to_vec();
        let part4 = dec[xe..].to_vec();

        // 槽位表
        let mut slots = Vec::with_capacity(command_len as usize / SLOT_SIZE);
        for i in 0..(command_len as usize / SLOT_SIZE) {
            let o = i * SLOT_SIZE;
            let tag = u32::from_le_bytes(commands[o..o + 4].try_into().unwrap());
            let len = u32::from_le_bytes(commands[o + 4..o + 8].try_into().unwrap());
            let offset = u32::from_le_bytes(commands[o + 8..o + 12].try_into().unwrap());
            slots.push(CommandSlot { tag, len, offset });
        }

        Ok(Self {
            header: YstbHeader {
                version,
                unknown1,
                part1_len,
                command_len,
                content_len,
                part4_len,
                unknown2,
            },
            part1,
            slots,
            content,
            part4,
        })
    }

    /// 文件头。
    pub fn header(&self) -> &YstbHeader {
        &self.header
    }

    /// part1 区（语义 Unknown，原样保留）。
    pub fn part1(&self) -> &[u8] {
        &self.part1
    }

    /// part4 区（语义 Unknown，原样保留）。
    pub fn part4(&self) -> &[u8] {
        &self.part4
    }

    /// 槽位表。
    pub fn slots(&self) -> &[CommandSlot] {
        &self.slots
    }

    /// 内容区（变长 VM 字节码）。
    pub fn content(&self) -> &[u8] {
        &self.content
    }

    /// 取某槽位指向的内容切片。
    pub fn slot_content(&self, slot: &CommandSlot) -> Result<&[u8]> {
        let start = slot.offset as usize;
        let end = start + slot.len as usize;
        self.content.get(start..end).ok_or_else(|| {
            Error::format(format!(
                "slot content {start:#x}..{end:#x} out of bounds (content len {})",
                self.content.len()
            ))
        })
    }

    /// 解析 part1 为**命令实例表**（引擎模型，Confirmed）。
    ///
    /// 逐条断言（铁律 2，与引擎加载器一致）：
    /// 1. `part1_len == 4 × unknown1`
    /// 2. `Σ window_count × 12 == command_len`
    ///
    /// 任一不满足即报错 —— 该文件不是 v555 形态的 YSTB（例如 `yst_list.ybn`
    /// 是 YSTL），不要猜。
    pub fn groups(&self) -> Result<Vec<CommandGroup>> {
        let g = self.header.unknown1 as usize;
        if self.part1.len() != g * GROUP_SIZE {
            return Err(Error::format(format!(
                "part1_len {} != 4 * unknown1({g}) —— 非 v555 命令组形态",
                self.part1.len()
            )));
        }
        let mut groups = Vec::with_capacity(g);
        let mut consumed = 0usize;
        for i in 0..g {
            let grp = CommandGroup::from_part1(&self.part1, i);
            consumed += grp.window_count as usize * SLOT_SIZE;
            groups.push(grp);
        }
        if consumed != self.slots.len() * SLOT_SIZE {
            return Err(Error::format(format!(
                "Σ window_count*12 = {consumed} != command_len {}",
                self.slots.len() * SLOT_SIZE
            )));
        }
        Ok(groups)
    }

    /// 第 `group` 组的窗口（连续 [`SLOT_SIZE`] 字节记录，从组的起始槽位开始）。
    ///
    /// `first_slot` 需由调用方按 `Σ count` 累计得出；见 [`Self::group_first_slots`]。
    pub fn group_windows(&self, first_slot: usize, group: &CommandGroup) -> &[CommandSlot] {
        let n = group.window_count as usize;
        &self.slots[first_slot..first_slot + n]
    }

    /// 每组的首槽位下标（引擎加载器的步进模型：`ptr[i+1] = ptr[i] + count*12`）。
    pub fn group_first_slots(&self, groups: &[CommandGroup]) -> Vec<usize> {
        let mut out = Vec::with_capacity(groups.len());
        let mut acc = 0usize;
        for g in groups {
            out.push(acc);
            acc += g.window_count as usize;
        }
        out
    }

    /// 内容池 = content ⊕ part4 逻辑拼接（引擎语义：窗口可伸入 part4；
    /// 样本语料 744 处，全 tag0）。
    ///
    /// 返回 `(content, part4)`；[`Self::window_bytes_pooled`] 按引擎读法跨区取窗。
    pub fn pool(&self) -> (&[u8], &[u8]) {
        (&self.content, &self.part4)
    }

    /// 按引擎读法取窗口字节：先 content，越界部分落进 part4。
    ///
    /// 返回 `None` 表示窗口超出 (content+part4) 总长（真损坏，报错而非猜测）。
    pub fn window_bytes_pooled(&self, slot: &CommandSlot) -> Option<&[u8]> {
        let start = slot.offset as usize;
        let end = start + slot.len as usize;
        let cl = self.content.len();
        let total = cl + self.part4.len();
        if end > total {
            return None;
        }
        if end <= cl {
            return Some(&self.content[start..end]);
        }
        // 跨界：编译期无法借用两段连续内存，仅返回 None 提示跨界；
        // 调用方可用 [`Self::window_bytes_pooled_copy`] 取拷贝。
        None
    }

    /// 同 [`Self::window_bytes_pooled`]，但返回拷贝（跨 content/part4 边界时）。
    pub fn window_bytes_pooled_copy(&self, slot: &CommandSlot) -> Option<Vec<u8>> {
        let start = slot.offset as usize;
        let end = start + slot.len as usize;
        let cl = self.content.len();
        let total = cl + self.part4.len();
        if end > total {
            return None;
        }
        if end <= cl {
            return Some(self.content[start..end].to_vec());
        }
        let mut v = self.content[start..].to_vec();
        v.extend_from_slice(&self.part4[..end - cl]);
        Some(v)
    }

    /// 连续性比例：`off[n] + len[n] == off[n+1]` 且不越界的槽位占比。
    ///
    /// 这是**密钥正确性的判定器之一**。注意：仅「顺序型」脚本（窗口无重叠）能到
    /// 1.0；池式脚本在正确密钥下也只有部分连续。跨脚本型比较请用
    /// [`Self::key_score`]。
    pub fn contiguity_score(&self) -> f32 {
        if self.slots.is_empty() {
            return 0.0;
        }
        let total = self.slots.len() as f32;
        let mut ok = 0usize;
        let mut prev_end = 0u32;
        for s in &self.slots {
            if s.offset == prev_end && s.offset.saturating_add(s.len) <= self.header.content_len {
                ok += 1;
            }
            prev_end = s.offset.saturating_add(s.len);
        }
        ok as f32 / total
    }

    /// 窗口闭合率：非 tag0、非空、不越界的窗口能被 [`segment_window`]
    /// **无残差精确切分**的比例。返回 `(score, 可测窗口数)`。
    ///
    /// 正确密钥下（v555）应为 `(1.0, n)`；n < 5 时评分不可信，调用方应回退
    /// 连续性评分。
    pub fn window_score(&self) -> (f32, usize) {
        let mut testable = 0usize;
        let mut ok = 0usize;
        for s in &self.slots {
            if s.tag == TAG_TEXT || s.len == 0 {
                continue;
            }
            let end = s.offset.saturating_add(s.len) as usize;
            if end > self.content.len() {
                continue;
            }
            testable += 1;
            if segment_window(&self.content[s.offset as usize..end]).is_ok() {
                ok += 1;
            }
        }
        if testable == 0 {
            return (0.0, 0);
        }
        (ok as f32 / testable as f32, testable)
    }

    /// 密钥评分：连续性与窗口闭合率取较大者（窗口样本不足时只信连续性）。
    pub fn key_score(&self) -> f32 {
        let contig = self.contiguity_score();
        let (win, testable) = self.window_score();
        if testable >= 5 {
            contig.max(win)
        } else {
            contig
        }
    }
}

/// content 窗口内的一条原始指令（v555 自描述变长编码，**Confirmed**）。
///
/// 语义（名称/求值规则）尚未逆向，见 `docs/opcode/opcode-table.md`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInstr {
    /// 指令头在窗口内的字节偏移。
    pub offset: usize,
    /// opcode 主码。
    pub op: u8,
    /// 操作数（`operand_len` 字节，宽度由编码自描述）。
    pub operand: Vec<u8>,
}

/// 按 v555 自描述变长编码 `[op:u8][operand_len:u16 LE][operand]` 切分窗口。
///
/// 全语料（278 文件 / 506,351 条指令）验证：非 tag0 窗口零失败。
/// tag0（文本）窗口是共享池中的注记窗口，**不保证**独立闭合，调用方自行排除。
pub fn segment_window(window: &[u8]) -> Result<Vec<RawInstr>> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < window.len() {
        if p + 3 > window.len() {
            return Err(Error::format(format!(
                "instruction header truncated at window offset {p}"
            )));
        }
        let op = window[p];
        let len = u16::from_le_bytes([window[p + 1], window[p + 2]]) as usize;
        let end = p + 3 + len;
        if end > window.len() {
            return Err(Error::format(format!(
                "operand overruns window: op={op:#04x} len={len} end={end} > {}",
                window.len()
            )));
        }
        out.push(RawInstr {
            offset: p,
            op,
            operand: window[p + 3..end].to_vec(),
        });
        p = end;
    }
    Ok(out)
}

/// 自动猜测 YSTB 的 4 字节 XOR 密钥。
///
/// 见模块文档。返回 `(key, score)`，score 为 [`YstbFile::key_score`]；
/// 两个候选都低于阈值则报错，**不要猜**。
pub fn guess_key(data: &[u8]) -> Result<([u8; 4], f32)> {
    let mut r = Reader::new(data);
    let magic = r.u32_bytes()?;
    if magic != YSTB_MAGIC {
        return Err(Error::BadMagic {
            expected: YSTB_MAGIC,
            actual: magic,
        });
    }
    let _version = r.u32_le()?;
    let _unknown1 = r.u32_le()?;
    let part1_len = r.u32_le()?;

    let cmd_off = HEADER_LEN + part1_len as usize;
    if cmd_off + SLOT_SIZE > data.len() {
        return Err(Error::Truncated {
            need: SLOT_SIZE,
            offset: cmd_off,
            have: data.len(),
        });
    }

    // 候选 1：假设首槽位 tag == 0
    let c1: [u8; 4] = data[cmd_off..cmd_off + 4].try_into().unwrap();
    // 候选 2：假设首槽位 offset == 0
    let c2: [u8; 4] = data[cmd_off + 8..cmd_off + 12].try_into().unwrap();

    const THRESHOLD: f32 = 0.9;
    let mut best: Option<([u8; 4], f32)> = None;
    for cand in [c1, c2] {
        let score = match YstbFile::from_bytes(data, cand) {
            Ok(f) => f.key_score(),
            Err(_) => 0.0,
        };
        if best.map_or(true, |(_, s)| score > s) {
            best = Some((cand, score));
        }
    }

    let (key, score) = best.ok_or_else(|| Error::format("no key candidate"))?;
    if score < THRESHOLD {
        return Err(Error::Unimplemented(
            "YSTB xor key not determinable by tag==0 / offset==0 heuristics",
        ));
    }
    Ok((key, score))
}
