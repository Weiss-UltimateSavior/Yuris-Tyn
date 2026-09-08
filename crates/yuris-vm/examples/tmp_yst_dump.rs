//! 临时取证工具(P3 补):按文件+关键字 dump YSTB 命令组
//! (es.BT.CG.SET 背景 / es.BT.XY.SET 坐标等)。
//!
//! 用法: cargo run -p yuris-vm --example tmp_yst_dump -- <游戏目录> <yst名子串> <关键字> [文件名子串2...]

use yuris_format::ypf::YpfArchive;
use yuris_format::ystb::YstbFile;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    let yst_sub = &args[2];
    let needles: Vec<&str> = args[3..].iter().map(|s| s.as_str()).collect();

    let bn = std::path::Path::new(dir).join("pac/bn.ypf");
    let arch = YpfArchive::from_bytes(std::fs::read(&bn).unwrap(), 0xC9).unwrap();
    for e in arch.entries() {
        if !e.name.contains(yst_sub.as_str()) || !e.name.ends_with(".ybn") {
            continue;
        }
        let Ok(data) = arch.read(&e.name) else { continue };
        if data.len() < 0x20 || &data[0..4] != b"YSTB" {
            continue;
        }
        let Ok((key, _)) = yuris_format::ystb::guess_key(&data) else { continue };
        let Ok(y) = YstbFile::from_bytes(&data, key) else { continue };
        println!("===== {} =====", e.name);
        let groups = y.groups().unwrap_or_default();
        let firsts = y.group_first_slots(&groups);
        for (gi, g) in groups.iter().enumerate() {
            let wins: Vec<String> = y
                .group_windows(firsts[gi], g)
                .iter()
                .map(|w| ascii_line(&y.window_bytes_pooled_copy(w).unwrap_or_default()))
                .collect();
            let joined = wins.join(" | ");
            if needles.iter().any(|n| joined.contains(n)) {
                println!("组{gi}(cmd={:#x} n={}): {}", g.command_type, g.window_count, joined);
            }
        }
    }
}

fn ascii_line(b: &[u8]) -> String {
    let mut out = String::new();
    for &c in b {
        match c {
            0x20..=0x7e => out.push(c as char),
            _ => out.push_str(&format!("[{c:02x}]")),
        }
    }
    out
}
