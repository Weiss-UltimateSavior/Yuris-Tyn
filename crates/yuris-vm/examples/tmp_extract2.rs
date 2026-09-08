//! 临时取证工具:提取 extra 画面素材(播放器同路径 read_cg_bytes)。
//!
//! 用法: cargo run -p yuris-vm --example tmp_extract2 -- <游戏目录> <输出目录>

use yuris_vm::host::PacFileIndex;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, outdir) = (args[1].clone(), args[2].clone());
    std::fs::create_dir_all(&outdir).unwrap();
    let index = PacFileIndex::scan_game_dir(std::path::Path::new(&dir), 0xC9).unwrap();
    for name in [
        "cgsys/extra/back",
        "cgsys/extra/cgmode/back",
        "cgsys/extra/bgmmode/back",
        "cgsys/extra/vomode/back",
        "cgsys/extra/btn_back_bt3",
        "cgsys/extra/btn_tab_cgmode_bt3n",
        "cgsys/extra/btn_tab_cgmode_on",
        "cgsys/extra/btn_tab_bgmmode_bt3n",
        "cgsys/extra/btn_tab_rpmode_bt3n",
        "cgsys/extra/btn_tab_mvmode_bt3n",
    ] {
        match index.read_cg_bytes(name) {
            Some(b) => {
                let safe = name.replace(['\\', '/'], "_");
                let out = format!("{outdir}/{safe}.png");
                std::fs::write(&out, &b).unwrap();
                println!("{name} -> {out} ({} bytes)", b.len());
            }
            None => println!("{name} -> 未命中"),
        }
    }
}
