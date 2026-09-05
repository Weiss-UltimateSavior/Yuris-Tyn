//! yuris-cli
//!
//! 播放器入口。
//!
//! > 本 crate 目前为**骨架**阶段，仅定义模块边界，不含业务实现。
//! > 实现顺序见 `docs/03-phase1-plan.md`。
//! > 所有未确认的行为一律返回 `Error::Unimplemented`，**禁止猜测**。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// 二进制入口：src/main.rs（待实现）

/// crate 版本（与 workspace 同步）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
