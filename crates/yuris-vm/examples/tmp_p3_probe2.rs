//! 临时取证工具(P3 补 2):extra 根背景/返回钮/tab 变体确认。
//!
//! 用法: cargo run -p yuris-vm --example tmp_p3_probe2 -- <游戏目录>

use yuris_format::ypf::YpfReader;

fn main() {
    let dir = std::env::args().nth(1).expect("游戏目录");
    let pac = std::path::Path::new(&dir).join("pac");
    let mut cgsys = YpfReader::open(&pac.join("cgsys_ec.ypf"), 0xC9).unwrap();

    let names: Vec<String> = cgsys
        .index()
        .entries
        .iter()
        .map(|e| String::from_utf8_lossy(&e.name).replace('/', "\\"))
        .collect();
    println!("== extra 根与 tab 相关条目 ==");
    for n in &names {
        let low = n.to_ascii_lowercase();
        if (low.contains(r"extra\back") && !low.contains("mode"))
            || low.contains(r"extra\btn_back")
            || low.contains("btn_tab_stmode")
            || low.contains("btn_tab_wpmode")
            || low.contains("btn_tab_svmode")
            || low.contains("btn_tab_rpmode")
            || low.contains("btn_tab_mvmode")
        {
            println!("  {n}");
        }
    }
    println!("== 关键素材尺寸 ==");
    for n in [
        r"cgsys\extra\back.png",
        r"cgsys\extra\btn_back_off.png",
        r"cgsys\extra\btn_tab_cgmode_bt3n.png",
        r"cgsys\extra\btn_tab_cgmode_on.png",
        r"cgsys\extra\btn_tab_bgmmode_bt3n.png",
        r"cgsys\extra\btn_tab_rpmode_bt3n.png",
        r"cgsys\extra\btn_tab_mvmode_bt3n.png",
        r"cgsys\extra\btn_tab_stmode_bt3n.png",
        r"cgsys\extra\btn_tab_wpmode_bt3n.png",
        r"cgsys\extra\btn_tab_svmode_bt3n.png",
    ] {
        match cgsys.read(n.as_bytes()) {
            Ok(d) => {
                let (w, h) = png_dims(&d).unwrap_or((0, 0));
                println!("  {n} = {w}x{h} ({}B)", d.len())
            }
            Err(e) => println!("  {n} = 读取失败({e})"),
        }
    }
}

fn png_dims(d: &[u8]) -> Option<(u32, u32)> {
    if d.len() < 24 || &d[0..4] != b"\x89PNG" {
        return None;
    }
    let w = u32::from_be_bytes([d[16], d[17], d[18], d[19]]);
    let h = u32::from_be_bytes([d[20], d[21], d[22], d[23]]);
    Some((w, h))
}
