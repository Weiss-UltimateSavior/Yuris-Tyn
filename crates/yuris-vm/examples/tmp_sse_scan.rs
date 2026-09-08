//! 临时取证工具(P2-1):sysse 音效清单/提取 + bn.ypf 全语料 sse 引用扫描
//! (YSTB 解密 → 池内字节搜索 → 命令组归属 → 整组窗口转储)。
//!
//! 用法: cargo run -p yuris-vm --example tmp_sse_scan -- <游戏目录> <输出目录>

use yuris_format::ypf::YpfArchive;
use yuris_format::ystb::YstbFile;
use yuris_vm::host::PacFileIndex;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, outdir) = (args[1].clone(), args[2].clone());
    std::fs::create_dir_all(&outdir).unwrap();

    let index = PacFileIndex::scan_game_dir(std::path::Path::new(&dir), 0xC9).unwrap();

    // ---- 1. sysse/SE 条目清单 + sse01~06 提取 ----
    let mut sse_keys: Vec<String> = index
        .keys()
        .map(|k| String::from_utf8_lossy(k).into_owned())
        .filter(|k| k.to_ascii_lowercase().contains("sse"))
        .collect();
    sse_keys.sort();
    sse_keys.dedup();
    println!("=== 音频包内 sse 相关条目(剥根键)×{} ===", sse_keys.len());
    for k in &sse_keys {
        println!("  {k}");
    }
    for i in 1..=6u32 {
        let name = format!("sysse/sse{i:02}.ogg");
        match index.read_stored_bytes(&name) {
            Some(b) => {
                let out = format!("{outdir}/sse{i:02}.ogg");
                std::fs::write(&out, &b).unwrap();
                println!("  提取 {name} -> {out} ({} bytes)", b.len());
            }
            None => println!("  提取 {name} -> 未命中"),
        }
    }

    // ---- 2. bn.ypf 全语料 YSTB 解密扫描 "sse" ----
    let bn = std::path::Path::new(&dir).join("pac/bn.ypf");
    let arch = YpfArchive::from_bytes(std::fs::read(&bn).unwrap(), 0xC9).unwrap();
    let mut hits = 0usize;
    println!("\n=== bn.ypf YSTB 池内 \"sse\" 引用(命令组归属) ===");
    for e in arch.entries() {
        if !e.name.ends_with(".ybn") {
            continue;
        }
        let data = match arch.read(&e.name) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if data.len() < 0x20 || &data[0..4] != b"YSTB" {
            continue;
        }
        let Ok((key, score)) = yuris_format::ystb::guess_key(&data) else {
            continue;
        };
        let Ok(y) = YstbFile::from_bytes(&data, key) else {
            continue;
        };
        let (content, part4) = y.pool();
        let mut pool = content.to_vec();
        pool.extend_from_slice(part4);
        let ct_len = content.len() as u32;

        let mut off = 0usize;
        while let Some(rel) = find(&pool[off..], b"sse") {
            let hit = off + rel;
            off = hit + 3;
            hits += 1;
            // 归属:覆盖该池偏移的槽位 → 所属命令组
            let hit32 = hit as u32;
            let mut slot_idx = None;
            for (si, s) in y.slots().iter().enumerate() {
                if s.offset <= hit32 && hit32 < s.offset + s.len {
                    slot_idx = Some(si);
                    break;
                }
            }
            let Some(si) = slot_idx else {
                println!(
                    "[{}] 池偏移 {hit}(ct={ct_len}) 无槽位归属(截断伪影?) ctx={}",
                    e.name,
                    ascii_ctx(&pool, hit)
                );
                continue;
            };
            let groups = y.groups().unwrap_or_default();
            let firsts = y.group_first_slots(&groups);
            let gi = firsts
                .iter()
                .rposition(|&f| f <= si)
                .expect("first_slots 非空");
            let g = groups[gi];
            print!(
                "[{}] 组{gi}(cmd={:#x} n={}) 槽{si} 池偏移 {hit} key={:02x}{:02x}{:02x}{:02x} score={score:.2}\n",
                e.name,
                g.command_type,
                g.window_count,
                key[0], key[1], key[2], key[3],
            );
            for w in y.group_windows(firsts[gi], &g) {
                let bytes = y
                    .window_bytes_pooled_copy(w)
                    .unwrap_or_default();
                let in_p4 = w.offset >= ct_len;
                println!(
                    "    槽(tag={:#010x} off={} len={} {}) = {}",
                    w.tag,
                    w.offset,
                    w.len,
                    if in_p4 { "p4" } else { "ct" },
                    ascii_line(&bytes),
                );
            }
        }
    }
    println!("=== 命中 {hits} 处 ===");

    // ---- 3. sc.ypf 剧本明文扫描(旁证) ----
    let sc = std::path::Path::new(&dir).join("pac/sc.ypf");
    let Ok(arch) = YpfArchive::from_bytes(std::fs::read(&sc).unwrap(), 0xC9) else {
        return;
    };
    println!("\n=== sc.ypf 剧本 \"sse\" 引用 ===");
    for e in arch.entries() {
        if !e.name.ends_with(".txt") {
            continue;
        }
        let Ok(data) = arch.read(&e.name) else { continue };
        let mut i = 0usize;
        while let Some(rel) = find(&data[i..], b"sse") {
            let hit = i + rel;
            i = hit + 3;
            println!("[{}] 偏移 {hit} ctx={}", e.name, ascii_ctx(&data, hit));
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// 命中点上下文(±32 字节,ASCII 可读化)。
fn ascii_ctx(buf: &[u8], at: usize) -> String {
    let s = at.saturating_sub(32);
    let e = (at + 35).min(buf.len());
    ascii_line(&buf[s..e])
}

/// ASCII 可读化:可打印原样,其余 [hex]。
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
