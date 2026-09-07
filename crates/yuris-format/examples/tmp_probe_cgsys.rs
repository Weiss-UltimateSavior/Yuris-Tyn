//! 临时探针:用鲁棒 YpfIndex 列出指定包内匹配前缀的条目(flag/长度)。
//! 用法: cargo run -p yuris-format --example tmp_probe_cgsys -- <ypf路径> <前缀> [上限]
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (path, prefix, limit) = match args.as_slice() {
        [_, p, pre] => (p.clone(), pre.clone(), 40usize),
        [_, p, pre, lim] => (p.clone(), pre.clone(), lim.parse().unwrap_or(40)),
        _ => {
            eprintln!("用法: tmp_probe_cgsys <ypf路径> <前缀> [上限]");
            std::process::exit(2);
        }
    };
    let idx = yuris_format::ypf::YpfIndex::from_path(Path::new(&path), 0xC9)
        .expect("YpfIndex 解析失败");
    println!(
        "# {} 条目总数 {}",
        path,
        idx.entries.len()
    );
    let mut shown = 0usize;
    for e in &idx.entries {
        let name = String::from_utf8_lossy(&e.name).to_string();
        if name.to_lowercase().contains(&prefix.to_lowercase()) {
            println!(
                "{:<55} flag=0x{:02X} uncomp={:<9} comp={:<9} off=0x{:X}",
                name, e.flag, e.uncompressed_len, e.compressed_len, e.offset
            );
            shown += 1;
            if shown >= limit {
                println!("(已达上限 {})", limit);
                break;
            }
        }
    }
    println!("# 命中 {} 条", shown);
}
