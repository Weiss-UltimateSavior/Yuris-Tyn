//! # yuris-core
//!
//! YurisKernel 的地基 crate：错误类型、字节工具、版本 Profile。
//!
//! 设计约束：
//! - **无业务逻辑**。本 crate 被所有 crate 依赖，保持最小与稳定。
//! - 所有「未确认」的行为一律返回 [`Error::Unimplemented`]，**禁止猜测**。
//! - 实现顺序见 `docs/03-phase1-plan.md`。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod bytes;
pub mod error;
pub mod hash;
pub mod version;

pub use bytes::{decode_xor_cstring, xor_cyclic_skip, Reader};
pub use error::{Error, Result};
pub use hash::murmur2;
pub use version::{CharEncoding, VersionProfile};

/// crate 版本（与 workspace 同步）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
