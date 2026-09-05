//! P5.2 侦查:列出目标 (script, pc) 附近的 YSLB 标签。
//! 用法: cargo run -p yuris-tools --example yslb_around -- <bn.ypf> <script> <pc>
use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (ypf, script, pc) = (
        args[1].clone(),
        args[2].parse::<u16>().unwrap(),
        args[3].parse::<u32>().unwrap(),
    );
    // 复用 ypf 提取:用 yuris_tools 内部函数不可行(私有),直接借 yuris_format。
    let data = std::fs::read(&ypf)?;
    // 简化:YSLB 条目名 = %ysbin\ysl.ybn;用 yuris_format::ypf 解包。
    let ypf_arch = yuris_format::ypf::YpfArchive::from_bytes(data, 0xC9)
        .map_err(|e| anyhow::anyhow!("ypf: {e}"))?;
    let blob = ypf_arch
        .read("%ysbin\\ysl.ybn")
        .or_else(|_| ypf_arch.read("ysbin/ysl.ybn"))
        .map_err(|e| anyhow::anyhow!("ysl: {e}"))?;
    let t = yuris_format::yslb::YslbTable::from_bytes(&blob)?;
    let mut by_target: BTreeMap<(u16, u32), Vec<String>> = Default::default();
    for l in t.labels() {
        by_target
            .entry((l.script_id, l.target_pc))
            .or_default()
            .push(String::from_utf8_lossy(&l.name).to_string());
    }
    for pc in pc.saturating_sub(4)..=(pc + 4) {
        if let Some(ns) = by_target.get(&(script, pc)) {
            println!("s{script} pc{pc}: {ns:?}");
        }
    }
    Ok(())
}
