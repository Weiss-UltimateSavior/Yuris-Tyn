//! 临时取证:用引擎自己的 scenario 桥宏 `ES.SCR.T1` 运行样本 `\T` 参数,
//! dump CG 注册表(真实屏幕坐标;成果 87 §3)。
//!
//! 用法:
//!   cargo run -p yuris-vm --example tmp_st_layout -- <游戏目录> <name> <ms> <x> <y> <z> [x2 y2 z2]
//! 例:
//!   cargo run -p yuris-vm --example tmp_st_layout -- "<game>" K_PEN_1A0100 800 -270 114 200

use std::sync::Arc;

use yuris_vm::boot::Bootstrap;
use yuris_vm::host::PacFileIndex;
use yuris_vm::VmSuspend;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 7 {
        eprintln!("用法: tmp_st_layout <游戏目录> <name> <ms> <x> <y> <z> [x2 y2 z2]");
        std::process::exit(2);
    }
    let dir = &args[1];
    let name = &args[2];
    let num = |i: usize| -> i64 { args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0) };
    let (ms, x, y, z) = (num(3), num(4), num(5), num(6));
    let ypf = std::path::Path::new(dir).join("pac").join("bn.ypf");
    let booted = Bootstrap {
        ypf_bytes: std::fs::read(&ypf).expect("bn.ypf"),
        name_key: 0xC9,
        key: [0x2b, 0x90, 0x4f, 0x93],
        entry_label: None,
    }
    .boot()
    .expect("boot");
    let mut vm = booted.vm;
    let index = Arc::new(PacFileIndex::scan_game_dir(std::path::Path::new(dir), 0xC9).expect("pac"));
    vm.set_file_probe(index);
    for _ in 0..80 {
        match vm.run(4000) {
            Ok(VmSuspend::None) => {}
            _ => break,
        }
    }
    println!("[st] boot 后:帧深={} pc={}", vm.frame_depth(), vm.pc());
    let before = vm.frame_depth();
    let events_before = vm.events().len();
    let ints: Vec<(u32, i64)> = if args.len() >= 10 {
        vec![
            (1, 1),
            (2, ms),
            (3, x),
            (4, y),
            (5, z),
            (6, num(7)),
            (7, num(8)),
            (8, num(9)),
        ]
    } else {
        vec![(1, 1), (2, ms), (3, x), (4, y), (5, z)]
    };
    if let Err(e) = vm.push_guest_call("ES.SCR.T1", &ints, &[(1, name.as_bytes())]) {
        eprintln!("push_guest_call 失败: {e}");
        std::process::exit(1);
    }
    let groups0 = vm.executed_groups();
    let mut steps = 0;
    for _ in 0..400 {
        if vm.frame_depth() < before {
            break;
        }
        steps += 1;
        match vm.run(2000) {
            Ok(VmSuspend::None) => {}
            Ok(other) => {
                eprintln!("[st] suspend: {other:?}(继续驱动)");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => {
                eprintln!("[st] run 中止: {e}");
                break;
            }
        }
    }
    println!(
        "[st] 调用后:帧深={} 步数={} 执行组 {} → {}",
        vm.frame_depth(),
        steps,
        groups0,
        vm.executed_groups()
    );
    // 事件统计(Cg/CgAct/CgEnd 是否被桥宏触发)
    let mut cg_events = 0usize;
    let mut cgact_events = 0usize;
    let mut text_events = 0usize;
    for ev in vm.events() {
        match ev {
            yuris_vm::VmEvent::Cg { .. } => cg_events += 1,
            yuris_vm::VmEvent::CgAct { .. } => cgact_events += 1,
            yuris_vm::VmEvent::Text { .. } => text_events += 1,
            _ => {}
        }
    }
    println!(
        "[st] 事件:总 {} Cg={} CgAct={} Text={}",
        vm.events().len(),
        cg_events,
        cgact_events,
        text_events
    );
    // 调用后事件汇总:按脚本计 Cg/CgAct;打印含位置/文件/ID 非空的样本
    println!("--- 调用后事件汇总 ---");
    let mut by_script: std::collections::BTreeMap<(u16, u8), usize> =
        std::collections::BTreeMap::new();
    let mut interesting: Vec<String> = Vec::new();
    for ev in &vm.events()[events_before..] {
        match ev {
            yuris_vm::VmEvent::Cg { pc, script_id, id, position, file, .. } => {
                *by_script.entry((*script_id, 1)).or_default() += 1;
                let has = position.is_some() || file.is_some()
                    || id.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
                if has && interesting.len() < 60 {
                    interesting.push(format!(
                        "Cg s{script_id} pc{pc} id={id:?} pos={position:?} file={:?}",
                        file.as_deref().map(String::from_utf8_lossy)
                    ));
                }
            }
            yuris_vm::VmEvent::CgAct { pc, id, evaluated } => {
                *by_script.entry((0, 2)).or_default() += 1;
                if interesting.len() < 60 {
                    interesting.push(format!("CgAct pc{pc} id={id:?} ev={evaluated:?}"));
                }
            }
            _ => {}
        }
    }
    for ((sid, kind), n) in &by_script {
        let k = if *kind == 1 { "Cg" } else { "CgAct" };
        println!("  s{sid} {k} × {n}");
    }
    for l in &interesting {
        println!("{l}");
    }
    println!("--- CG 注册表({} 条)---", vm.cg_registry_len());
    for (n, cx, cy, f) in vm.cg_registry_snapshot() {
        let file = f
            .as_deref()
            .map(String::from_utf8_lossy)
            .map(|s| s.to_string())
            .unwrap_or_default();
        println!("{} @({cx},{cy}) file={file}", String::from_utf8_lossy(&n));
    }
}
