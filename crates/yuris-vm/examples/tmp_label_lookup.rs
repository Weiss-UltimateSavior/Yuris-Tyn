//! 临时取证工具:YSLB 标签查址(es.BT.XY.SET 宏体定位)。
//!
//! 用法: cargo run -p yuris-vm --example tmp_label_lookup -- <游戏目录> <标签名>

use yuris_format::ypf::YpfArchive;
use yuris_format::yslb::YslbTable;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    let label = &args[2];
    let bn = std::path::Path::new(dir).join("pac/bn.ypf");
    let arch = YpfArchive::from_bytes(std::fs::read(&bn).unwrap(), 0xC9).unwrap();
    let data = arch.read(r"%ysbin\ysl.ybn").expect("ysl.ybn");
    let yslb = YslbTable::from_bytes(&data).expect("yslb");
    for l in &yslb.labels {
        if String::from_utf8_lossy(&l.name).contains(label.as_str()) {
            println!(
                "{} → script {} pc {}",
                String::from_utf8_lossy(&l.name),
                l.script_id,
                l.target_pc
            );
        }
    }
}
