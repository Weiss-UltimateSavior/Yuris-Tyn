//! 哈希函数。
//!
//! [`murmur2`] 是 YU-RIS 引擎的字符串哈希（标签表查找 `FUN_0045124c`），
//! **Confirmed**：YSLB 全部 4153 条标签的 `murmur2(name) == 存储哈希` 零失败
//! （2026-09-03，`probe` 脚本交叉验证）。种子为 0，乘数 0x5BD1E995。

/// 引擎使用的 Murmur2（32 位，seed = 0）。
///
/// 与引擎 `FUN_0045124c` 逐行对齐：`h` 以长度起算（等价 seed^len），
/// 4 字节块混乘 `0x5BD1E995`，尾部 1-3 字节逐位混合，
/// 收尾 `h ^= h>>13; h *= m; h ^= h>>15`。
#[must_use]
pub fn murmur2(data: &[u8]) -> u32 {
    const M: u32 = 0x5BD1_E995;
    let len = data.len();
    let mut h: u32 = len as u32;
    let mut i = 0usize;
    while len - i >= 4 {
        let mut k = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
        i += 4;
    }
    match len - i {
        3 => {
            h ^= u32::from(data[i + 2]) << 16;
            h ^= u32::from(data[i + 1]) << 8;
            h ^= u32::from(data[i]);
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= u32::from(data[i + 1]) << 8;
            h ^= u32::from(data[i]);
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= u32::from(data[i]);
            h = h.wrapping_mul(M);
        }
        _ => {}
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 样本 YSLB 首条标签 `es.BT.W.GET` 的实测存储哈希（probe 实测）。
    #[test]
    fn matches_engine_stored_hash() {
        assert_eq!(murmur2(b"es.BT.W.GET"), 0x0000_DE4A);
    }
}
