//! 临时取证工具:用播放器同路径(read_image_bytes)提取标题按钮素材。
//!
//! 用法: cargo run -p yuris-vm --example tmp_extract -- <游戏目录> <输出目录>

use yuris_vm::host::PacFileIndex;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, outdir) = (args[1].clone(), args[2].clone());
    let index = PacFileIndex::scan_game_dir(std::path::Path::new(&dir), 0xC9).unwrap();
    for name in [
        "cgsys/title/btn_start_off.png",
        "cgsys/title/btn_start_on.png",
        "cgsys/title/btn_start_over.png",
        "cgsys/title/btn_end_off.png",
        "cgsys/title/btn_end_on.png",
        "cgsys/title/btn_end_over.png",
        "cgsys/title/btn_lastload_na.png",
        "cgsys/title/btn_arasuji_off.png",
    ] {
        match index.read_image_bytes(name) {
            Some(b) => {
                let safe = name.replace(['\\', '/'], "_");
                let out = format!("{outdir}/{safe}");
                std::fs::write(&out, &b).unwrap();
                println!("{name} -> {out} ({} bytes)", b.len());
            }
            None => println!("{name} -> 未命中"),
        }
    }
}
