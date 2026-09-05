//! YPF 封包解析。
//!
//! 规格：`docs/formats/ypf.md`（**Confirmed**，样本 v500 / 309 条目）。
//!
//! 要点：
//! - header 0x20 字节 + 索引区前置 4 字节 `index_prefix`（用途 Unknown）
//! - entry 从 `0x24` 开始
//! - 文件名：C 字符串，非 0 字节 XOR key，0x00 终止符不加密
//! - 末条 entry 的 tail 可能不足 8 字节（被 `first_data_off` 截断），
//!   解析时按剩余空间钳制，因此解析后位置**精确等于** `first_data_off`

use std::collections::HashMap;
use std::io::Read;

use flate2::read::ZlibDecoder;
use yuris_core::{decode_xor_cstring, Error, Reader, Result};

/// YPF magic：`YPF\0`。
pub const YPF_MAGIC: [u8; 4] = *b"YPF\0";

/// 标志位：条目数据经过 zlib 压缩。
pub const FLAG_ZLIB: u8 = 1;

/// YPF 文件头（不含 `index_prefix`）。
#[derive(Debug, Clone, Copy)]
pub struct YpfHeader {
    /// YPF 格式版本（样本 500）。
    pub version: u32,
    /// 条目数。
    pub file_count: u32,
    /// 首个文件数据的绝对偏移；同时是索引区结束位置。
    pub first_data_off: u32,
}

/// 索引条目。
///
/// > `name_raw_prefix` / `tail` / `reserved` 语义均 **Unknown**，但**原样保留**，
/// > 便于将来回查与回写。
#[derive(Debug, Clone)]
pub struct YpfEntry {
    /// 已解码的内部路径，如 `$ysbin\yst00034.ybn`。
    pub name: String,
    /// 名字首字节的原始值（样本中 303×`0xED`、5×`0xEC`、1×`0xF0`，含义 Unknown）。
    pub name_raw_prefix: u8,
    /// 名字在文件中的绝对偏移（调试用）。
    pub name_offset: usize,
    /// 标志位：1 = zlib 压缩，0 = 原样存储。
    pub flags: u8,
    /// 解压后长度。
    pub uncompressed_len: u32,
    /// 压缩后长度（= 数据区占用字节数）。
    pub compressed_len: u32,
    /// 数据区绝对偏移。
    pub offset: u32,
    /// 保留字段，样本实测恒 0。
    pub reserved: u32,
    /// 条目尾部 8 字节，用途 Unknown，原样保留。
    pub tail: [u8; 8],
    /// 实际读到的 tail 字节数（末条可能 < 8，见模块文档）。
    pub tail_len: usize,
}

/// YPF 封包。
///
/// 当前实现为**内存全量**（样本 bn.ypf 仅 1.4 MB，可接受）。
/// 827 MB 的 cg.ypf 不应整体载入 —— 资源侧请走 `yuris-resource` 的懒加载路径，
/// 那里会改成 seek 式随机访问。
#[derive(Debug)]
pub struct YpfArchive {
    data: Vec<u8>,
    header: YpfHeader,
    index_prefix: u32,
    entries: Vec<YpfEntry>,
    index: HashMap<String, usize>,
}

impl YpfArchive {
    /// 从内存字节解析。
    ///
    /// `name_key` 为文件名 XOR key（样本 `0xC9`）。
    pub fn from_bytes(data: Vec<u8>, name_key: u8) -> Result<Self> {
        let mut r = Reader::new(&data);

        // ---- header（0x00..0x20）----
        let magic = r.u32_bytes()?;
        if magic != YPF_MAGIC {
            return Err(Error::BadMagic {
                expected: YPF_MAGIC,
                actual: magic,
            });
        }
        let version = r.u32_le()?;
        let file_count = r.u32_le()?;
        let first_data_off = r.u32_le()?;

        // 0x10..0x20：样本实测全 0，语义 Unknown，跳过
        r.seek(0x20);

        // ---- 索引区前置 4 字节（用途 Unknown，原样保留）----
        let index_prefix = r.u32_le()?;

        // ---- entries（从 0x24 开始）----
        let mut entries: Vec<YpfEntry> = Vec::with_capacity(file_count as usize);
        let mut index: HashMap<String, usize> = HashMap::with_capacity(file_count as usize);

        for i in 0..file_count {
            let name_offset = r.pos();

            // 名字：C 字符串，非 0 字节 XOR key
            let name_bytes = decode_xor_cstring(r.rest(), name_key);
            let name_len = name_bytes.len();
            let name = String::from_utf8(name_bytes)
                .map_err(|_| Error::format(format!("entry #{i}: name is not ASCII")))?;
            let name_raw_prefix = if name_len > 0 { r.rest()[0] } else { 0 };
            r.skip(name_len + 1)?; // 跳过名字 + NUL 终止符

            let flags = r.u8()?;
            let uncompressed_len = r.u32_le()?;
            let compressed_len = r.u32_le()?;
            let offset = r.u32_le()?;
            let reserved = r.u32_le()?;

            // tail：8 字节，但末条可能被 first_data_off 截断
            let tail_start = r.pos();
            let avail = (first_data_off as usize).saturating_sub(tail_start).min(8);
            let mut tail = [0u8; 8];
            if avail > 0 {
                tail[..avail].copy_from_slice(r.bytes(avail)?);
            }

            let entry = YpfEntry {
                name: name.clone(),
                name_raw_prefix,
                name_offset,
                flags,
                uncompressed_len,
                compressed_len,
                offset,
                reserved,
                tail,
                tail_len: avail,
            };
            index.insert(name, entries.len());
            entries.push(entry);
        }

        // ---- 闭合断言 ----
        if r.pos() != first_data_off as usize {
            return Err(Error::format(format!(
                "index does not close: parsed end = {:#x}, first_data_off = {:#x} (delta {} bytes)",
                r.pos(),
                first_data_off,
                first_data_off as i64 - r.pos() as i64
            )));
        }

        Ok(Self {
            data,
            header: YpfHeader {
                version,
                file_count,
                first_data_off,
            },
            index_prefix,
            entries,
            index,
        })
    }

    /// 文件头。
    pub fn header(&self) -> &YpfHeader {
        &self.header
    }

    /// 索引区前置 4 字节（用途 Unknown）。
    pub fn index_prefix(&self) -> u32 {
        self.index_prefix
    }

    /// 全部条目。
    pub fn entries(&self) -> &[YpfEntry] {
        &self.entries
    }

    /// 按内部路径查找条目。
    pub fn entry(&self, name: &str) -> Option<&YpfEntry> {
        self.index.get(name).map(|&i| &self.entries[i])
    }

    /// 读取一个条目并解压，返回原始字节。
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let e = self
            .entry(name)
            .ok_or_else(|| Error::format(format!("entry not found: {name}")))?;

        let start = e.offset as usize;
        let end = start
            .checked_add(e.compressed_len as usize)
            .ok_or_else(|| Error::format(format!("entry {name}: offset+len overflow")))?;
        let raw = self
            .data
            .get(start..end)
            .ok_or_else(|| Error::format(format!(
                "entry {name}: data range {start:#x}..{end:#x} out of bounds (file {} bytes)",
                self.data.len()
            )))?;

        match e.flags {
            FLAG_ZLIB if e.compressed_len > 0 => {
                let mut out = Vec::with_capacity(e.uncompressed_len as usize);
                ZlibDecoder::new(raw).read_to_end(&mut out)?;
                if out.len() != e.uncompressed_len as usize {
                    return Err(Error::format(format!(
                        "entry {name}: decompressed {} bytes, header says {}",
                        out.len(),
                        e.uncompressed_len
                    )));
                }
                Ok(out)
            }
            FLAG_ZLIB => Ok(Vec::new()),
            0 => Ok(raw.to_vec()),
            other => Err(Error::format(format!(
                "entry {name}: unknown flags value {other:#x} (语义 Unknown，不要猜)"
            ))),
        }
    }
}

/// `YpfIndex` 的单条目信息(P6.3,成果 64)。
///
/// `flag`:bn 型 = 压缩标记(0=stored / 1=zlib);se 型 = 内容类型码
/// (0x02=PNG / 0x06=OGG,数据 stored)。
#[derive(Debug, Clone)]
pub struct YpfEntryInfo {
    /// 规范化条目名(se 型已剥尾缀类型码)。
    pub name: Vec<u8>,
    /// flag(bn=压缩标记 / se=类型码)。
    pub flag: u8,
    /// 解压后长度。
    pub uncompressed_len: u32,
    /// 存储长度(数据区占用字节数)。
    pub compressed_len: u32,
    /// 数据区绝对偏移。
    pub offset: u32,
}

/// 仅索引的轻量 YPF 读取器(资源**存在性**检查用)。
///
/// cg.ypf 等数百 MB 封包不整载:只读 `0x00..first_data_off`(头部 + 索引区),
/// 建立条目名集合。数据区不触碰 —— 内容读取走 [`YpfReader`]
/// 或 [`YpfArchive`]。
/// 名字保留**原始解码字节**(部分封包为 SJIS 日文名,非 UTF-8)。
///
/// ## 条目布局(P6.2,成果 63):条目级自适应
///
/// 实证同一包内(update1.ypf)两种条目布局并存,总开销均为 len+26:
/// - **bn 型**(脚本/文本条目):`name + NUL + flag + uncomp + comp + off +
///   zero + tail8`;flag∈{0=stored, 1=zlib}。
/// - **se 型**(资源条目,PNG/OGG):`name + flag + NUL + uncomp + comp + off +
///   zero + tail8`;flag 紧贴名字尾(rcs 会吞进名字尾),值 = 内容类型码
///   (0x02=PNG / 0x06=OGG 实证),数据 stored 明文。
///
/// 判别:se 型名字尾字节为控制码(<0x20,合法路径名不含);bn 型名字尾为
/// 可打印字符。名字规范化:se 型剥尾缀。
#[derive(Debug)]
pub struct YpfIndex {
    /// 条目名集合(规范化解码字节)。
    pub names: std::collections::HashSet<Vec<u8>>,
    /// 头部。
    pub header: YpfHeader,
    /// 各条目的 flag(bn 型 = 压缩标记 / se 型 = 类型码;顺序同解析序)。
    pub flags: Vec<u8>,
    /// 全部条目信息(顺序同索引区;P6.3)。
    pub entries: Vec<YpfEntryInfo>,
    /// 条目名 → `entries` 下标(首个命中;同名条目取索引区先出现者)。
    pub map: std::collections::HashMap<Vec<u8>, usize>,
}

impl YpfIndex {
    /// 从磁盘文件解析索引区。
    pub fn from_path(path: &std::path::Path, name_key: u8) -> Result<Self> {
        use std::io::{Read, Seek, SeekFrom};

        let mut f = std::fs::File::open(path)?;
        let mut head = [0u8; 0x24];
        f.read_exact(&mut head)?;
        let mut r = Reader::new(&head);
        let magic = r.u32_bytes()?;
        if magic != YPF_MAGIC {
            return Err(Error::BadMagic {
                expected: YPF_MAGIC,
                actual: magic,
            });
        }
        let version = r.u32_le()?;
        let file_count = r.u32_le()?;
        let first_data_off = r.u32_le()?;

        // 索引区(0x24..first_data_off)整体读入后按同一游走逻辑解析
        let index_len = (first_data_off as usize)
            .checked_sub(0x24)
            .ok_or_else(|| Error::format("ypf index: first_data_off < 0x24"))?;
        f.seek(SeekFrom::Start(0x24))?;
        let mut block = vec![0u8; index_len];
        f.read_exact(&mut block)?;

        let mut r = Reader::new(&block);
        let file_len = std::fs::metadata(path)?.len() as u32;
        let mut names = std::collections::HashSet::with_capacity(file_count as usize);
        let mut flags = Vec::with_capacity(file_count as usize);
        let mut entries: Vec<YpfEntryInfo> = Vec::with_capacity(file_count as usize);
        let mut map: std::collections::HashMap<Vec<u8>, usize> =
            std::collections::HashMap::with_capacity(file_count as usize);
        for _ in 0..file_count {
            let mut name_bytes = decode_xor_cstring(r.rest(), name_key);
            let raw_len = name_bytes.len();
            let np = r.pos() + raw_len + 1; // 名字(根前缀+路径+可能尾缀码) + NUL 后
            r.skip(raw_len + 1)?;
            let b0 = block.get(np).copied().unwrap_or(0);
            // 布局判别(成果 63 勘误,probe_census.py 全包实证):
            // 名字段 = 虚拟根字节(1B) + 路径 + [类型码(1B,仅 se 型)];
            // bn 型(name NUL flag uncomp...):flag∈{0,1}(zlib/stored);
            // se 型(name NUL uncomp...):类型码 XOR 后 ∈ {0xCB=PNG, 0xCF=OGG}。
            // 复合判别(2026-09-05 勘误:原实现此处取反,真实 bn 条目
            // (b0∈{0,1} 且 bn 解释落界)全部被错判成 se → sc.ypf 等纯 bn 包
            // 自第 2 条起逐条漂移 1 字节;此前被「se 包走类型码路径 / bn 包走
            // YpfArchive 分区解析器」双巧合掩盖,sc_zlib_text_read 暴露)。
            let tail = name_bytes.last().copied();
            let code = match tail {
                Some(t) if t == 0xCB || t == 0xCF => Some(t),
                _ => None,
            };
            let is_bn = if code.is_none() {
                if b0 <= 1 {
                    let off_a = u32::from_le_bytes(
                        block.get(np + 9..np + 13).unwrap_or(&[0xff; 4]).try_into().unwrap(),
                    );
                    let comp_a = u32::from_le_bytes(
                        block.get(np + 5..np + 9).unwrap_or(&[0xff; 4]).try_into().unwrap(),
                    );
                    first_data_off <= off_a && off_a.saturating_add(comp_a) <= file_len
                } else {
                    false
                }
            } else {
                false
            };
            let (flag, code_stripped) = if is_bn {
                (b0, false)
            } else if let Some(c) = code {
                (c, true)
            } else {
                // se 未知码(如 se.ypf 4 个 2 字节条目):不剥尾,记 Unknown
                (b0, false)
            };
            if code_stripped {
                name_bytes.truncate(raw_len - 1);
            }
            // bn 型:flag 字节占位须先消费(字段在 NUL 后 flag+1 起)
            if is_bn {
                r.skip(1)?;
            }
            let uncompressed_len = r.u32_le()?;
            let compressed_len = r.u32_le()?;
            let offset = r.u32_le()?;
            let _reserved = r.u32_le()?;
            // tail 8 字节(末条可被 first_data_off 截断)
            let avail = (block.len() - r.pos()).min(8);
            r.skip(avail)?;
            flags.push(flag);
            // 双索引:全名(根+路径)与剥根路径并存(引擎侧两种查询形态:
            // YpfScriptHost 用 $ysbin\...(带根);FILEINFO EXIST 用 cg/...(无根))
            map.entry(name_bytes.clone()).or_insert(entries.len());
            if name_bytes.len() > 1 {
                map.entry(name_bytes[1..].to_vec()).or_insert(entries.len());
            }
            entries.push(YpfEntryInfo {
                name: name_bytes.clone(),
                flag,
                uncompressed_len,
                compressed_len,
                offset,
            });
            names.insert(name_bytes);
        }

        Ok(Self {
            names,
            header: YpfHeader {
                version,
                file_count,
                first_data_off,
            },
            flags,
            entries,
            map,
        })
    }
}

/// seek 式单包读取器(P6.3,成果 64):`File` + [`YpfIndex`],按需随机读取。
///
/// cg.ypf 级封包(827MB)不整载 —— 索引一次解析,条目按 `offset` seek 读。
/// flag=1(bn 型)→ zlib 解压;其余 stored 原样(se 型类型码 0x02/0x06 均
/// 明文,实证见成果 63)。
pub struct YpfReader {
    file: std::fs::File,
    index: YpfIndex,
}

impl YpfReader {
    /// 打开封包并解析索引区。
    ///
    /// `name_key` 为文件名 XOR key(样本 `0xC9`)。
    pub fn open(path: &std::path::Path, name_key: u8) -> Result<Self> {
        let index = YpfIndex::from_path(path, name_key)?;
        let file = std::fs::File::open(path)?;
        Ok(Self { file, index })
    }

    /// 索引(条目名/flag/长度/偏移)。
    pub fn index(&self) -> &YpfIndex {
        &self.index
    }

    /// 读取条目(原始字节名,精确匹配)并按 flag 解压。
    pub fn read(&mut self, name: &[u8]) -> Result<Vec<u8>> {
        let e = self
            .index
            .map
            .get(name)
            .map(|&i| &self.index.entries[i])
            .ok_or_else(|| Error::format(format!("entry not found: {:?}", String::from_utf8_lossy(name))))?;
        use std::io::{Read, Seek, SeekFrom};
        self.file.seek(SeekFrom::Start(e.offset as u64))?;
        let mut raw = vec![0u8; e.compressed_len as usize];
        self.file.read_exact(&mut raw)?;
        if e.flag == FLAG_ZLIB {
            let mut out = Vec::with_capacity(e.uncompressed_len as usize);
            ZlibDecoder::new(&raw[..]).read_to_end(&mut out)?;
            if out.len() != e.uncompressed_len as usize {
                return Err(Error::format(format!(
                    "entry {:?}: decompressed {} bytes, header says {}",
                    String::from_utf8_lossy(name),
                    out.len(),
                    e.uncompressed_len
                )));
            }
            Ok(out)
        } else {
            Ok(raw)
        }
    }
}
