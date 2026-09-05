//! P5.2 TASKINFO/TASK 测试(成果 61)。
//!
//! 引擎语义(反编译 `p5_sysvar/00451838_CMD_TASKINFO_0x61.c` +
//! `0044fe40_CMD_TASK_0x5f.c`):
//! - TASK:槽 0=ID(名)、槽 34='#'(入口标签)、槽 3-7 参数 → 按名创建/
//!   重配置任务(FUN_0040ca7d 按名查找);
//! - TASKINFO:槽 0=ID + 槽 2=EXIST + 槽 1=LET → 注册表命中写 1/未命中写 0。
//!
//! 真实样本:引擎 400k trace seq 139496/139505 两处 TASKINFO(s190 g472)
//! 均 EXIST=0(任务未创建)→ 随后 TASK(0x5f)注册 "es.IDSubTask";
//! VM 对拍零分歧至 137544 组(全 boot 链 + scenario 入口)。

mod common;

use common::{make_ystb, SAMPLE_KEY};
use yuris_format::ystb::YstbFile;
use yuris_value::{Value, VarRef, VarSpace};
use yuris_vm::{GroupVm, VmSuspend, cmd};

fn var(space: VarSpace, id: u16) -> VarRef {
    VarRef { space, id }
}

/// 合成:TASK(名) → TASKINFO(ID+EXIST+LET) → 断言 1;未注册名 → 0。
#[test]
fn taskinfo_exist_roundtrip() {
    let key = SAMPLE_KEY;
    // w_task_name: 槽0 = "es.Test" ;w_label: 槽34 = "ES.LOOP"
    let w_name: &[u8] = &[
        0x4d, 9, 0, b'"', b'e', b's', b'.', b'T', b'e', b's', b't', b'"',
    ];
    let w_label: &[u8] = &[
        0x4d, 8, 0, b'"', b'E', b'S', b'.', b'L', b'O', b'O', b'P', b'"',
    ];
    let w_exist: &[u8] = &[0x42, 1, 0, 1]; // push 1
    // TASKINFO 的 ID 槽 = 字符串(合成用字面量推入;引擎经 $55[1] 帧局部)
    let w_query: &[u8] = &[
        0x4d, 9, 0, b'"', b'e', b's', b'.', b'T', b'e', b's', b't', b'"',
    ];
    let w_let: &[u8] = &[0x48, 3, 0, 0x40, 0x28, 0x23]; // var @9000

    let groups: &[(u8, u8, u16)] = &[
        (cmd::TASK, 2, 0),       // TASK(名, 标签)
        (cmd::TASKINFO, 3, 0),   // TASKINFO(ID, EXIST, LET)
        (cmd::RETURN, 0, 0),
    ];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_name.len() as u32, 0, w_name),  // 槽 0
        ([0x22, 0x00, 0x00, 0x00], w_label.len() as u32, 0, w_label), // 槽 34
        ([0x00, 0x00, 0x00, 0x00], w_query.len() as u32, 0, w_query), // 槽 0
        ([0x02, 0x00, 0x00, 0x00], w_exist.len() as u32, 0, w_exist), // 槽 2
        ([0x01, 0x00, 0x00, 0x00], w_let.len() as u32, 0, w_let),    // 槽 1
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert!(matches!(s, VmSuspend::Complete), "{s:?}");
    let r = var(VarSpace::At, 9000);
    assert_eq!(vm.store().get(&r).unwrap(), &Value::Int(1), "注册后 EXIST=1");
}

/// 未注册任务名 → EXIST=0。
#[test]
fn taskinfo_exist_missing() {
    let key = SAMPLE_KEY;
    let w_query: &[u8] = &[
        0x4d, 9, 0, b'"', b'e', b's', b'N', b'o', b'p', b'e', b'"',
    ];
    let w_exist: &[u8] = &[0x42, 1, 0, 1];
    let w_let: &[u8] = &[0x48, 3, 0, 0x40, 0x29, 0x23]; // var @9001
    let groups: &[(u8, u8, u16)] = &[(cmd::TASKINFO, 3, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_query.len() as u32, 0, w_query),
        ([0x02, 0x00, 0x00, 0x00], w_exist.len() as u32, 0, w_exist),
        ([0x01, 0x00, 0x00, 0x00], w_let.len() as u32, 0, w_let),
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert!(
        vm.executed_groups() > 0,
        "应执行组:suspend={s:?} events={}",
        vm.events().len()
    );
    let r = var(VarSpace::At, 9001);
    assert_eq!(vm.store().get(&r).unwrap(), &Value::Int(0), "未注册 EXIST=0");
}

/// 非 EXIST 查询槽(NEXTVOICE=1 无 ID)→ 显式 Unsupported(不猜)。
#[test]
fn taskinfo_other_query_halts() {
    let key = SAMPLE_KEY;
    let w_voice: &[u8] = &[0x42, 1, 0, 1]; // 槽 13 NEXTVOICE = 1
    let w_let: &[u8] = &[0x48, 3, 0, 0x40, 0x2a, 0x23]; // var @9002
    let groups: &[(u8, u8, u16)] = &[(cmd::TASKINFO, 2, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x0d, 0x00, 0x00, 0x00], w_voice.len() as u32, 0, w_voice), // 槽 13
        ([0x01, 0x00, 0x00, 0x00], w_let.len() as u32, 0, w_let),     // 槽 1
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let err = vm.run(100).unwrap_err();
    assert!(err.to_string().contains("非 EXIST 查询槽"), "{err}");
}
