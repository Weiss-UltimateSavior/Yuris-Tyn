//! YSLB 标签表（`ysl.ybn`）。
//!
//! 规格：`docs/engine/command-layer.md`（**Confirmed**，引擎 v555）。
//!
//! 证据：引擎 `FUN_00463c7c`（加载）+ `FUN_0045124c`（Murmur2 查找）。
//! 样本 `ysl.ybn`：**4153/4153 条目**，`murmur2(name) == 存储哈希` 零失败，
//! 消费 **139051/139051 精确闭合**（2026-09-03）。
//!
//! ```text
//! [Header 12B]  magic b"YSLB" + version u32 + 标签数 u32
//! [Buckets]     256 × u32（按 hash>>24 分桶的升序标签下标；供引擎查表）
//! [Labels]      标签数 × {
//!     u8  name_len
//!     u8[name_len]  name（ASCII/SJIS，如 "es.BT.W.GET"）
//!     u32 hash      = murmur2(name)
//!     u32 target_pc 目标组下标（GO/GOSUB 跳转目标）
//!     u16 script_id 所属脚本号（yst%05d）
//!     u8  flag_a    引擎存入标签 struct +0xE
//!     u8  flag_b    引擎存入 +0xF（语义 Unknown，原样保留）
//! }
//! ```
//!
//! GO/GOSUB 的跳转目标由此表提供（编译期生成，运行期只查不改）。

use std::collections::HashMap;

use yuris_core::{murmur2, Error, Reader, Result};

/// YSLB magic：`YSLB`。
pub const YSLB_MAGIC: [u8; 4] = *b"YSLB";

/// header 长度（magic 4 + version 4 + count 4）。
pub const HEADER_LEN: usize = 12;

/// 哈希桶数组长度。
pub const BUCKET_COUNT: usize = 256;

/// 一条标签。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YslbLabel {
    /// 标签名（原样字节，ASCII/SJIS）。
    pub name: Vec<u8>,
    /// Murmur2(name)，引擎查找键。
    pub hash: u32,
    /// 目标组下标（脚本内 PC）。
    pub target_pc: u32,
    /// 所属脚本号（`yst%05d.ybn` 的 %05d）。
    pub script_id: u16,
    /// 引擎标签 struct +0xE 字节（语义 Unknown）。
    pub flag_a: u8,
    /// 引擎标签 struct +0xF 字节（语义 Unknown）。
    pub flag_b: u8,
}

/// 已解析的 YSLB。
#[derive(Debug, Clone)]
pub struct YslbTable {
    /// 引擎版本。
    pub version: u32,
    /// 哈希桶头（原样保留；引擎按 hash>>24 取桶头下标做线性扫描）。
    pub buckets: Vec<u32>,
    /// 全部标签。
    pub labels: Vec<YslbLabel>,
    /// 条目流结束位置（样本 == 文件长度）。
    pub consumed: usize,
    /// name → 下标（构建时逐条校验 `murmur2(name) == hash`）。
    index: HashMap<Vec<u8>, usize>,
}

impl YslbTable {
    /// 从**已解压**的 YSLB 字节解析。
    ///
    /// 逐条校验 `murmur2(name) == stored_hash`（引擎查找正确性的决定性判据），
    /// 不符即报错 —— 不猜。
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let magic = r.u32_bytes()?;
        if magic != YSLB_MAGIC {
            return Err(Error::BadMagic {
                expected: YSLB_MAGIC,
                actual: magic,
            });
        }
        let version = r.u32_le()?;
        let count = r.u32_le()? as usize;

        let mut buckets = Vec::with_capacity(BUCKET_COUNT);
        for _ in 0..BUCKET_COUNT {
            buckets.push(r.u32_le()?);
        }

        let mut labels = Vec::with_capacity(count);
        let mut index = HashMap::with_capacity(count);
        for i in 0..count {
            let name_len = r.u8()? as usize;
            let name = r.bytes(name_len)?.to_vec();
            let hash = r.u32_le()?;
            let target_pc = r.u32_le()?;
            let script_id = r.u16_le()?;
            let flag_a = r.u8()?;
            let flag_b = r.u8()?;

            let actual = murmur2(&name);
            if actual != hash {
                return Err(Error::format(format!(
                    "YSLB label #{i} hash mismatch: stored {hash:#010x}, computed {actual:#010x}"
                )));
            }
            index.insert(name.clone(), i);
            labels.push(YslbLabel {
                name,
                hash,
                target_pc,
                script_id,
                flag_a,
                flag_b,
            });
            let _ = i;
        }

        Ok(Self {
            version,
            buckets,
            labels,
            consumed: r.pos(),
            index,
        })
    }

    /// 版本。
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 全部标签。
    pub fn labels(&self) -> &[YslbLabel] {
        &self.labels
    }

    /// 按名字查标签（引擎 `FUN_0045124c` 的等价操作；返回下标）。
    pub fn find(&self, name: &[u8]) -> Option<usize> {
        self.index.get(name).copied()
    }
}
