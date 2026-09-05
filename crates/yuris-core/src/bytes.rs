//! 字节工具：定长整数读取与 XOR 解密。
//!
//! 两个 XOR 都是**样本实测**得到的（见 `docs/formats/ypf.md` / `docs/formats/ystb.md`）：
//!
//! | 函数 | 用途 | 语义 |
//! |---|---|---|
//! | [`xor_cyclic_skip`] | YSTB 加密 | 4 字节循环 XOR，跳过前 `skip` 字节，偏移取**绝对值**取模 |
//! | [`decode_xor_cstring`] | YPF 文件名 | 单字节 XOR，**0x00 不参与**（终止符天然保持为 0） |

use crate::error::{Error, Result};

/// 4 字节循环 XOR，跳过前 `skip` 字节。
///
/// 关键点：相位用的是**数据在文件中的绝对偏移** `i`，不是相对偏移。
/// YSTB 的 header 恰为 0x20 字节（`0x20 ≡ 0 (mod 4)`），所以各分区起始相位都为 0，
/// 但实现上仍按绝对偏移处理，避免将来 header 尺寸变化时踩坑。
///
/// ```text
/// plain[i] = cipher[i] ^ key[i % 4]   (i >= skip)
/// ```
pub fn xor_cyclic_skip(data: &mut [u8], key: &[u8; 4], skip: usize) {
    for (i, b) in data.iter_mut().enumerate().skip(skip) {
        *b ^= key[i & 3];
    }
}

/// 解码 YPF 索引中的文件名：单字节 XOR，遇到 0x00 终止（0x00 不参与 XOR）。
///
/// ```text
/// raw:  ED B0 BA AB A0 A7 95 B0 BA BD F9 F9 F9 FA FD E7 B0 AB A7 00
/// out:  "$ysbin\yst00034.ybn"        (key = 0xC9)
/// ```
pub fn decode_xor_cstring(raw: &[u8], key: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    for &b in raw {
        if b == 0 {
            break;
        }
        out.push(b ^ key);
    }
    out
}

/// 顺序读取器：带位置跟踪与越界检查。
#[derive(Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// 在 `buf` 上建立读取器，初始位置 0。
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// 当前位置。
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// 跳到指定位置。
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// 剩余未读字节。
    pub fn rest(&self) -> &'a [u8] {
        &self.buf[self.pos.min(self.buf.len())..]
    }

    /// 距文件末尾还剩多少字节。
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn check(&self, n: usize) -> Result<()> {
        if self.pos.saturating_add(n) > self.buf.len() {
            return Err(Error::Truncated {
                need: n,
                offset: self.pos,
                have: self.buf.len(),
            });
        }
        Ok(())
    }

    /// 读 `n` 字节并前移位置。
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.check(n)?;
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// 跳过 `n` 字节。
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.check(n)?;
        self.pos += n;
        Ok(())
    }

    /// 读 1 字节。
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    /// 读 2 字节 LE。
    pub fn u16_le(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// 读 4 字节 LE。
    pub fn u32_le(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// 读 4 字节并转成 `[u8; 4]`。
    pub fn u32_bytes(&mut self) -> Result<[u8; 4]> {
        let b = self.bytes(4)?;
        Ok([b[0], b[1], b[2], b[3]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_cyclic_skips_header() {
        // 模拟 YSTB：header 4 字节不动，其后按绝对偏移取模
        let mut d = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xFF, 0xFF, 0xFF, 0xFF];
        let key = [0x0F, 0x00, 0x00, 0x0F];
        xor_cyclic_skip(&mut d, &key, 4);
        assert_eq!(&d[..4], &[0xAA, 0xBB, 0xCC, 0xDD]); // header 不变
        // i=4 -> key[0]=0x0F ; i=5 -> key[1]=0x00 ; i=6 -> key[2]=0x00 ; i=7 -> key[3]=0x0F
        assert_eq!(&d[4..], &[0xF0, 0xFF, 0xFF, 0xF0]);
    }

    #[test]
    fn xor_cyclic_roundtrip() {
        let mut d: Vec<u8> = (0u8..=63).collect();
        let key = [0x2B, 0x90, 0x4F, 0x93];
        let orig = d.clone();
        xor_cyclic_skip(&mut d, &key, 0x20);
        assert_ne!(&d[..], &orig[..]);
        xor_cyclic_skip(&mut d, &key, 0x20);
        assert_eq!(d, orig, "XOR 必须是自反的");
    }

    #[test]
    fn decode_name_stops_at_zero_and_keeps_zero_untouched() {
        // 真实编码方式：明文逐字节 XOR key，随后追加一个**未加密**的 0x00 终止符
        let plain = b"$ysbin\\yst00034.ybn";
        let mut raw: Vec<u8> = plain.iter().map(|&b| b ^ 0xC9).collect();
        raw.push(0x00); // 终止符不参与 XOR
        raw.extend_from_slice(b"trailing garbage");

        let out = decode_xor_cstring(&raw, 0xC9);
        assert_eq!(out, plain.to_vec());

        // 终止符本身绝不会被解码进结果（否则会多出一个 0x00）
        assert!(!out.contains(&0u8));
    }

    #[test]
    fn reader_bounds() {
        let buf = [1u8, 2, 3, 4];
        let mut r = Reader::new(&buf);
        assert_eq!(r.u32_le().unwrap(), 0x0403_0201);
        assert!(r.u8().is_err(), "越界必须报 Truncated");
    }
}
