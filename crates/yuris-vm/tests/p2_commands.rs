//! P2 命令补全测试:运行期声明族(INT/FLT/STR)+ 后端子系统事件族。
//!
//! 真实样本断言:窗口字节取自引擎 trace 实际执行过的组
//! (engine_trace_boot.jsonl 定位 → probe_group_windows.py dump):
//! - INT:script190 g30(@6293 = 0)、script12 g11(@1749 = 0)
//! - FLT:script301 g0(@7558 = 1080*100.0/1200 + 0.5 = 90.5,分辨率自适应布局)
//! - STR:script12 g10($1748 = $55[1])
//! - FONTINFO:script45 g26(B0=18 LET 目标 + B0=11 参数)
//! - MATH:script190 g342(函数选择 B0=20 + 操作数 @53[1]/@53[2] + 目标 @6352)

use yuris_format::ystb::YstbFile;
use yuris_vm::{GroupVm, VmEvent, cmd};
mod common;
use common::{make_ystb, SAMPLE_KEY};
use yuris_value::{ElemType, VarRef, VarSpace, Value};

fn var(space: u8, id: u16) -> VarRef {
    VarRef { space: VarSpace::from_prefix(space), id }
}

/// script190 g30 真实窗口:`INT @6293 = 0`(w1 = pushint64 0)
#[test]
fn runtime_int_declare_init_real_sample() {
    let w0: &[u8] = &[0x48, 3, 0, 0x40, 0x95, 0x18]; // var @6293
    let w1: &[u8] = &[
        0x4c, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, // pushint64 0
    ];
    let groups: &[(u8, u8, u16)] = &[(0x32, 2, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x01, 0x00, 0x00], w0.len() as u32, 0, w0),
        ([0x00, 0x01, 0x00, 0x00], w1.len() as u32, 0, w1),
    ];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    vm.run(100).unwrap();
    assert_eq!(
        vm.store().get(&var(0x40, 6293)).unwrap(),
        &Value::Int(0),
        "引擎 trace 实证:script190 g30 INT @6293 = 0(desc[6293] type=1 实测)"
    );
    assert!(vm.events().iter().any(
        |e| matches!(e, VmEvent::Declaration { command: 0x32, .. })
    ));
}

/// script12 g11 真实窗口:`INT @1749 = 0`;非零变体验证初值写入
#[test]
fn runtime_int_init_value_nonzero() {
    let w0: &[u8] = &[0x48, 3, 0, 0x40, 0xd5, 0x06]; // var @1749
    let w1: &[u8] = &[0x42, 1, 0, 7]; // pushint8 7
    let groups: &[(u8, u8, u16)] = &[(0x32, 2, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x01, 0x00, 0x00], w0.len() as u32, 0, w0),
        ([0x00, 0x01, 0x00, 0x00], w1.len() as u32, 0, w1),
    ];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    vm.run(100).unwrap();
    assert_eq!(vm.store().get(&var(0x40, 1749)).unwrap(), &Value::Int(7));
}

/// script301 g0 真实窗口:`FLT @7558 = 1080*100.0/1200 + 0.5`(= 90.5)
#[test]
fn runtime_flt_expression_init_real_sample() {
    let w0: &[u8] = &[0x48, 3, 0, 0x40, 0x86, 0x1d]; // var @7558
    let w1: &[u8] = &[
        0x57, 2, 0, 0x38, 0x04, // pushint16 1080
        0x46, 8, 0, 0, 0, 0, 0, 0, 0, 0x59, 0x40, // pushfloat 100.0
        0x2a, 0, 0, // *
        0x57, 2, 0, 0xb0, 0x04, // pushint16 1200
        0x2f, 0, 0, // /
        0x46, 8, 0, 0, 0, 0, 0, 0, 0, 0xe0, 0x3f, // pushfloat 0.5
        0x2b, 0, 0, // +
    ];
    let groups: &[(u8, u8, u16)] = &[(0x19, 2, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x01, 0x00, 0x00], w0.len() as u32, 0, w0),
        ([0x00, 0x02, 0x00, 0x00], w1.len() as u32, 0, w1),
    ];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    vm.run(100).unwrap();
    assert_eq!(
        vm.store().get(&var(0x40, 7558)).unwrap(),
        &Value::Float(90.5),
        "script301 分辨率自适应布局表达式:1080*100.0/1200 + 0.5"
    );
}

/// script12 g10 真实窗口:`STR $1748 = $55[1]`(字符串拷贝)
#[test]
fn runtime_str_copy_real_sample() {
    let w0: &[u8] = &[0x48, 3, 0, 0x24, 0xd4, 0x06]; // var $1748
    let w1: &[u8] = &[
        0x56, 3, 0, 0x24, 0x37, 0x00, // ref $55
        0x42, 1, 0, 1, // push 1
        0x29, 1, 0, 0x00, // aload → $55[1]
    ];
    let groups: &[(u8, u8, u16)] = &[(0x5c, 2, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x03, 0x00, 0x00], w0.len() as u32, 0, w0),
        ([0x00, 0x03, 0x00, 0x00], w1.len() as u32, 0, w1),
    ];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    // 引擎 trace 时点:script12 处于某 GOSUB 帧,$55[1] = 该帧实参
    // (成果 56:帧局部族读恒走当前帧,全局 store 无回退)。
    vm.frame_locals_mut().declare_array(
        &var(0x24, 55),
        ElemType::Str,
        &[2],
    );
    vm.frame_locals_mut()
        .set_elem(&var(0x24, 55), &[1], Value::Str(b"abc".to_vec()))
        .unwrap();
    vm.run(100).unwrap();
    assert_eq!(
        vm.store().get(&var(0x24, 1748)).unwrap(),
        &Value::Str(b"abc".to_vec()),
        "引擎 trace 实证:script12 g10 STR $1748 = $55[1]"
    );
}

/// script45 g26/g28 真实窗口:FONTINFO(B0=18 LET 目标 / B0=11、B0=13 参数)。
/// 未声明目标读取 → 引擎容忍(实测),VM 事件记录 err,不挂起。
#[test]
fn subsystem_fontinfo_real_sample() {
    let w0: &[u8] = &[0x48, 3, 0, 0x40, 0x7c, 0x0a]; // var @2684
    let w1: &[u8] = &[0x42, 1, 0, 1]; // push 1
    let groups: &[(u8, u8, u16)] = &[(0x1b, 2, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x12, 0x01, 0x00, 0x00], w0.len() as u32, 0, w0), // B0=18
        ([0x0b, 0x01, 0x00, 0x00], w1.len() as u32, 0, w1), // B0=11
    ];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    vm.run(100).unwrap();
    let ev = vm.events().iter().find_map(|e| match e {
        VmEvent::Subsystem { command, evaluated, .. } if *command == 0x1b => {
            Some(evaluated.clone())
        }
        _ => None,
    });
    let evaluated = ev.expect("FONTINFO 应产生 Subsystem 事件");
    // NUM 查询写回 @2684 = oracle 字体数(环境依赖常量,与 lib.rs 同步;
    // 2026-09-05 采集 = 783)
    const FONTS: i64 = 783;
    assert_eq!(evaluated.len(), 2, "两个窗口都应记录");
    assert_eq!(evaluated[0], (18, format!("int:{FONTS}")));
    assert_eq!(evaluated[1], (11, "int:1".to_string()));
    let r = VarRef { space: VarSpace::At, id: 2684 };
    assert_eq!(vm.store().get(&r).unwrap(), &Value::Int(FONTS));
}

/// script190 g342 真实窗口:MATH(函数选择 B0=20 + 操作数 @53[1]/@53[2]
/// + 目标 @6352)。
#[test]
fn subsystem_math_real_sample() {
    let w0: &[u8] = &[0x42, 1, 0, 1]; // push 1(函数选择)
    let w1: &[u8] = &[
        0x56, 3, 0, 0x40, 0x35, 0x00, 0x42, 1, 0, 1, 0x29, 1, 0, 0x00, // @53[1]
    ];
    let w2: &[u8] = &[
        0x56, 3, 0, 0x40, 0x35, 0x00, 0x42, 1, 0, 2, 0x29, 1, 0, 0x00, // @53[2]
    ];
    let w3: &[u8] = &[0x48, 3, 0, 0x40, 0xd0, 0x18]; // var @6352
    let groups: &[(u8, u8, u16)] = &[(0x3c, 4, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x14, 0x01, 0x00, 0x00], w0.len() as u32, 0, w0), // B0=20
        ([0x01, 0x01, 0x00, 0x00], w1.len() as u32, 0, w1), // B0=1
        ([0x02, 0x01, 0x00, 0x00], w2.len() as u32, 0, w2), // B0=2
        ([0x00, 0x01, 0x00, 0x00], w3.len() as u32, 0, w3), // B0=0
    ];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    // 引擎 trace 时点:s190 g342 处于某子程序帧,@53[1]/@53[2] = 帧实参
    // (成果 56:帧局部族读恒走当前帧,全局 store 无回退)。
    vm.frame_locals_mut().declare_array(&var(0x40, 53), ElemType::Int, &[4]);
    vm.frame_locals_mut().set_elem(&var(0x40, 53), &[1], Value::Int(11)).unwrap();
    vm.frame_locals_mut().set_elem(&var(0x40, 53), &[2], Value::Int(22)).unwrap();
    vm.run(100).unwrap();
    let ev = vm.events().iter().find_map(|e| match e {
        VmEvent::Subsystem { command, evaluated, .. } if *command == 0x3c => {
            Some(evaluated.clone())
        }
        _ => None,
    });
    let evaluated = ev.expect("MATH 应产生 Subsystem 事件");
    assert_eq!(evaluated.len(), 4);
    assert_eq!(evaluated[0], (20, "int:1".to_string()));
    assert_eq!(evaluated[1], (1, "int:11".to_string()));
    assert_eq!(evaluated[2], (2, "int:22".to_string()));
    assert!(
        evaluated[3].0 == 0 && evaluated[3].1.starts_with("err:"),
        "未声明 @6352 读取 → err 记录(引擎容忍语义),实际 {:?}",
        evaluated[3]
    );
}

/// 子系统族全量:0x0e/0x14/0x15/0x1a/0x1c/0x31/0x45/0x5d/0x6b 各命令事件化
/// (反编译引用:004699cc/0043da80/0043eb28/00441338/004426a4/00443480/
/// 0044955c/0044f968/004583bc —— 引擎侧均为子系统状态操作,无 VM 状态)。
#[test]
fn subsystem_family_events() {
    let cmds = [0x0eu8, 0x14, 0x15, 0x1a, 0x1c, 0x31, 0x45, 0x5d, 0x6b];
    let w: &[u8] = &[0x42, 1, 0, 3]; // push 3
    for c in cmds {
        let groups: &[(u8, u8, u16)] = &[(c, 1, 0), (cmd::RETURN, 0, 0)];
        let windows = &[([0x00, 0x01, 0x00, 0x00], w.len() as u32, 0, w)];
        let bytes = make_ystb(SAMPLE_KEY, groups, windows);
        let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
        let mut vm = GroupVm::load(script).unwrap();
        vm.run(100).unwrap();
        assert!(
            vm.events().iter().any(|e| matches!(
                e,
                VmEvent::Subsystem { command, evaluated, .. }
                    if *command == c && evaluated.len() == 1
            )),
            "0x{c:02x} 应产生带参数的 Subsystem 事件"
        );
    }
}

/// 事件 JSON schema:sub 事件与 diff 工具域一致。
#[test]
fn subsystem_event_json_shape() {
    use yuris_vm::event_json;
    let e = VmEvent::Subsystem {
        pc: 5,
        command: 0x1b,
        evaluated: vec![(18, "int:1".into())],
    };
    assert_eq!(
        event_json(&e),
        r#"{"ev":"sub","pc":5,"cmd":27,"ev_":["18:int:1"]}"#
    );
}

/// 声明组消费不受运行期声明族影响(boot 消费路径回归)。
#[test]
fn declarations_still_consumed_at_load() {
    use yuris_vm::consume_declarations_into;
    use yuris_vm::host::ScriptCtx;
    let w0: &[u8] = &[0x48, 3, 0, 0x40, 0x11, 0x04]; // var @1041
    let groups: &[(u8, u8, u16)] = &[(0x32, 1, 0)];
    let windows = &[([0x00, 0x01, 0x00, 0x00], w0.len() as u32, 0, w0)];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    let ctx = ScriptCtx::parse(0, script).unwrap();
    let mut store = yuris_value::VariableStore::new();
    let n = consume_declarations_into(&mut store, &ctx).unwrap();
    assert_eq!(n, 1);
    assert_eq!(store.get(&var(0x40, 1041)).unwrap(), &Value::Int(0));
}
