//! P5.2 侦查:按名字查 YSLB 标签。
//! 用法: cargo run -p yuris-tools --example yslb_find -- <bn.ypf> <label>
use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (ypf, needle) = (args[1].clone(), args[2].clone());
    let data = std::fs::read(&ypf)?;
    let arch = yuris_format::ypf::YpfArchive::from_bytes(data, 0xC9)
        .map_err(|e| anyhow::anyhow!("ypf: {e}"))?;
    let blob = arch
        .read("%ysbin\\ysl.ybn")
        .map_err(|e| anyhow::anyhow!("ysl: {e}"))?;
    let t = yuris_format::yslb::YslbTable::from_bytes(&blob)?;
    let mut by_target: BTreeMap<(u16, u32), Vec<String>> = Default::default();
    for l in t.labels() {
        by_target
            .entry((l.script_id, l.target_pc))
            .or_default()
            .push(String::from_utf8_lossy(&l.name).to_string());
    }
    for l in t.labels() {
        let name = String::from_utf8_lossy(&l.name);
        if name.eq_ignore_ascii_case(&needle) {
            println!("found: {} -> s{} pc{}", name, l.script_id, l.target_pc);
            if let Some(ns) = by_target.get(&(l.script_id, l.target_pc)) {
                println!("  all labels at target: {ns:?}");
            }
        }
    }
    Ok(())
}
