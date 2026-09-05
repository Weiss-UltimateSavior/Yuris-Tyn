//! 错误类型。
//!
//! 原则：**未知即 `Unimplemented`，不猜**。
//! 这是把「Confirmed / Likely / Hypothesis / Unknown」分级落到代码里的方式。

use thiserror::Error as DeriveError;

/// YurisKernel 统一错误类型。
#[derive(Debug, DeriveError)]
pub enum Error {
    /// 文件 / 网络等 I/O 错误。
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// magic 不匹配。
    #[error("bad magic: expected {expected:?}, got {actual:?}")]
    BadMagic {
        /// 期望的 magic。
        expected: [u8; 4],
        /// 实际读到的 magic。
        actual: [u8; 4],
    },

    /// 结构性格式错误（字段矛盾、越界等）。
    #[error("format: {0}")]
    Format(String),

    /// 数据被截断。
    #[error("truncated: need {need} byte(s) at offset {offset}, buffer has {have}")]
    Truncated {
        /// 需要的字节数。
        need: usize,
        /// 需要读取的起始偏移。
        offset: usize,
        /// 缓冲区实际大小。
        have: usize,
    },

    /// 版本不受支持（应构造对应的 [`crate::VersionProfile`]）。
    #[error("unsupported version: engine={engine} ypf={ypf}")]
    UnsupportedVersion {
        /// 引擎版本号。
        engine: u32,
        /// YPF 格式版本号。
        ypf: u32,
    },

    /// 未实现。**不要用猜测值填充** —— 先标记 Unknown，拿到证据再实现。
    #[error("unimplemented: {0}")]
    Unimplemented(&'static str),

    /// 未解析的 opcode（语义 Unknown）。求值器遇到时不得猜测，交由上层处理。
    #[error("unresolved opcode: {code:#x} at offset {offset}")]
    UnresolvedOpcode {
        /// 原始 opcode 字节。
        code: u32,
        /// 指令在窗口内的偏移。
        offset: usize,
    },
}

impl Error {
    /// 构造 [`Error::Format`]。
    pub fn format(msg: impl Into<String>) -> Self {
        Error::Format(msg.into())
    }
}

/// 统一 `Result` 别名。
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_helper() {
        let e = Error::format("field x is negative");
        assert!(e.to_string().contains("field x is negative"));
    }
}
