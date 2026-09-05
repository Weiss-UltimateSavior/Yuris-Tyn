//! 版本 Profile。
//!
//! YU-RIS 不同版本在加密、opcode、编码上存在差异（见 `docs/01-runtime-architecture.md` §3）。
//! 版本兼容逻辑**集中**在这里，不要散落到各处硬编码。

/// 字符编码。具体使用哪种由游戏决定（汉化版会把 SJIS 表改成 GBK 表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharEncoding {
    /// SHIFT-JIS / CP932（日文原版）。
    Sjis,
    /// GBK / CP936（汉化版）。
    Gbk,
    /// UTF-8（极少见，预留）。
    Utf8,
}

/// 版本 Profile：一个 YU-RIS 游戏的全部版本相关参数。
#[derive(Debug, Clone)]
pub struct VersionProfile {
    /// 引擎版本号，如 `555`（实测自样本 `yscfg.ybn` / `yst*.ybn` header）。
    pub engine_version: u32,
    /// YPF 格式版本号，如 `500`（实测自 `bn.ypf` header）。
    pub ypf_version: u32,
    /// YPF 索引文件名的单字节 XOR key。
    ///
    /// 样本实测 `0xC9`。是否随版本变化 —— **Unknown**，仅本样本验证。
    pub ypf_name_xor_key: u8,
    /// YSTB 的 4 字节循环 XOR key。
    ///
    /// 密钥**逐游戏不同**。`None` 表示未知，需用
    /// [`crate::bytes`] 配合 YSTB 的连续性判定器猜测
    /// （见 `yuris-format::ystb::guess_key`）。
    pub ystb_xor_key: Option<[u8; 4]>,
    /// 字符编码。
    pub encoding: CharEncoding,
}

impl Default for VersionProfile {
    fn default() -> Self {
        Self {
            engine_version: 0,
            ypf_version: 0,
            // 0xC9 是当前唯一实测值，作为默认是合理的，但**不可当作跨版本事实**
            ypf_name_xor_key: 0xC9,
            ystb_xor_key: None,
            encoding: CharEncoding::Sjis,
        }
    }
}

impl VersionProfile {
    /// 本仓库实测样本的 profile：
    /// `Animal Trail Girlish Square 2`（引擎 555 / YPF 500）。
    ///
    /// 所有字段均有实测证据（见 `PROGRESS.md` 成果 1–3）。
    pub fn sample_v555() -> Self {
        Self {
            engine_version: 555,
            ypf_version: 500,
            ypf_name_xor_key: 0xC9,
            ystb_xor_key: Some([0x2B, 0x90, 0x4F, 0x93]),
            encoding: CharEncoding::Sjis,
        }
    }

    /// YSTB 密钥是否已知。未知时应调用 `yuris-format::ystb::guess_key`。
    pub fn ystb_key_or_none(&self) -> Option<[u8; 4]> {
        self.ystb_xor_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_profile_matches_measurements() {
        let p = VersionProfile::sample_v555();
        assert_eq!(p.engine_version, 555);
        assert_eq!(p.ypf_version, 500);
        assert_eq!(p.ypf_name_xor_key, 0xC9);
        assert_eq!(p.ystb_xor_key, Some([0x2B, 0x90, 0x4F, 0x93]));
    }
}
