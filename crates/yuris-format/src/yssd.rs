//! YSSD 系统数据文件（`save/*.sd`）+ SNP（snappy 变体）解码。
//!
//! 规格（**Confirmed**，2026-09-05 侦查，样本 = 本作 save/ 六文件 28 块全解码）：
//!
//! ```text
//! [Header 0x1010B]  magic b"YSSD" + version u32(0x1E0) + 块数 u32
//!                   + 偏移表容量 u32(0x400) + 偏移表 0x400 × u32
//!                   （表项[i] = 块号 i+1 的绝对偏移；0 = 空）
//! [Block]           u32 块号(=DNO) + u8 类型(0=普通) + u8 严格维数标志
//!                   + u16 目标变量 id + u32 压缩长 + u32 未压长
//!                   + SNP 压缩载荷
//! [载荷(解压后)]     u32 类型(1=INT 2=FLT 3=STR) + u32 维数 + u32[维数] 边界
//!                   + u32 数据长 + 数据（INT/FLT = 8B/元素；STR = 逐元素
//!                   {u32 长度 + 字节}，元素数 = 维数积）
//! ```
//!
//! 引擎证据：LOAD 处理器 `FUN_00444648`（文件分支 fseek `DNO*4+0xC` =
//! 偏移表[DNO-1]，块头 +4 跳过块号；magic/版本校验）；解压选择
//! `DAT_0059b718 != 0` → SNP（`YSSNP.DLL!YSSnp_Uncompress`）；写回
//! `FUN_0044564d`（类型一致 + 严格维数匹配 + memcpy 到描述符数据区）。
//! 本构建实测载荷全部 SNP（zlib 头校验失败、SNP 全量闭合）。
//!
//! SNP = **snappy 变体**：头部 varint 未压长度（7-bit LE base-128）；
//! 元素 tag 低 2 位 00=字面量 / 01=copy1 / 10=copy2 / 11=copy4；
//! **字面量长度 = (tag>>2)+1**（非标准 snappy 的 tag>>2，引擎查表
//! `DAT_1000ba40`/`FUN_10001d40` 实锤）；长字面量（tag>>2 ∈ 60..63）
//! 后随 1..4 字节小端长度（同样 +1）；copy 族与标准 snappy 相同。

use yuris_core::{Error, Result};

/// YSSD magic：`YSSD`。
pub const YSSD_MAGIC: [u8; 4] = *b"YSSD";

/// 偏移表容量（样本恒 0x400）。
pub const OFFSET_TABLE_LEN: usize = 0x400;

/// 一个 YSSD 块（压缩形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct YssdBlock {
    /// 块号（= DNO，1 基；脚本 LOAD 槽 2）。
    pub dno: u32,
    /// 块类型（0 = 普通单块；2 = 多子块 —— 语料未出现，不实现）。
    pub btype: u8,
    /// 严格维数标志（1 = 载荷维数必须与变量声明完全一致）。
    pub strict: u8,
    /// 目标变量 id（与脚本槽 3 引用一致；双重记录）。
    pub var_id: u16,
    /// SNP 压缩载荷。
    pub compressed: Vec<u8>,
}

/// 已解析的 YSSD 文件。
#[derive(Debug, Clone)]
pub struct YssdFile {
    /// 版本（样本 0x1E0）。
    pub version: u32,
    /// 块表（按出现顺序；DNO → 块经 `block()` 查找）。
    blocks: Vec<YssdBlock>,
    /// DNO → blocks 下标（偏移表展开）。
    index: Vec<Option<u32>>,
}

impl YssdFile {
    /// 从字节解析（header + 偏移表 + 全部块头）。
    ///
    /// 块数据区整体读取（块间可能共享尾部容差 —— 逐块按压缩长切分）。
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 0x10 {
            return Err(Error::format("YSSD too short"));
        }
        if &data[0..4] != &YSSD_MAGIC {
            return Err(Error::BadMagic {
                expected: YSSD_MAGIC,
                actual: data[0..4].try_into().unwrap(),
            });
        }
        let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
        if version != 0x1E0 {
            return Err(Error::format(format!(
                "YSSD version {version:#x} != 0x1E0 (引擎 0x1a608 同族)"
            )));
        }
        let table_end = 0x10 + OFFSET_TABLE_LEN * 4;
        if data.len() < table_end {
            return Err(Error::format("YSSD offset table truncated"));
        }
        let mut blocks = Vec::new();
        let mut index = vec![None::<u32>; OFFSET_TABLE_LEN];
        for (i, off) in data[0x10..table_end]
            .chunks_exact(4)
            .enumerate()
            .map(|(i, c)| (i, u32::from_le_bytes(c.try_into().unwrap())))
        {
            if off == 0 {
                continue;
            }
            let off = off as usize;
            if off + 0x10 > data.len() {
                return Err(Error::format(format!(
                    "YSSD block[{}] offset {off:#x} out of range",
                    i + 1
                )));
            }
            let dno = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            let btype = data[off + 4];
            let strict = data[off + 5];
            let var_id = u16::from_le_bytes(data[off + 6..off + 8].try_into().unwrap());
            let clen = u32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap()) as usize;
            if off + 0x10 + clen > data.len() {
                return Err(Error::format(format!(
                    "YSSD block[{}] payload truncated (need {clen}B)",
                    i + 1
                )));
            }
            index[i] = Some(blocks.len() as u32);
            blocks.push(YssdBlock {
                dno,
                btype,
                strict,
                var_id,
                compressed: data[off + 0x10..off + 0x10 + clen].to_vec(),
            });
        }
        Ok(Self { version, blocks, index })
    }

    /// 按 DNO（1 基）取块。
    pub fn block(&self, dno: u32) -> Option<&YssdBlock> {
        if dno == 0 || dno as usize > OFFSET_TABLE_LEN {
            return None;
        }
        self.index
            .get((dno - 1) as usize)
            .and_then(|s| *s)
            .and_then(|i| self.blocks.get(i as usize))
    }

    /// 块总数。
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

/// SNP（snappy 变体）解压。
///
/// 字面量长度 = `(tag>>2)+1`；copy1/2/4 与标准 snappy 一致
///（YSSNP.DLL `FUN_10001d40` + 派发表 `DAT_1000ba40` 实锤）。
pub fn snp_uncompress(src: &[u8]) -> Result<Vec<u8>> {
    // varint 未压长度（最多 5 字节）
    let mut expected: u64 = 0;
    let mut p = 0;
    let mut shift = 0;
    let mut done = false;
    while p < src.len() && shift < 35 {
        let b = src[p];
        p += 1;
        expected |= ((b & 0x7F) as u64) << shift;
        if b < 0x80 {
            done = true;
            break;
        }
        shift += 7;
    }
    if !done {
        return Err(Error::format("SNP varint header missing"));
    }
    let expected = expected as usize;
    let mut out = Vec::with_capacity(expected);
    while p < src.len() {
        let tag = src[p];
        match tag & 3 {
            0 => {
                // 字面量:长度 = (tag>>2)+1;60..63 → 1..4 额外长度字节(同样 +1)
                let mut n = (tag >> 2) as usize + 1;
                p += 1;
                if n > 0x3C {
                    let extra = n - 1 - 59;
                    if p + extra > src.len() {
                        return Err(Error::format("SNP literal length truncated"));
                    }
                    let mut v: usize = 0;
                    for (k, b) in src[p..p + extra].iter().enumerate() {
                        v |= (*b as usize) << (8 * k);
                    }
                    p += extra;
                    n = v + 1;
                }
                if p + n > src.len() {
                    return Err(Error::format("SNP literal truncated"));
                }
                out.extend_from_slice(&src[p..p + n]);
                p += n;
            }
            1 => {
                // copy1:len = 4 + ((tag>>2)&7);offset = ((tag>>5)<<8)|下一字节
                if p + 2 > src.len() {
                    return Err(Error::format("SNP copy1 truncated"));
                }
                let n = 4 + ((tag >> 2) & 7) as usize;
                let off = (((tag >> 5) as usize) << 8) | src[p + 1] as usize;
                p += 2;
                copy_back(&mut out, off, n)?;
            }
            2 => {
                // copy2:len = (tag>>2)+1;offset = u16 LE
                if p + 3 > src.len() {
                    return Err(Error::format("SNP copy2 truncated"));
                }
                let n = (tag >> 2) as usize + 1;
                let off = u16::from_le_bytes([src[p + 1], src[p + 2]]) as usize;
                p += 3;
                copy_back(&mut out, off, n)?;
            }
            _ => {
                // copy4:len = (tag>>2)+1;offset = u32 LE
                if p + 5 > src.len() {
                    return Err(Error::format("SNP copy4 truncated"));
                }
                let n = (tag >> 2) as usize + 1;
                let off = u32::from_le_bytes(src[p + 1..p + 5].try_into().unwrap()) as usize;
                p += 5;
                copy_back(&mut out, off, n)?;
            }
        }
    }
    if out.len() != expected {
        return Err(Error::format(format!(
            "SNP length mismatch: got {}, header says {expected}",
            out.len()
        )));
    }
    Ok(out)
}

/// 回引用拷贝（offset 相对 out 末尾；逐字节 —— 引擎语义允许重叠）。
fn copy_back(out: &mut Vec<u8>, off: usize, n: usize) -> Result<()> {
    if off == 0 || off > out.len() {
        return Err(Error::format(format!(
            "SNP copy offset {off} out of range (out len {})",
            out.len()
        )));
    }
    for _ in 0..n {
        let b = out[out.len() - off];
        out.push(b);
    }
    Ok(())
}

/// 解压后的 YSSD 载荷（`FUN_0044564d` 消费形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct YssdPayload {
    /// 类型：1=INT 2=FLT 3=STR（须与目标变量描述符类型一致）。
    pub ty: u8,
    /// 维数（0 = 标量）。
    pub dims: Vec<u32>,
    /// 数据（INT/FLT = 8B/元素；STR = 逐元素 {u32 长度 + 字节}）。
    pub data: Vec<u8>,
}

impl YssdPayload {
    /// 从解压后的字节解析。
    pub fn from_bytes(raw: &[u8]) -> Result<Self> {
        if raw.len() < 12 {
            return Err(Error::format("YSSD payload too short"));
        }
        let ty = u32::from_le_bytes(raw[0..4].try_into().unwrap()) as u8;
        let dim = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
        if dim > 8 {
            return Err(Error::format(format!(
                "YSSD payload dim count {dim} > 8 (引擎 900000 同族)"
            )));
        }
        if raw.len() < 8 + dim * 4 + 4 {
            return Err(Error::format("YSSD payload dims truncated"));
        }
        let dims = raw[8..8 + dim * 4]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let dlen = u32::from_le_bytes(
            raw[8 + dim * 4..12 + dim * 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let data_start = 12 + dim * 4;
        if raw.len() < data_start + dlen {
            return Err(Error::format("YSSD payload data truncated"));
        }
        Ok(Self {
            ty,
            dims,
            data: raw[data_start..data_start + dlen].to_vec(),
        })
    }
}
