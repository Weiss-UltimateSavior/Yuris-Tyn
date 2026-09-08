//! 临时取证工具(P2-1 补充):槽位级精确统计 —— 全语料含 "sysse/sseNN"
//! 参数窗口的唯一槽位计数(不受池重叠/组归属伪影影响)。
//!
//! 用法: cargo run -p yuris-vm --example tmp_sse_count -- <游戏目录>

use std::collections::HashMap;

use yuris_format::ypf::YpfArchive;
use yuris_format::ystb::YstbFile;

fn main() {
    let dir = &std::env::args().nth(1).expect("游戏目录");
    let bn = std::path::Path::new(dir).join("pac/bn.ypf");
    let arch = YpfArchive::from_bytes(std::fs::read(&bn).unwrap(), 0xC9).unwrap();
    // sseNN → 唯一 (文件, 槽位下标) 集合
    let mut per_sound: HashMap<String, std::collections::HashSet<(String, usize)>> =
        HashMap::new();
    // 文件 → 含 sse 窗口的槽位数
    for e in arch.entries() {
        if !e.name.ends_with(".ybn") {
            continue;
        }
        let Ok(data) = arch.read(&e.name) else { continue };
        if data.len() < 0x20 || &data[0..4] != b"YSTB" {
            continue;
        }
        let Ok((key, _)) = yuris_format::ystb::guess_key(&data) else { continue };
        let Ok(y) = YstbFile::from_bytes(&data, key) else { continue };
        for (si, s) in y.slots().iter().enumerate() {
            if s.len == 0 {
                continue;
            }
            let Some(bytes) = y.window_bytes_pooled_copy(s) else { continue };
            let mut i = 0usize;
            while let Some(rel) = find(&bytes[i..], b"sysse/sse") {
                let at = i + rel + 9;
                i = at;
                if at + 2 <= bytes.len() {
                    let name = format!("sse{}", String::from_utf8_lossy(&bytes[at..at + 2]));
                    per_sound
                        .entry(name)
                        .or_default()
                        .insert((e.name.clone(), si));
                }
            }
        }
    }
    let mut names: Vec<_> = per_sound.keys().cloned().collect();
    names.sort();
    for n in &names {
        let set = &per_sound[n];
        let files: std::collections::HashSet<_> = set.iter().map(|(f, _)| f.clone()).collect();
        println!("{n}: 唯一参数窗口 {} 个 / {} 文件", set.len(), files.len());
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}
