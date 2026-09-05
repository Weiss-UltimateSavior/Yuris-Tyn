//! YSCM 命令字典（`ysc.ybn`）。
//!
//! 规格：`docs/formats/yscm.md`。
//!
//! **这是整个项目的关键数据源**：它就是 YU-RIS 脚本语言的命令表，
//! 包含全部命令名、每个命令的参数名与参数类型码。
//! P1.3（opcode 交叉比对）以它为字典。
//!
//! 结构（Confirmed，样本 121 条命令 / 1113 个参数）：
//!
//! ```text
//! [Header 0x10]  magic + version + command_count + unknown
//! [Body]         command_count × {
//!                    name\0
//!                    u8 param_count
//!                    param_count × { param_name\0  u16 param_type }
//!                }
//! [Tail]         SJIS 消息表 + 分词器字符表（结构部分 Unknown）
//! ```

use std::collections::HashMap;

use yuris_core::{Error, Reader, Result};

/// YSCM magic：`YSCM`。
pub const YSCM_MAGIC: [u8; 4] = *b"YSCM";

/// header 长度（magic + version + command_count + unknown）。
pub const HEADER_LEN: usize = 0x10;

/// 一条命令的某个参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YscmParam {
    /// 参数名，可能为空（如 `VAR :3`）。
    pub name: String,
    /// 类型码。语义 **Unknown**（样本中观测到 0,1,2,3 与大量 0xN00 值）。
    pub ty: u16,
}

/// 一条 YU-RIS 脚本命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YscmCommand {
    /// 命令名，如 `CG` / `TEXT` / `MATH`。
    pub name: String,
    /// 参数表（按声明顺序）。
    pub params: Vec<YscmParam>,
    /// 该命令条目在 YSCM 内的起始偏移（调试用）。
    pub offset: usize,
}

/// YSCM tail 的解析结果（**Confirmed**：引擎 `FUN_0046305c` 按同一模型消费）。
///
/// tail = **37** 个 C 串（引擎拷入 `DAT_00667fc0`，实测为 CRT 错误消息，SJIS）
///      + 256 字节表（`DAT_00667d40`，疑似 errno→消息下标映射，语义 Likely）。
/// 样本：37 串消耗 789 字节 + 256 表 = 1045，**尾部无剩余**。
///
/// 此前「tail = 系统变量表 / 配置键」为**误判**（已进 PROGRESS.md 勘误）：
/// 配置键是 SYSTEMMODE 的参数名，在 body 内。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YscmTail {
    /// 37 个 C 串（原样 SJIS 字节；引擎 do-while `i<0x91, i+=4` 恰 37 次）。
    pub messages: Vec<Vec<u8>>,
    /// 256 字节表。
    pub table: [u8; 256],
    /// 剩余字节（样本为空；非空时语义 Unknown，原样保留）。
    pub rest: Vec<u8>,
}

/// 已解析的 YSCM。
#[derive(Debug, Clone)]
pub struct YscmTable {
    /// 引擎版本。
    pub version: u32,
    /// 命令表。
    pub commands: Vec<YscmCommand>,
    /// header 第二个 u32（样本 0，语义 Unknown）。
    pub unknown: u32,
    /// body 结束位置（tail 起点）。
    pub tail_offset: usize,
    /// tail 原始字节（含 SJIS 消息表与分词器表；结构 Unknown，原样保留）。
    pub tail: Vec<u8>,
    /// 命令名 → 下标。
    index: HashMap<String, usize>,
}

impl YscmTable {
    /// 从**已解压**的 YSCM 字节解析。
    ///
    /// 解析 `command_count` 条后停止；剩余字节原样存入 [`Self::tail`]，
    /// **不做猜测性解析**。
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let magic = r.u32_bytes()?;
        if magic != YSCM_MAGIC {
            return Err(Error::BadMagic {
                expected: YSCM_MAGIC,
                actual: magic,
            });
        }
        let version = r.u32_le()?;
        let command_count = r.u32_le()?;
        let unknown = r.u32_le()?;

        let mut commands = Vec::with_capacity(command_count as usize);
        let mut index = HashMap::with_capacity(command_count as usize);

        for i in 0..command_count {
            let offset = r.pos();
            let name = read_cstring(&mut r)?;
            let param_count = r.u8()? as usize;

            let mut params = Vec::with_capacity(param_count);
            for _ in 0..param_count {
                let pname = read_cstring(&mut r)?;
                let ty = r.u16_le()?;
                params.push(YscmParam {
                    name: String::from_utf8_lossy(&pname).into_owned(),
                    ty,
                });
            }

            let name = String::from_utf8_lossy(&name).into_owned();
            index.insert(name.clone(), commands.len());
            commands.push(YscmCommand {
                name,
                params,
                offset,
            });
            let _ = i;
        }

        let tail_offset = r.pos();
        let tail = r.rest().to_vec();

        Ok(Self {
            version,
            commands,
            unknown,
            tail_offset,
            tail,
            index,
        })
    }

    /// 引擎版本。
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 全部命令。
    pub fn commands(&self) -> &[YscmCommand] {
        &self.commands
    }

    /// 按名字查命令。
    pub fn command(&self, name: &str) -> Option<&YscmCommand> {
        self.index.get(name).map(|&i| &self.commands[i])
    }

    /// 按下标查命令。
    pub fn command_at(&self, i: usize) -> Option<&YscmCommand> {
        self.commands.get(i)
    }

    /// 统计信息（trace / 文档用）。
    pub fn param_total(&self) -> usize {
        self.commands.iter().map(|c| c.params.len()).sum()
    }

    /// 按**引擎消费模型**解析 tail（`FUN_0046305c` do-while：恰好 **37** 个 C 串
    /// + 256 字节表）。
    ///
    /// 注意：引擎循环是先解析后判 `i += 4 < 0x91`，即 37 次（0..=0x90 step 4）。
    /// 按「35 次」的朴素读法会恰好闭合（785+256+4=1045）但**是错的**——
    /// 第 35/36 条是单空格串，会被误并入 256 字节表（教训已录勘误）。
    pub fn parse_tail(&self) -> Result<YscmTail> {
        // 引擎 do-while 的逐条镜像：先解析再计步
        let tail = &self.tail;
        let mut pos = 0usize;
        let mut step = 0usize;
        let mut messages = Vec::new();
        loop {
            let end = tail[pos..]
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| Error::format("unterminated C string in YSCM tail"))?
                + pos;
            messages.push(tail[pos..end].to_vec());
            pos = end + 1;
            step += 4;
            if step >= 0x91 {
                break;
            }
        }
        if tail.len() < pos + 256 {
            return Err(Error::format(format!(
                "YSCM tail too short: need {} bytes for the 256-byte table, have {}",
                pos + 256,
                tail.len()
            )));
        }
        let mut table = [0u8; 256];
        table.copy_from_slice(&tail[pos..pos + 256]);
        pos += 256;
        Ok(YscmTail {
            messages,
            table,
            rest: tail[pos..].to_vec(),
        })
    }
}

fn read_cstring<'a>(r: &mut Reader<'a>) -> Result<Vec<u8>> {
    let rest = r.rest();
    let len = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::format("unterminated C string in YSCM"))?;
    let s = rest[..len].to_vec();
    r.skip(len + 1)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用真实样本构造的最小断言（完整集成测试见 tests/sample.rs）。
    #[test]
    fn parses_synthetic_yscm() {
        // ALIAS(0 参数) + CG(1 参数: ID, 类型 1)
        let mut d = Vec::new();
        d.extend_from_slice(b"YSCM");
        d.extend_from_slice(&555u32.to_le_bytes());
        d.extend_from_slice(&2u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(b"ALIAS\0");
        d.push(0);
        d.extend_from_slice(b"CG\0");
        d.push(1);
        d.extend_from_slice(b"ID\0");
        d.extend_from_slice(&1u16.to_le_bytes());

        let t = YscmTable::from_bytes(&d).unwrap();
        assert_eq!(t.version(), 555);
        assert_eq!(t.commands().len(), 2);
        assert_eq!(t.param_total(), 1);
        assert_eq!(t.command("CG").unwrap().params[0].name, "ID");
        assert_eq!(t.command("CG").unwrap().params[0].ty, 1);
        assert!(t.command("NOPE").is_none());
        assert!(t.tail.is_empty());
    }

    #[test]
    fn rejects_bad_magic() {
        let d = b"NOPE\0\0\0\0\0\0\0\0\0\0\0\0";
        assert!(YscmTable::from_bytes(d).is_err());
    }
}
