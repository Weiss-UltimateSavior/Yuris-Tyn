//! 临时取证工具(P3):load/extra/confirm 素材计数 + 关键素材 PNG 尺寸 +
//! thumb_cg ↔ ev 全图映射抽查(鲁棒 YpfReader,cg.ypf 不整载)。
//!
//! 用法: cargo run -p yuris-vm --example tmp_p3_probe -- <游戏目录>

use yuris_format::ypf::YpfReader;

fn main() {
    let dir = std::env::args().nth(1).expect("游戏目录");
    let pac = std::path::Path::new(&dir).join("pac");

    let mut cgsys = YpfReader::open(&pac.join("cgsys_ec.ypf"), 0xC9).unwrap();
    let mut cg = YpfReader::open(&pac.join("cg.ypf"), 0xC9).unwrap();

    // ---- 计数(包内名为反斜杠分隔 + 虚拟根前缀) ----
    let names: Vec<String> = cgsys
        .index()
        .entries
        .iter()
        .map(|e| String::from_utf8_lossy(&e.name).replace('/', "\\"))
        .collect();
    let count = |p: &str| names.iter().filter(|n| n.contains(p)).count();
    println!(
        "extra bgmmode\\btn_m*={} tip_m*={} cgmode\\back*={} vomode\\back*={} tab_cgmode*={} tab_bgmmode*={}",
        count(r"bgmmode\btn_m"),
        count(r"bgmmode\title\tip_m"),
        count(r"cgmode\back"),
        count(r"vomode\back"),
        count(r"btn_tab_cgmode"),
        count(r"btn_tab_bgmmode"),
    );
    println!(
        "saveload back_load*={} btn_back*={} btn_page*={} btn_plate*={} load\\tab*={}",
        count(r"back_load"),
        count(r"saveload\btn_back"),
        count(r"saveload\btn_page"),
        count(r"saveload\btn_plate"),
        count(r"saveload\load\btn_tab"),
    );
    println!(
        "confirm dialog_end*={} dialog_title*={} btn_yes*={} btn_no*={}",
        count(r"confirm\dialog_end"),
        count(r"confirm\dialog_title"),
        count(r"confirm\btn_yes"),
        count(r"confirm\btn_no"),
    );
    println!(
        "== 相关条目全列(tab/back/yes/no) =="
    );
    for n in &names {
        if n.contains("btn_tab_cgmode") || n.contains("btn_tab_bgmmode")
            || n.contains(r"cgmode\back") || n.contains(r"saveload\btn_back")
            || n.contains(r"confirm\btn_yes") || n.contains(r"confirm\btn_no")
        {
            println!("  {n}");
        }
    }

    // ---- 关键素材尺寸(PNG IHDR) ----
    println!("== 关键素材尺寸 ==");
    for n in [
        r"cgsys\saveload\back_load.png",
        r"cgsys\saveload\btn_back_off.png",
        r"cgsys\saveload\load\btn_tab_load_on.png",
        r"cgsys\confirm\dialog_end.png",
        r"cgsys\confirm\btn_yes_off.png",
        r"cgsys\confirm\btn_no_off.png",
        r"cgsys\extra\bgmmode\back.png",
        r"cgsys\extra\cgmode\btn_plate_back_bt3.png",
    ] {
        match cgsys.read(n.as_bytes()) {
            Ok(d) => {
                let (w, h) = png_dims(&d).unwrap_or((0, 0));
                println!("  {n} = {w}x{h} ({}B)", d.len())
            }
            Err(_) => println!("  {n} = 读取失败"),
        }
    }

    // ---- cg.ypf thumb/ev 计数 + 映射抽查 ----
    let mut thumbs: Vec<String> = Vec::new();
    let mut evs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for e in &cg.index().entries {
        let n = String::from_utf8_lossy(&e.name).replace('/', "\\");
        if n.contains("thumb_cg") && n.ends_with(".png") {
            thumbs.push(n.clone());
        }
        if n.contains("\\ev\\") && n.ends_with(".png") {
            evs.insert(n);
        }
    }
    thumbs.sort();
    println!("== cg.ypf: thumb_cg={} ev={} ==", thumbs.len(), evs.len());
    let sample = thumbs.iter().take(6).cloned().collect::<Vec<_>>().join(" | ");
    println!("  thumb 前 6: {sample}");
    let mut hit = 0usize;
    for t in thumbs.iter().take(200) {
        let base = t.rsplit('\\').next().unwrap();
        if evs.iter().any(|e| e.ends_with(&format!(r"\{base}"))) {
            hit += 1;
        }
    }
    println!("  thumb→cg\\ev\\同名 直映命中 {}/{}(抽样 200)", hit, thumbs.len().min(200));
}

/// PNG IHDR 尺寸(字节 16..24:宽高各 u32 BE)。
fn png_dims(d: &[u8]) -> Option<(u32, u32)> {
    if d.len() < 24 || &d[0..4] != b"\x89PNG" {
        return None;
    }
    let w = u32::from_be_bytes([d[16], d[17], d[18], d[19]]);
    let h = u32::from_be_bytes([d[20], d[21], d[22], d[23]]);
    Some((w, h))
}
