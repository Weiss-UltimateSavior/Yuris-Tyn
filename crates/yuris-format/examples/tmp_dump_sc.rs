//! 临时探针:dump sc.ypf 指定条目原文(SJIS 可读化),用毕即删。
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let needle = args.get(2).map(|s| s.as_bytes().to_vec());
    let bytes = std::fs::read(path).unwrap();
    let arch = yuris_format::ypf::YpfArchive::from_bytes(bytes, 0xC9).unwrap();
    for e in arch.entries() {
        if !e.name.ends_with(".txt") {
            continue;
        }
        let data = arch.read(&e.name).unwrap();
        if let Some(nd) = &needle {
            if !windows_bytes(&data).windows(nd.len()).any(|w| w == nd.as_slice()) {
                continue;
            }
        }
        println!("===== {} ({} bytes) =====", e.name, data.len());
        if needle.is_none() && !e.name.ends_with("scenario_start.txt") {
            continue;
        }
        dump(&data);
    }
}

/// 跳过非正文窗口(简版:全量 SJIS 可读化)
fn dump(data: &[u8]) {
    let mut out = String::new();
    for &c in data {
        match c {
            0x0a => out.push('\n'),
            0x20..=0x7e => out.push(c as char),
            _ => out.push_str(&format!("[{c:02x}]")),
        }
    }
    println!("{out}");
}

fn windows_bytes(b: &[u8]) -> &[u8] {
    b
}

#[allow(unused)]
fn _arc(_: Arc<()>) {}
