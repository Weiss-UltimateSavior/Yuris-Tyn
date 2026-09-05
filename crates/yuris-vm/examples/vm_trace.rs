//! P1 引擎真值对拍 —— Rust VM 事件流导出器。
//!
//! 从真实 bn.ypf 跑完整 Bootstrap 启动链(SYSTEM_START 入口),把
//! [`VmEvent`] 流按统一 schema(`yuris_vm::event_json`)导出 JSONL,
//! 与 `scripts/engine_trace.py` 采集的引擎真值逐事件对拍
//! (diff 工具:`scripts/diff_engine_vm.py`)。
//!
//! 用法:
//! ```text
//! cargo run -p yuris-vm --example vm_trace -- <bn.ypf> <out.jsonl> [max_groups]
//! ```
//!
//! 首行输出 meta(入口脚本/组),与引擎 trace 的对拍域 = group 事件
//! 重建出的 (script, pc, cmd) 序列。

use std::io::Write;

use yuris_vm::boot::Bootstrap;
use yuris_vm::{ResumeResponse, VmSuspend};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: vm_trace <bn.ypf> <out.jsonl> [max_groups=2000000]");
        std::process::exit(2);
    }
    let ypf_path = &args[1];
    let out_path = &args[2];
    let budget_groups: usize = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2_000_000);

    let bytes = std::fs::read(ypf_path).unwrap_or_else(|e| panic!("读 {ypf_path}: {e}"));
    let boot = Bootstrap {
        ypf_bytes: bytes,
        name_key: 0xC9,
        key: [0x2b, 0x90, 0x4f, 0x93],
        entry_label: None, // 缺省 SYSTEM_START(与引擎启动链一致)
    };
    let booted = boot.boot().expect("启动链编排");
    eprintln!(
        "[vm_trace] 入口 script 0x{:03x} pc {}(YSVR 初值 {} 条)",
        booted.script_id, booted.entry_pc, booted.applied_ysvr
    );

    let mut vm = booted.vm;
    // 虚拟 FS 探针(FILEINFO EXIST;P5.2):bn.ypf 所在目录树 = 游戏数据根。
    // 复现 oracle 环境:引擎 trace 时刻存在松散 R18 标记文件(现已不在),
    // es.R18Check 据此返回 1 —— 重放须同输入。
    if let Some(dir) = std::path::Path::new(ypf_path).parent() {
        match yuris_vm::host::PacFileIndex::scan_game_dir(dir, 0xC9) {
            Ok(mut idx) => {
                idx.add_virtual("cg/thumb_cg/A_HAN_2002_a.png");
                vm.set_file_probe(std::sync::Arc::new(idx));
                eprintln!("[vm_trace] file probe 就绪");
            }
            Err(e) => eprintln!("[vm_trace] file probe 不可用: {e}"),
        }
    }
    let out_file = std::fs::File::create(out_path)
        .unwrap_or_else(|e| panic!("建 {out_path}: {e}"));
    let mut out = std::io::BufWriter::with_capacity(1 << 20, out_file);

    writeln!(
        out,
        r#"{{"ev":"meta","side":"vm","entry_script":{},"entry_pc":{}}}"#,
        booted.script_id, booted.entry_pc
    )
    .unwrap();

    let mut dumped = 0usize;
    let mut stop = String::from("budget");
    loop {
        let s = match vm.run(1000) {
            Ok(s) => s,
            Err(e) => {
                stop = format!("err:{e}");
                break;
            }
        };
        let evs = vm.events();
        for e in &evs[dumped..] {
            writeln!(out, "{}", yuris_vm::event_json(e)).unwrap();
        }
        dumped = evs.len();
        match s {
            VmSuspend::None => {
                if vm.executed_groups() >= budget_groups {
                    break;
                }
            }
            VmSuspend::Wait { .. } => {
                // 引擎侧每帧真等;VM 侧立即 resume(帧循环在引擎 trace 里
                // 受真实帧率限制,对拍时按前缀对齐)
                if let Err(e) = vm.resume(ResumeResponse::Continue) {
                    stop = format!("resume-err:{e}");
                    break;
                }
                if vm.executed_groups() >= budget_groups {
                    break;
                }
            }
            VmSuspend::Complete => {
                stop = "complete".into();
                break;
            }
            VmSuspend::Error(m) => {
                stop = format!("error:{m}");
                break;
            }
        }
    }
    writeln!(
        out,
        r#"{{"ev":"done","reason":"{}","events":{},"groups":{}}}"#,
        stop.replace('\\', "\\\\").replace('"', "\\\""),
        dumped,
        vm.executed_groups()
    )
    .unwrap();
    out.flush().unwrap();
    eprintln!("[vm_trace] {} -> {} 事件,{} 组,停于 {}", out_path, dumped, vm.executed_groups(), stop);
}
