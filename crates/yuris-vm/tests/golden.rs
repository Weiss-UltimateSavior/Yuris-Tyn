//! Golden Test 框架(P3.2,自基线版)。
//!
//! ## 原理(`docs/02-workspace-design.md` §5)
//!
//! VM 事件流序列化为 JSONL 快照;与 `tests/golden/*.jsonl` 逐行比对。
//! 当前基线 = 本实现自身产生(自基线)——引擎真值待接(需 Windows/原版
//! 观测),替换期望文件后即可做**真** Golden 对比。
//!
//! 再生成:`YURIS_REGEN_GOLDEN=1 cargo test -p yuris-vm --test golden`

use std::collections::HashMap;

use yuris_format::ystb::YstbFile;
use yuris_vm::{GroupVm, cmd, event_json};
mod common;
use common::{make_ystb, SAMPLE_KEY};

/// 事件流 JSONL 序列化统一由 [`yuris_vm::event_json`] 提供(P1 引擎真值
/// 对拍与 vm_trace 导出共用同一 schema)。

fn snapshot_name(name: &str) -> String {
    format!("tests/golden/{name}.jsonl")
}

fn check_snapshot(name: &str, lines: &[String]) {
    let path = snapshot_name(name);
    let rendered = lines.join("\n") + "\n";
    if std::env::var("YURIS_REGEN_GOLDEN").is_ok() {
        std::fs::write(&path, &rendered).unwrap();
        eprintln!("golden regenerated: {path}");
        return;
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("golden 快照缺失 {path}({e});用 YURIS_REGEN_GOLDEN=1 生成")
    });
    assert_eq!(
        existing, rendered,
        "Golden 快照不一致: {path}\n(事件流变化;若为有意变更,用 YURIS_REGEN_GOLDEN=1 重生成)"
    );
}

/// 场景 1:IF 假 → ELSE → IFEND → RETURN 全链事件流。
#[test]
fn golden_if_false_chain() {
    let cond: &[u8] = &[0x42, 1, 0, 0]; // 假
    let groups: &[(u8, u8, u16)] = &[
        (cmd::IF, 3, 0),
        (cmd::GO, 1, 0),
        (cmd::ELSE, 0, 0),
        (cmd::IFEND, 0, 0),
        (cmd::RETURN, 0, 0),
    ];
    let go_label: &[u8] = &[0x4d, 3, 0, b'"', b'L', b'"'];
    let windows = &[
        ([0x00, 0x00, 0x01, 0x00], cond.len() as u32, 0, cond),
        (common::DUMMY, 2, 0, b""),
        (common::DUMMY, 3, 0, b""),
        (common::DUMMY, go_label.len() as u32, 0, go_label),
    ];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    vm.set_strict(false);
    vm.run(100).unwrap();

    let lines: Vec<String> = vm.events().iter().map(|e| event_json(e)).collect();
    check_snapshot("if_false_chain", &lines);
}

/// 场景 2:GOSUB/RETURN 往返 + LET 数组赋值复合码。
#[test]
fn golden_gosub_and_let() {
    // g0 GOSUB(真) → g2=SUB;SUB 内 LET @5000[1]=5;RETURN → g1 IFEND 路径结束
    let cond: &[u8] = &[0x42, 1, 0, 1];
    let sub_label: &[u8] = &[0x4d, 5, 0, b'"', b'S', b'U', b'B', b'"'];
    let let_lhs: &[u8] = &[0x56, 3, 0, 0x40, 0x88, 0x13, 0x42, 1, 0, 1]; // @5000[1]
    let let_rhs: &[u8] = &[0x42, 1, 0, 5];
    let groups: &[(u8, u8, u16)] = &[
        (cmd::GOSUB, 2, 0),  // g0
        (cmd::RETURN, 0, 0), // g1(帧空 → 结束)
        (cmd::LET, 2, 0),    // g2 = SUB
    ];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], cond.len() as u32, 0, cond),
        (common::DUMMY, sub_label.len() as u32, 0, sub_label),
        ([0x00, 0x00, 0x00, 0x00], let_lhs.len() as u32, 0, let_lhs),
        (common::DUMMY, let_rhs.len() as u32, 0, let_rhs),
    ];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    vm.set_labels(HashMap::from([(
        b"SUB".to_vec(),
        (2u32, 0u16),
    )]));
    vm.set_script_id(0);
    // YSVR 语义:声明全局数组
    vm.store_mut().declare_array(
        &yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 5000 },
        yuris_value::ElemType::Int,
        &[4],
    );
    vm.run(100).unwrap();

    let lines: Vec<String> = vm.events().iter().map(|e| event_json(e)).collect();
    check_snapshot("gosub_and_let", &lines);
}
