//! 临时取证工具:立绘档位/锚点验证(`read_cg_bytes` 确定性档位 + 尺寸)。
//!
//! 用法: cargo run -p yuris-vm --example tmp_tachie_probe -- <游戏目录> <名字...>
//! 例:   cargo run -p yuris-vm --example tmp_tachie_probe -- \
//!         "/path/to/game" L_NYA_1A0100 M_LOP_1A0100 K_PEN_1A0100
//!
//! 输出 = 播放器同路径解析结果(PNG 宽高);用于核对
//! docs/layout-fix-plan.md P0(m_050 优先,不随进程随机)。

use yuris_vm::host::PacFileIndex;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: tmp_tachie_probe <游戏目录> <名字...>");
        std::process::exit(2);
    }
    let dir = &args[1];
    let index = PacFileIndex::scan_game_dir(std::path::Path::new(dir), 0xC9).unwrap();
    for name in &args[2..] {
        match index.read_cg_bytes(name) {
            Some(b) if b.len() >= 24 => {
                let w = u32::from_be_bytes(b[16..20].try_into().unwrap());
                let h = u32::from_be_bytes(b[20..24].try_into().unwrap());
                println!("{name}: {w}x{h} ({} B, x=960+x-{:.0}, y=顶边)", b.len(), w as f32 / 2.0);
            }
            Some(b) => println!("{name}: 命中但非 PNG({} B)", b.len()),
            None => println!("{name}: 未命中"),
        }
    }
}
