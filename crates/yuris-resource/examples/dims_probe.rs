//! 临时侦查:P1 收尾 —— 扫 cg/cgsys_ec/update1 包的 PNG 尺寸分布,
//! 验证「script9 g1092 IF = 1×1 占位图判定」假说(引擎 watch 实测
//! CGINFO 槽13 应答 occ1=1.0 / occ2=1350.0)。
//!
//! 用法: cargo run -p yuris-resource --example dims_probe -- <game_dir>

fn main() {
    let dir = std::env::args().nth(1).expect("game dir");
    let dir = std::path::PathBuf::from(dir);
    for pack in ["cg.ypf", "cgsys_ec.ypf", "update1.ypf"] {
        let path = dir.join("pac").join(pack);
        if !path.is_file() {
            continue;
        }
        let mut stack = yuris_resource::ResourceStack::new(0xC9);
        if let Err(e) = stack.mount(&path) {
            println!("== {pack}: mount 失败 {e}");
            continue;
        }
        let ypf = yuris_format::ypf::YpfIndex::from_path(&path, 0xC9).expect("index");
        let mut ones = vec![];
        let mut w1350 = vec![];
        let mut total = 0usize;
        let mut parsed = 0usize;
        for e in &ypf.entries {
            total += 1;
            let name = String::from_utf8_lossy(&e.name).into_owned();
            if !name.to_ascii_lowercase().ends_with(".png") {
                continue;
            }
            let Ok(data) = stack.read(&name) else { continue };
            if data.len() >= 24 && data[..8] == *b"\x89PNG\r\n\x1a\n" {
                let w = u32::from_be_bytes(data[16..20].try_into().unwrap());
                let h = u32::from_be_bytes(data[20..24].try_into().unwrap());
                parsed += 1;
                if w == 1 && h == 1 {
                    ones.push(name.clone());
                }
                if w == 1350 {
                    w1350.push((name.clone(), h));
                }
            }
        }
        println!("== {pack}: {total} 条目,PNG 头解析 {parsed}");
        println!("   1x1 占位图(前8): {ones:?}");
        println!("   1350 宽(前8): {w1350:?}");
    }
}
