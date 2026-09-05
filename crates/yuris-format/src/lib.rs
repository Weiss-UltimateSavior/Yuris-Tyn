//! # yuris-format
//!
//! L0（存储层）+ L1（容器解码层）。
//!
//! | 模块 | 内容 | 证据等级 |
//! |---|---|---|
//! | [`ypf`] | YPF 封包（header + 索引 + zlib 数据） | Confirmed |
//! | [`ystb`] | YSTB 脚本容器（header + 4 分区 + XOR + 命令组/窗口表） | Confirmed |
//! | [`yscm`] | YSCM **命令字典**（121 命令 / 1113 参数 + tail） | Confirmed |
//! | [`yscf`] | YSCF 工程配置（`yscfg.ybn`） | 部分确认 |
//! | [`ysvr`] | YSVR 变量定义表（`ysv.ybn`） | Confirmed |
//! | [`yssd`] | YSSD 系统数据（`save/*.sd`）+ SNP 解码 | Confirmed |
//! | [`yslb`] | YSLB 标签表（`ysl.ybn`，GO/GOSUB 目标） | Confirmed |
//!
//! 规格文档：`docs/formats/ypf.md`、`docs/formats/ystb.md`、`docs/formats/yscm.md`、
//! `docs/engine/command-layer.md`。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod ypf;
pub mod yscf;
pub mod yscm;
pub mod yslb;
pub mod ystb;
pub mod ysvr;
pub mod yssd;

/// crate 版本（与 workspace 同步）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
