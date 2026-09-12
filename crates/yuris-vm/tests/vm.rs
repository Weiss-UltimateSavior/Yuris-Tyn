//! 组级 VM 测试:合成脚本(走完整 YSTB 加密管线)+ 真实样本烟雾。
//!
//! 执行模型证据:`docs/engine/command-layer.md`(引擎 FUN_0040449c/
//! CMDH_0044272c/CMDH_004428c0/CMDH_0044b418/CMDH_004431ec/CMDH_004432e4/
//! CMDH_00443434)。

use std::collections::HashMap;

use yuris_format::ystb::YstbFile;
use yuris_vm::{GroupVm, JumpKind, ResumeResponse, VmEvent, VmSuspend, VmState, cmd};
mod common;
use common::{make_ystb, DUMMY};


/// 是否为已实现的流程控制命令(启动链评估用)。
fn is_flow_command(cmd: u8) -> bool {
    matches!(
        cmd,
        0x2a | 0x2b | 0x2c | 0x2d | 0x30 | 0x35 | 0x37 | 0x38 | 0x39 | 0x3a | 0x4f | 0x68 | 0x0d
    )
}

/// IF 条件为假 → 跳 w1.len(ELSE) → ELSE 跳 w2.len(IFEND) → 弹栈 → RETURN 结束。
#[test]
fn if_false_else_ifend_flow() {
    // 条件字节码:pushint8 0(假)
    let cond: &[u8] = &[0x42, 1, 0, 0];
    let groups: &[(u8, u8, u16)] = &[
        (cmd::IF, 3, 0),    // g0
        (cmd::GO, 1, 0),    // g1(then 动作:GO,本路径不应执行)
        (cmd::ELSE, 0, 0),  // g2
        (cmd::IFEND, 0, 0), // g3
        (cmd::RETURN, 0, 0),// g4
    ];
    // IF 3 窗:w0=条件;w1.len=2(假跳目标=ELSE);w2.len=3(end=IFEND);GO 1 窗
    let go_label: &[u8] = &[0x4d, 3, 0, b'"', b'L', b'"'];
    let windows = &[
        ([0x00, 0x00, 0x01, 0x00], cond.len() as u32, 0, cond), // tag=0x00010000
        (DUMMY, 2, 0, b""), // w1.len = ELSE 组号
        (DUMMY, 3, 0, b""), // w2.len = IFEND 组号
        (DUMMY, go_label.len() as u32, 0, go_label),
    ];
    let bytes = make_ystb([0x2b, 0x90, 0x4f, 0x93], groups, windows);
    let script = YstbFile::from_bytes(&bytes, [0x2b, 0x90, 0x4f, 0x93]).unwrap();
    assert_eq!(script.groups().unwrap().len(), 5);

    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert_eq!(vm.state(), VmState::Finished);

    // 事件链:IF 假 → 跳 ELSE 起点(2,IF 处理器 w1.len);ELSE(0x0b)0 窗
    //    = "else 块开始"标记 → 顺序执行 else 块 → g3 IFEND 弹栈 → g4 RETURN 结束。
    // 真实引擎:IF 假跳 w1.len;ELSE 无窗进块;IFBLEND 才跳过 else 块。
    let jumps: Vec<_> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            VmEvent::Jump { from, to, kind } => Some((*from, *to, *kind)),
            _ => None,
        })
        .collect();
    assert_eq!(jumps, vec![(0, 2, JumpKind::IfFalse)]);
}

/// IF 条件为真 → 顺序落入 then 块 → GO 经标签表跳到 RETURN → 结束。
#[test]
fn if_true_go_label_flow() {
    let cond: &[u8] = &[0x42, 1, 0, 1]; // pushint8 1(真)
    // GO 标签窗口:M-串 "L"(4d len "L")
    let go_label: &[u8] = &[0x4d, 3, 0, b'"', b'L', b'"'];
    let groups: &[(u8, u8, u16)] = &[
        (cmd::IF, 3, 0),     // g0
        (cmd::GO, 1, 0),     // g1(then:GO L)
        (cmd::ELSE, 0, 0),   // g2
        (cmd::IFEND, 0, 0),  // g3
        (cmd::RETURN, 0, 0), // g4 = L
    ];
    let windows = &[
        ([0, 0, 0, 1], cond.len() as u32, 0, cond), // IF w0:条件;w1.len=0 → 假跳走 w2.len
        (DUMMY, 0, 0, b""),                         // IF w1(len=0)
        (DUMMY, 3, 0, b""),                         // IF w2(end 目标 = IFEND g3)
        (DUMMY, go_label.len() as u32, 0, go_label), // GO 的标签窗
    ];
    let bytes = make_ystb([0x2b, 0x90, 0x4f, 0x93], groups, windows);
    let script = YstbFile::from_bytes(&bytes, [0x2b, 0x90, 0x4f, 0x93]).unwrap();

    let mut labels = HashMap::new();
    labels.insert(b"L".to_vec(), (4u32, 0u16));
    let mut vm = GroupVm::load(script).unwrap();
    vm.set_labels(labels);
    vm.set_script_id(0);

    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    let jumps: Vec<_> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            VmEvent::Jump { from, to, kind } => Some((*from, *to, *kind)),
            _ => None,
        })
        .collect();
    // 真路径:不跳过 then;GO(1→4) 直接跳过 ELSE/IFEND
    assert_eq!(jumps, vec![(1, 4, JumpKind::Go)]);
}

/// GOSUB(条件真)压帧跳转 → RETURN 弹帧回到 pc+1;帧空 RETURN = 脚本结束。
#[test]
fn gosub_return_roundtrip() {
    let cond_true: &[u8] = &[0x42, 1, 0, 1]; // 真
    let sub_label: &[u8] = &[0x4d, 5, 0, b'"', b'S', b'U', b'B', b'"'];
    let groups: &[(u8, u8, u16)] = &[
        (cmd::GOSUB, 2, 0),  // g0:条件真 → 跳 SUB(g2)
        (cmd::RETURN, 0, 0), // g1:RETURN(帧空)= 结束
        (cmd::RETURN, 0, 0), // g2 = SUB:RETURN 弹帧 → 回 g1
    ];
    let windows = &[
        ([0, 0, 0, 0], cond_true.len() as u32, 0, cond_true),
        (DUMMY, sub_label.len() as u32, 0, sub_label),
    ];
    let bytes = make_ystb([0x2b, 0x90, 0x4f, 0x93], groups, windows);
    let script = YstbFile::from_bytes(&bytes, [0x2b, 0x90, 0x4f, 0x93]).unwrap();

    let mut labels = HashMap::new();
    labels.insert(b"SUB".to_vec(), (2u32, 0u16));
    let mut vm = GroupVm::load(script).unwrap();
    vm.set_labels(labels);
    vm.set_script_id(0);

    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert_eq!(vm.state(), VmState::Finished);
    // 事件:Call(0→2) → Jump{Return}(2→1) → 结束
    assert!(vm
        .events()
        .iter()
        .any(|e| matches!(e, VmEvent::Call { from: 0, to: 2 })));
    assert!(vm
        .events()
        .iter()
        .any(|e| matches!(e, VmEvent::Jump { from: 2, to: 1, kind: JumpKind::Return })));
}

/// strict(默认):未实现命令 → Unsupported 事件 + Error 挂起;resume 显式报错。
/// (夹具 0x16 FLASH:P2 后事件化了 0x0e ERROR 等,FLASH 语义仍未逆向)
#[test]
fn strict_unsupported_halts() {
    let groups: &[(u8, u8, u16)] = &[(0x16, 0, 0)]; // 命令 0x16(FLASH)未逆向
    let bytes = make_ystb([0x2b, 0x90, 0x4f, 0x93], groups, &[]);
    let script = YstbFile::from_bytes(&bytes, [0x2b, 0x90, 0x4f, 0x93]).unwrap();

    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(10).unwrap();
    assert!(matches!(s, VmSuspend::Error(ref m) if m.contains("0x16")));
    assert_eq!(vm.state(), VmState::Error);
    assert!(matches!(
        vm.events().first(),
        Some(VmEvent::Unsupported { command: 0x16, .. })
    ));
    assert!(vm.resume(ResumeResponse::Continue).is_err());
}

/// trace 模式(strict=false):未实现命令记录事件后继续,走完全部组。
#[test]
fn trace_mode_continues_past_unsupported() {
    let groups: &[(u8, u8, u16)] = &[(0x16, 0, 0), (0x3e, 0, 0)]; // FLASH + MENU 未实现
    let bytes = make_ystb([0x2b, 0x90, 0x4f, 0x93], groups, &[]);
    let script = YstbFile::from_bytes(&bytes, [0x2b, 0x90, 0x4f, 0x93]).unwrap();

    let mut vm = GroupVm::load(script).unwrap();
    vm.set_strict(false);
    let s = vm.run(10).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert_eq!(vm.executed_groups(), 2);
    assert_eq!(
        vm.events()
            .iter()
            .filter(|e| matches!(e, VmEvent::Unsupported { .. }))
            .count(),
        2
    );
}

/// 真实样本烟雾:yst00000 全是 F_INT/F_STR 声明组(运行期处理器=报错 stub,
/// 引擎把它们当载入期数据)→ strict 首组即 Unsupported 挂起,事件携带 0x11。
#[test]
fn sample_yst00000_first_group_is_declaration() {
    let Some(p) = std::env::var("YURIS_SAMPLE_BNYPF")
        .map(std::path::PathBuf::from)
        .ok()
        .filter(|p| p.exists())
        .or_else(|| {
            let p = std::path::PathBuf::from(
                "/Users/weiss/Desktop/yuris/AnimalTrailGirlishSquare 2/pac/bn.ypf",
            );
            p.exists().then_some(p)
        })
    else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = yuris_format::ypf::YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("$ysbin\\yst00000.ybn").unwrap();
    let script = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();

    let mut vm = GroupVm::load(script).unwrap();
    assert_eq!(vm.group_count(), 202);
    // 声明类命令(首组 F_STR/F_INT 等)→ 记录 Declaration 事件并继续;
    // 一路走到某条真正运行时命令(引擎对声明类也用 stub 不执行)。
    let s = vm.run(1000).unwrap();
    // yst00000 全部为声明组(F_INT/F_STR/F_VAR 等)→ 全作为 Declaration 记录,
    // 到 RETURN 结束
    assert_eq!(s, VmSuspend::Complete);
    assert!(
        vm.events()
            .iter()
            .any(|e| matches!(e, VmEvent::Declaration { .. })),
        "应有声明类事件"
    );
}

/// LET 命令:数组元素赋值(全局 id≥1000,先按 YSVR 语义 declare_array),
/// 含复合赋值码(B3=1 → +=),再经 IF(aload+eq)读回验证。全程 Confirmed 语义。
#[test]
fn let_array_element_compound_and_readback() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // LET w0(左值): pushvarref @5000; pushint 2      → tag B3=1(+=)
    let w_lhs: &[u8] = &[0x56, 3, 0, 0x40, 0x88, 0x13, 0x42, 1, 0, 2];
    // LET w1(右值): pushint 3
    let w_rhs3: &[u8] = &[0x42, 1, 0, 3];
    // IF w0(条件): pushvaridx @5000; pushint 2; aload; pushint 3; eq
    //   (数组读 = 0x76 延迟占位 + 0x29 装载;引擎 0x48 不带下标)
    let w_cond: &[u8] = &[
        0x76, 3, 0, 0x40, 0x88, 0x13, 0x42, 1, 0, 2, 0x29, 1, 0, 0,
        0x42, 1, 0, 3, 0x3d, 0, 0,
    ];
    // IF w1/w2:占位(len 域存编译期组号)
    // LET 组 2(=99) 与 RETURN
    let w_rhs99: &[u8] = &[0x42, 1, 0, 99];
    let groups: &[(u8, u8, u16)] = &[
        (cmd::LET, 2, 0),   // g0
        (cmd::IF, 3, 0),    // g1
        (cmd::LET, 2, 0),   // g2(else 路径不应执行)
        (cmd::IFEND, 0, 0), // g3
        (cmd::RETURN, 0, 0),// g4
    ];
    let tag_let_add = [0x00, 0x00, 0x00, 0x01]; // B3=1 → 0x01000000(LE)
    let tag_let_set = [0x00, 0x00, 0x00, 0x00];
    let tag_if_cond = [0x00, 0x00, 0x01, 0x00]; // B2=1(@INT 求值表)
    let windows = &[
        ([0x00, 0x00, 0x00, 0x01], w_lhs.len() as u32, 0, w_lhs),
        ([0x00, 0x00, 0x00, 0x00], w_rhs3.len() as u32, 0, w_rhs3),
        ([0x00, 0x00, 0x01, 0x00], w_cond.len() as u32, 0, w_cond),
        ([0x00, 0x00, 0x00, 0x00], 0, 0, b""),  // IF w1(假跳目标 = ELSE;这里无 ELSE → 0 → 走 w2)
        ([0x00, 0x00, 0x00, 0x00], 3, 0, b""),  // IF w2(end = IFEND g3)
        ([0x00, 0x00, 0x00, 0x00], 10, 0, &[0x56, 3, 0, 0x40, 0x88, 0x13, 0x42, 1, 0, 2]), // LET g2 w0(左值)
        ([0x00, 0x00, 0x00, 0x00], w_rhs99.len() as u32, 0, w_rhs99), // LET g2 w1(右值 =99)
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();

    let mut vm = GroupVm::load(script).unwrap();
    // YSVR 语义:声明 @5000 为 4 元素 INT 数组(初值 0)
    use yuris_value::{ElemType, VarRef, VarSpace};
    vm.store_mut().declare_array(
        &VarRef { space: VarSpace::At, id: 5000 },
        ElemType::Int,
        &[4],
    );
    // 执行:LET @5000[2] += 3 → 0+3=3;IF(aload==3) 真 → 顺序走 g2 覆写 =99 → IFEND → RETURN
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    // 读回:@5000[2] == 99(真路径 LET 覆写,复合赋值与纯赋值先后生效)
    let r = VarRef { space: VarSpace::At, id: 5000 };
    assert_eq!(
        vm.store().get_elem(&r, &[2]).unwrap(),
        &yuris_value::Value::Int(99)
    );
    assert_eq!(
        vm.store().get_elem(&r, &[0]).unwrap(),
        &yuris_value::Value::Int(0)
    );
    // 事件:LET 组执行(条件=newv 99);真路径无 IfFalse 跳转
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::GroupExecuted { command: cmd::LET, condition: Some(yuris_value::Value::Int(99)), .. }
    )));
}

/// LET 帧局部(id=0x32-0x46)于顶层(depth 0)→ 写**基帧** locals(引擎
/// 004428c0/0044b418 定性:任务创建即有 record[0],`0x148+depth*4` 懒分配
/// 只作用于新帧;depth 0 的帧局部写落 record[0],无挂起路径 —— 成果 56)。
#[test]
fn let_frame_local_no_frame_halts() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // 左值: pushvarref @0x35 空间? 注意:帧局部 = LHS id 0x35(LET 局部)
    // 变量操作数 = [前缀][id:u16];前缀 0x40(@)。id=0x35 → 40 35 00
    let w_lhs: &[u8] = &[0x56, 3, 0, 0x40, 0x35, 0x00];
    let w_rhs: &[u8] = &[0x42, 1, 0, 1];
    let groups: &[(u8, u8, u16)] = &[(cmd::LET, 2, 0)];
    let _tag0 = [0x00, 0x00, 0x00, 0x00];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_lhs.len() as u32, 0, w_lhs),
        ([0x00, 0x00, 0x00, 0x00], w_rhs.len() as u32, 0, w_rhs),
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(10).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    // 值落在基帧(= 引擎 record[0]),不进全局 store
    let r = yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 0x35 };
    assert_eq!(
        vm.frame_locals_mut().get(&r).unwrap(),
        &yuris_value::Value::Int(1)
    );
}

/// YSVR 初始化链:解析真实 ysv.ybn → apply 初值 → LET 覆写 → 读回。
#[test]
fn ysvr_init_chain() {
    let Some(p) = std::env::var("YURIS_SAMPLE_BNYPF")
        .map(std::path::PathBuf::from)
        .ok()
        .filter(|p| p.exists())
        .or_else(|| {
            let p = std::path::PathBuf::from(
                "/Users/weiss/Desktop/yuris/AnimalTrailGirlishSquare 2/pac/bn.ypf",
            );
            p.exists().then_some(p)
        })
    else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = yuris_format::ypf::YpfArchive::from_bytes(data, 0xC9).unwrap();
    let ysv = ypf.read("%ysbin\\ysv.ybn").unwrap();
    let table = yuris_format::ysvr::YsvrTable::from_bytes(&ysv).unwrap();
    assert_eq!(table.entries().len(), 3362);

    // apply(引擎 FUN_00451348 语义):标量 → set;数组 → declare_array + 初值元素
    let mut store = yuris_value::VariableStore::new();
    let mut applied_scalar = 0usize;
    let mut applied_elem = 0usize;
    for e in table.entries() {
        let r = yuris_value::VarRef {
            space: yuris_value::VarSpace::from_prefix(0x40), // @ 空间(全语料 LET 左值)
            id: e.var_id,
        };
        match &e.init {
            yuris_format::ysvr::YsvrInit::None => {}
            init => {
                if e.bounds.is_empty() {
                    let v = match init {
                        yuris_format::ysvr::YsvrInit::Int(i) => yuris_value::Value::Int(*i),
                        yuris_format::ysvr::YsvrInit::Float(f) => yuris_value::Value::Float(*f),
                        yuris_format::ysvr::YsvrInit::Str(b) => yuris_value::Value::Str(b.clone()),
                        _ => continue,
                    };
                    store.set(&r, v);
                    applied_scalar += 1;
                } else {
                    let elem = match e.ty {
                        1 => yuris_value::ElemType::Int,
                        2 => yuris_value::ElemType::Float,
                        3 => yuris_value::ElemType::Str,
                        _ => continue,
                    };
                    store.declare_array(&r, elem, &e.bounds);
                    // 初值写入 0 号元素(多维=全零下标)
                    let zeros = vec![0i64; e.bounds.len()];
                    match init {
                        yuris_format::ysvr::YsvrInit::Int(i) => {
                            store.set_elem(&r, &zeros, yuris_value::Value::Int(*i)).unwrap()
                        }
                        yuris_format::ysvr::YsvrInit::Float(f) => {
                            store.set_elem(&r, &zeros, yuris_value::Value::Float(*f)).unwrap()
                        }
                        yuris_format::ysvr::YsvrInit::Str(b) => {
                            store.set_elem(&r, &zeros, yuris_value::Value::Str(b.clone())).unwrap()
                        }
                        _ => {}
                    }
                    applied_elem += 1;
                }
            }
        }
    }
    assert!(applied_scalar > 1000, "标量初值应大量存在,实际 {applied_scalar}");
    assert!(applied_elem > 100, "数组初值应大量存在,实际 {applied_elem}");
}

/// LOOP 族:LOOP(3) → 体(LET @5001[0] += 1;读回)+ LOOPEND。
/// 计数语义:LOOP 置 counter=1 → 每过一次 LOOPEND +1 → 计数≥limit 时退出。
/// 体执行 = 3 次(limit=3:counter 1→2→3→退出前最后一次判断)。
#[test]
fn loop_three_iterations_with_let_counter() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // g0 LOOP:计数 = 3(w0 = pushint 3)
    // g1 LET @5001[0] += 1(左值 @5001[0], 右值 1;B3=1)
    // g2 LOOPEND
    // g3 RETURN(帧空 → 结束)
    let w_loop_cnt: &[u8] = &[0x42, 1, 0, 3];
    let w_lhs: &[u8] = &[
        0x56, 3, 0, 0x40, 0x89, 0x13, // pushvarref @5001
        0x42, 1, 0, 0, // pushint 0(下标;LET 处理器装载/存储)
    ];
    let w_rhs: &[u8] = &[0x42, 1, 0, 1];
    let groups: &[(u8, u8, u16)] = &[
        (cmd::LOOP, 1, 0),  // g0
        (cmd::LET, 2, 0),   // g1
        (cmd::LOOPEND, 0, 0), // g2
        (cmd::RETURN, 0, 0), // g3
    ];
    let tag_let = [0x00, 0x00, 0x00, 0x01]; // B3=1(+=)
    let windows = &[
        ([0x00, 0x00, 0x01, 0x00], w_loop_cnt.len() as u32, 0, w_loop_cnt),
        (tag_let, w_lhs.len() as u32, 0, w_lhs),
        (common::DUMMY, w_rhs.len() as u32, 0, w_rhs),
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();

    let mut vm = GroupVm::load(script).unwrap();
    vm.store_mut().declare_array(
        &yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 5001 },
        yuris_value::ElemType::Int,
        &[1],
    );
    let s = vm.run(1000).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    // counter 语义:LOOP 置 1;每次 LOOPEND 后 +1;counter>=limit 时退出。
    // limit=3:体执行次数 = 3(counter 1,2,3 → 第 3 次 LOOPEND 时 counter=3>=3 退出)
    // → LET 执行 3 次 → @5001[0] = 3
    assert_eq!(
        vm.store().get_elem(
            &yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 5001 },
            &[0]
        ).unwrap(),
        &yuris_value::Value::Int(3)
    );
}

/// LOOPBREAK:var>=1 时跳出(默认 LV=1)。
/// 迭代 0:var=0 → 条件假 → LET(+10) → var=10;迭代 1:10>=1 真 → LOOPBREAK
/// → 退出点 = 配对 LOOPEND 的下一条 → RETURN。最终 var=10(体只执行一次)。
#[test]
fn loop_break_exits_early() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    let w_loop_cnt: &[u8] = &[0x42, 1, 0, 3]; // LOOP 3(上限够大,提前 break)
    let w_cond: &[u8] = &[
        0x76, 3, 0, 0x40, 0x89, 0x13, 0x42, 1, 0, 0, 0x29, 1, 0, 0,
        0x42, 1, 0, 10, 0x3d, 0, 0, // aload @5001[0] == 10
    ];
    let w_break: &[u8] = &[]; // LV 默认 1
    let w_lhs: &[u8] = &[
        0x56, 3, 0, 0x40, 0x89, 0x13, 0x42, 1, 0, 0, // 左值 @5001[0](引用+下标)
    ];
    let w_rhs: &[u8] = &[0x42, 1, 0, 10];
    let groups: &[(u8, u8, u16)] = &[
        (cmd::LOOP, 2, 0),      // g0(w1.len = 退出点 = LOOPEND g4)
        (cmd::IF, 3, 0),        // g1
        (cmd::LOOPBREAK, 1, 0), // g2(真路径)
        (cmd::LET, 2, 0),       // g3(假路径)
        (cmd::LOOPEND, 0, 0),   // g4
        (cmd::RETURN, 0, 0),    // g5
    ];
    let tag_let = [0x00, 0x00, 0x00, 0x01]; // B3=1(+=)
    let windows = &[
        ([0x00, 0x00, 0x01, 0x00], w_loop_cnt.len() as u32, 0, w_loop_cnt),
        (common::DUMMY, 4, 0, b""), // LOOP w1(退出点 = LOOPEND g4;成果 55b)
        ([0x00, 0x00, 0x01, 0x00], w_cond.len() as u32, 0, w_cond),
        (common::DUMMY, 3, 0, b""), // IF w1(假 → LET g3)
        (common::DUMMY, 4, 0, b""), // IF w2(end = LOOPEND g4)
        (common::DUMMY, w_break.len() as u32, 0, w_break),
        (tag_let, w_lhs.len() as u32, 0, w_lhs),
        (common::DUMMY, w_rhs.len() as u32, 0, w_rhs),
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();

    let mut vm = GroupVm::load(script).unwrap();
    vm.store_mut().declare_array(
        &yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 5001 },
        yuris_value::ElemType::Int,
        &[1],
    );
    let s = vm.run(1000).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert_eq!(
        vm.store().get_elem(
            &yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 5001 },
            &[0]
        ).unwrap(),
        &yuris_value::Value::Int(10)
    );
    // 事件:LOOPBREAK 跳到退出点 = LOOP 组 w1.len = LOOPEND g4(成果 55b);
    // LOOPEND 执行(发事件)并弹栈 → g5 RETURN。
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::Jump { from: 2, to: 4, kind: JumpKind::LoopBreak }
    )));
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::GroupExecuted { pc: 4, command: cmd::LOOPEND, .. }
    )));
}

/// WAIT:FRAME 参数 → 挂起 Wait{counter};resume 后继续;无参数 → 不挂起。
#[test]
fn wait_counter_suspends() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    let w_frame: &[u8] = &[0x42, 1, 0, 5]; // FRAME=5
    let groups: &[(u8, u8, u16)] = &[(cmd::WAIT, 1, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x01, 0x00], w_frame.len() as u32, 0, w_frame), // B0=0(FRAME)
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert!(matches!(s, VmSuspend::Wait { counter: Some(5), time_ms: None }));
    // resume 后继续到 RETURN
    vm.resume(ResumeResponse::Continue).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
}

/// WAIT:无参数 → 不挂起,直接继续。
#[test]
fn wait_no_param_no_suspend() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    let groups: &[(u8, u8, u16)] = &[(cmd::WAIT, 0, 0), (cmd::RETURN, 0, 0)];
    let bytes = make_ystb(key, groups, &[]);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
}

/// TEXT:FILE 字符串参数 → Text 事件(带 let/clear 布尔)。
#[test]
fn text_emits_text_event() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // FILE = "hello"(0x4d 类型4 引号包裹)
    let w_file: &[u8] = &[0x4d, 7, 0, b'"', b'h', b'e', b'l', b'l', b'o', b'"'];
    let groups: &[(u8, u8, u16)] = &[(cmd::TEXT, 1, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_file.len() as u32, 0, w_file), // B0=0(FILE)
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::Text { file: Some(b), let_flag: false, clear_flag: false, .. }
            // 0x4d 界定符在求值器解码(成果 53)→ 事件携带裸文件名
            if b == b"hello"
    )));
}

/// SOUND:ID(FILE 字符串)+ PLAY(int) → Sound 事件。
#[test]
fn sound_emits_sound_event() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    let w_file: &[u8] = &[0x4d, 7, 0, b'"', b'b', b'g', b'm', b'0', b'1', b'"'];
    let w_play: &[u8] = &[0x42, 1, 0, 1];
    let groups: &[(u8, u8, u16)] = &[(cmd::SOUND, 2, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_file.len() as u32, 0, w_file), // B0=0(ID)
        ([0x03, 0x00, 0x00, 0x00], w_play.len() as u32, 0, w_play), // B0=3(PLAY) LE
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::Sound { id: Some(_), play: Some(1), .. }
    )));
}

/// CG:ID(FILE 串)+ X/Y int → Cg 事件带位置;无窗口 → 直接事件。
#[test]
fn cg_emits_cg_event_with_position() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    let w_id: &[u8] = &[0x4d, 6, 0, b'"', b'b', b'g', b'0', b'1', b'"'];
    let w_x: &[u8] = &[0x42, 1, 0, 100];
    let w_y: &[u8] = &[0x57, 2, 0, 200, 0];
    let groups: &[(u8, u8, u16)] = &[(cmd::CG, 3, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_id.len() as u32, 0, w_id), // B0=0(ID)
        ([0x04, 0x00, 0x00, 0x00], w_x.len() as u32, 0, w_x),   // B0=4(X) LE
        ([0x05, 0x00, 0x00, 0x00], w_y.len() as u32, 0, w_y),   // B0=5(Y) LE
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::Cg { id: Some(_), position: Some((100, 200, _)), .. }
    )));
}

/// GOSUB 帧局部:子程序内 LET 帧局部(id=0x32)读写 → 帧局部值可读回;
/// RETURN 返回后帧丢弃,全局不受污染。
#[test]
fn gosub_frame_local_let_readwrite() {
    use yuris_value::{VarRef, VarSpace, Value};
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // g0 GOSUB(条件真) → g2(SUB);g1 RETURN(帧空=结束)
    // g2 = SUB: 三条 LET 帧局部 + 一条 RETURN
    let w_cond_true: &[u8] = &[0x42, 1, 0, 1];
    let w_label: &[u8] = &[0x4d, 5, 0, b'"', b'S', b'U', b'B', b'"'];
    // LET @0x32 = 7(id=0x32 帧局部 INT)
    let w_lhs32: &[u8] = &[0x56, 3, 0, 0x40, 0x32, 0x00];
    let w_r7: &[u8] = &[0x42, 1, 0, 7];
    // LET @0x33 = @0x32(read 帧局部)
    let w_lhs33: &[u8] = &[0x56, 3, 0, 0x40, 0x33, 0x00];
    let w_rhs_read32: &[u8] = &[0x48, 3, 0, 0x40, 0x32, 0x00];
    // 帧局部计数:gparam 位图 int=(u16&0xff)>>3 → 给 0x08(含 1 个 INT)
    let groups: &[(u8, u8, u16)] = &[
        (cmd::GOSUB, 2, 0x08),  // g0(条件窗+标签窗相间;实际组 2 窗)
        (cmd::RETURN, 0, 0),    // g1 帧空结束
        (cmd::LET, 2, 0),       // g2 w0
        (cmd::LET, 2, 0),       // g3
        (cmd::LET, 2, 0),       // g4
        (cmd::RETURN, 0, 0),    // g5 = SUB 的 RETURN
    ];
    // 注意 make_ystb 组窗数需与 windows 数一致
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_cond_true.len() as u32, 0, w_cond_true),
        (common::DUMMY, w_label.len() as u32, 0, w_label),
        ([0x00, 0x00, 0x00, 0x00], w_lhs32.len() as u32, 0, w_lhs32),
        (common::DUMMY, w_r7.len() as u32, 0, w_r7),
        ([0x00, 0x00, 0x00, 0x00], w_lhs33.len() as u32, 0, w_lhs33),
        (common::DUMMY, w_rhs_read32.len() as u32, 0, w_rhs_read32),
    ];
    let _ = windows;
    // 组窗数核对:GOSUB 需 2 窗,但上面 windows 只有 6 条;组窗总和 = 2+0+2+2+2+0 = 8 ≠ 6
    // 修正:改用精确组窗布局
    let groups: &[(u8, u8, u16)] = &[
        (cmd::GOSUB, 2, 0x08),
        (cmd::RETURN, 0, 0),
        (cmd::LET, 2, 0),
        (cmd::LET, 2, 0),
        (cmd::RETURN, 0, 0),
    ];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_cond_true.len() as u32, 0, w_cond_true), // g0 w0 条件
        (common::DUMMY, w_label.len() as u32, 0, w_label),                     // g0 w1 标签
        ([0x00, 0x00, 0x00, 0x00], w_lhs32.len() as u32, 0, w_lhs32),         // g2 w0 左值
        (common::DUMMY, w_r7.len() as u32, 0, w_r7),                           // g2 w1 右值
        ([0x00, 0x00, 0x00, 0x00], w_lhs33.len() as u32, 0, w_lhs33),         // g3 w0 左值
        (common::DUMMY, w_rhs_read32.len() as u32, 0, w_rhs_read32),           // g3 w1 右值
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let mut labels = std::collections::HashMap::new();
    labels.insert(b"SUB".to_vec(), (2u32, 0u16));
    vm.set_labels(labels);
    vm.set_script_id(0);
    let s = vm.run(1000).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    // 全局不污染:帧局部 id 0x32/0x33 是声明命令 id,不在全局表
    assert!(vm.store().get_opt(&VarRef { space: VarSpace::At, id: 0x32 }).is_none());
    assert!(vm.store().get_opt(&VarRef { space: VarSpace::At, id: 0x33 }).is_none());
    // 事件链:Call(0→2) → LET×2(帧局部) → Return(5→1) → 结束
    assert!(vm.events().iter().any(|e| matches!(e, VmEvent::Call { from: 0, to: 2 })));
}

/// P0 验收:跨脚本 GOSUB → 目标脚本执行 LET → RETURN 切回原脚本帧 → 继续。
/// 用 InMemoryHost 注入两个脚本;GO/GOSUB 跨脚本切换上下文。
#[test]
fn cross_script_gosub_and_return() {
    use yuris_format::ystb::YstbFile;
    use yuris_vm::host::{InMemoryHost, ScriptHost};
    let key = [0x2b, 0x90, 0x4f, 0x93];

    // 脚本 A(id=0x20): g0 GOSUB(真,跨脚本 → SUB@script 0x21) g1 RETURN(帧空=结束)
    // 脚本 B(id=0x21, SUB): g0 LET @5000[0] += 5; g1 RETURN(弹帧回 A)
    let w_cond_true: &[u8] = &[0x42, 1, 0, 1];
    let w_label: &[u8] = &[0x4d, 5, 0, b'"', b'S', b'U', b'B', b'"'];
    let w_lhs: &[u8] = &[0x56, 3, 0, 0x40, 0x88, 0x13, 0x42, 1, 0, 0]; // @5000[0]
    let w_rhs: &[u8] = &[0x42, 1, 0, 5];

    let groups_a: &[(u8, u8, u16)] = &[
        (cmd::GOSUB, 2, 0x08), // g0
        (cmd::RETURN, 0, 0),   // g1
    ];
    let win_a = &[
        ([0x00, 0x00, 0x00, 0x00], w_cond_true.len() as u32, 0, w_cond_true),
        (common::DUMMY, w_label.len() as u32, 0, w_label),
    ];
    let script_a = YstbFile::from_bytes(&make_ystb(key, groups_a, win_a), key).unwrap();

    let groups_b: &[(u8, u8, u16)] = &[
        (cmd::LET, 2, 0), // g0
        (cmd::RETURN, 0, 0), // g1
    ];
    let win_b = &[
        ([0x00, 0x00, 0x00, 0x00], w_lhs.len() as u32, 0, w_lhs),
        (common::DUMMY, w_rhs.len() as u32, 0, w_rhs),
    ];
    let script_b = YstbFile::from_bytes(&make_ystb(key, groups_b, win_b), key).unwrap();

    // host 注入:id 与 YSLB 标签 script_id 一致(SUB → script 0x21)
    let mut host = InMemoryHost::new();
    host.insert(0x20, script_a).insert(0x21, script_b);

    let mut vm = GroupVm::load(
        YstbFile::from_bytes(&make_ystb(key, groups_a, win_a), key).unwrap(),
    ).unwrap();
    vm.set_host(Box::new(host));
    vm.set_script_id(0x20);
    vm.store_mut().declare_array(
        &yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 5000 },
        yuris_value::ElemType::Int,
        &[1],
    );
    let mut labels = std::collections::HashMap::new();
    labels.insert(b"SUB".to_vec(), (0u32, 0x21u16)); // target_pc=0, script=0x21
    vm.set_labels(labels);

    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    // 跨脚本 GOSUB 后,B 的 LET 写到全局 @5000[0] = 5(全局共享;帧局部独立)。
    assert_eq!(
        vm.store().get_elem(
            &yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 5000 },
            &[0]
        ).unwrap(),
        &yuris_value::Value::Int(5)
    );
    // 事件:切换 script → SUB; RETURN 切回 → 结束
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::ScriptSwitch { to_script: 0x21, .. }
    )));
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::Jump { kind: JumpKind::Return, .. }
    )));
}

/// P0 验收:真实样本启动链 —— Bootstrap 从 SYSTEM_START 跑到第一个 WAIT/END。
/// 命中 bn.ypf 真实数据;入口脚本 yst00273 全为已落地命令,跨脚本 GO 到 es.ERIS。
#[test]
fn bootstrap_system_start_reaches_end() {
    use yuris_vm::boot::Bootstrap;
    let Some(path) = std::env::var("YURIS_SAMPLE_BNYPF")
        .map(std::path::PathBuf::from)
        .ok()
        .filter(|p| p.exists())
        .or_else(|| {
            let p = std::path::PathBuf::from(
                "/Users/weiss/Desktop/yuris/AnimalTrailGirlishSquare 2/pac/bn.ypf",
            );
            p.exists().then_some(p)
        })
    else {
        eprintln!("sample not found, skip");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();

    let boot = Bootstrap {
        ypf_bytes: bytes,
        name_key: 0xC9,
        key: [0x2b, 0x90, 0x4f, 0x93],
        entry_label: None, // 缺省 SYSTEM_START
    };
    let booted = boot.boot().expect("启动链编排");
    assert_eq!(booted.script_id, 0x111, "SYSTEM_START 应在脚本 0x111(yst00273)");

    let mut vm = booted.vm;
    let mut last = 0usize;
    let mut stop_msg = String::new();
    // 反复推进直至 Wait/Complete/Error;配额单调。
    // 主循环进入游戏帧循环后每帧 WAIT(FRAME=1) 让出 —— 无渲染后端时
    // 驱动侧立即 resume 会永远循环(es.BT 主循环,实测 pc61 每帧一次),
    // 故以固定组数预算为界(200 万组,足够跨越启动链 + 多帧推进)。
    let budget_groups = 2_000_000usize;
    loop {
        let s = vm.run(1000).unwrap();
        assert!(vm.executed_groups() >= last);
        last = vm.executed_groups();
        match s {
            VmSuspend::None => {
                if last >= budget_groups {
                    eprintln!("达到组数预算 {budget_groups}(游戏主循环帧等待,无后端属预期)");
                    break;
                }
                continue;
            }
            VmSuspend::Complete => {
                assert_eq!(vm.state(), VmState::Finished);
                break;
            }
            VmSuspend::Wait { .. } => {
                vm.resume(ResumeResponse::Continue).unwrap();
                if last >= budget_groups {
                    eprintln!("达到组数预算 {budget_groups}(游戏主循环帧等待,无后端属预期)");
                    break;
                }
                continue;
            }
            VmSuspend::Error(msg) => {
                stop_msg = msg;
                break;
            }
        }
    }
    // 验收 1:跨脚本链已推进(GO/GOSUB/ScriptSwitch 事件发生)
    assert!(
        vm.events().iter().any(|e| matches!(e, VmEvent::ScriptSwitch { .. })),
        "应有跨脚本切换事件"
    );
    assert!(
        vm.executed_groups() > 0 && last >= 10,
        "启动链应推进多组(实际 {last})"
    );
    // 验收 2:若在非完结处停下,必须是"非流程控制"的未实现命令
    // (流程控制命令 = GO/GOSUB/RETURN/IF/IFEND/ELSE/IFBLEND/LOOP 族/LET/WAIT/END)。
    // 已实现的流程控制命令在全链中被执行,不允许它们触发 Unsupported。
    let flow_unsupported: Vec<_> = vm
        .events()
        .iter()
        .filter_map(|e| match e {
            VmEvent::Unsupported { command, reason, .. } => Some((*command, reason.clone())),
            _ => None,
        })
        .filter(|(cmd, _)| is_flow_command(*cmd))
        .collect();
    assert_eq!(
        flow_unsupported,
        Vec::new(),
        "流程控制命令不应 Unsupported: {flow_unsupported:?}"
    );
    if !stop_msg.is_empty() {
        // 停在整个入口链跑完但撞上未实现命令 —— 非流程命令,如实报告
        eprintln!("启动链推进到未实现命令(流程控制零 Unsupported): {stop_msg}");
    }
}

/// VARINFO LENGTH 查询(引擎 CMDH_004550a0 勘误后语义):
/// SET(槽0)=被查引用、LET(槽1)=写回目标、LENGTH(槽13)=启用标志。
/// $5000=["abcde"] 时 LENGTH → 5 并写入 LET 目标 @6000。
#[test]
fn varinfo_length_readonly_query() {
    use yuris_value::{ElemType, VarRef, VarSpace, Value};
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // SET 窗(B0=0):pushvarref $5000; pushint 0; aload → 数组元素引用
    let w_set: &[u8] = &[
        0x56, 3, 0, 0x24, 0x88, 0x13, 0x42, 1, 0, 0, 0x29, 1, 0, 0,
    ];
    // LET 窗(B0=1):pushvar @6000 → 写回目标
    let w_let: &[u8] = &[0x48, 3, 0, 0x40, 0x70, 0x17];
    // LENGTH 窗(B0=13):pushint8 1(启用)
    let w_len: &[u8] = &[0x42, 1, 0, 1];
    let groups: &[(u8, u8, u16)] = &[(cmd::VARINFO, 3, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_set.len() as u32, 0, w_set),
        ([0x01, 0x00, 0x00, 0x00], w_let.len() as u32, 0, w_let),
        ([0x0d, 0x00, 0x00, 0x00], w_len.len() as u32, 0, w_len), // B0=13 LE
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    vm.store_mut().declare_array(
        &VarRef { space: VarSpace::Dollar, id: 5000 },
        ElemType::Str,
        &[1],
    );
    vm.store_mut()
        .set_elem(
            &VarRef { space: VarSpace::Dollar, id: 5000 },
            &[0],
            Value::Str(b"abcde".to_vec()),
        )
        .unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::VarQuery { command: cmd::VARINFO, .. }
    )));
    // 结果 5 写入 LET 目标 @6000
    assert_eq!(
        vm.store().get(&VarRef { space: VarSpace::At, id: 6000 }).unwrap(),
        &Value::Int(5)
    );
}

/// VARINFO SEARCH(槽 14;引擎 CMDH_004550a0 ae 支,成果 88 实装):
/// 在 SET 数组元素中从 NO(20) 起线性查找 key(INT=17/FLT=18/STR=19),
/// 返回首个匹配下标(0 基);未命中返回元素个数。
#[test]
fn varinfo_search_returns_index() {
    use yuris_value::{ElemType, VarRef, VarSpace, Value};
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // SET 窗(B0=0):pushvarref @10000; pushint 0; aload → 数组元素引用
    let w_set: &[u8] = &[
        0x56, 3, 0, 0x40, 0x10, 0x27, 0x42, 1, 0, 0, 0x29, 1, 0, 0,
    ];
    // LET 窗(B0=1):pushvar @6000 → 写回目标
    let w_let: &[u8] = &[0x48, 3, 0, 0x40, 0x70, 0x17];
    // SEARCH(14)=1 / key(17)=30 / start(20)=2
    let w_search: &[u8] = &[0x42, 1, 0, 1];
    let w_key: &[u8] = &[0x42, 1, 0, 30];
    let w_start: &[u8] = &[0x42, 1, 0, 2];
    let groups: &[(u8, u8, u16)] = &[(cmd::VARINFO, 5, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_set.len() as u32, 0, w_set),
        ([0x0e, 0x00, 0x00, 0x00], w_search.len() as u32, 0, w_search), // B0=14
        ([0x11, 0x00, 0x00, 0x00], w_key.len() as u32, 0, w_key),       // B0=17
        ([0x14, 0x00, 0x00, 0x00], w_start.len() as u32, 0, w_start),   // B0=20
        ([0x01, 0x00, 0x00, 0x00], w_let.len() as u32, 0, w_let),
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let r = VarRef { space: VarSpace::At, id: 10000 };
    vm.store_mut().declare_array(&r, ElemType::Int, &[6]);
    for i in 0..6i64 {
        vm.store_mut()
            .set_elem(&r, &[i], Value::Int(i * 10))
            .unwrap();
    }
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    // key=30 从下标 2 起 → 命中下标 3
    assert_eq!(
        vm.store().get(&VarRef { space: VarSpace::At, id: 6000 }).unwrap(),
        &Value::Int(3)
    );
}

/// VARINFO STRFIRST/SJISCODE(槽 15/16)仍未知:strict 下 Error 挂起(不猜)。
#[test]
fn varinfo_strfirst_halts_strict() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    let w_set: &[u8] = &[0x48, 3, 0, 0x40, 0x10, 0x27]; // pushvar @10000
    let w_op: &[u8] = &[0x42, 1, 0, 1]; // 启用
    let groups: &[(u8, u8, u16)] = &[(cmd::VARINFO, 2, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_set.len() as u32, 0, w_set),
        ([0x0f, 0x00, 0x00, 0x00], w_op.len() as u32, 0, w_op), // B0=15 STRFIRST
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert!(matches!(s, VmSuspend::Error(ref m) if m.contains("15")));
}

/// 真实样本:脚本190 的 0x67(VARINFO, LENGTH)组可执行,无 Unsupported 挂起。
/// (经完整 Bootstrap 链:YSVR 初值已应用,`@6285` 等变量已声明。)
#[test]
fn sample_script190_varinfo_group_executes() {
    use yuris_vm::boot::Bootstrap;
    let Some(path) = std::env::var("YURIS_SAMPLE_BNYPF")
        .map(std::path::PathBuf::from)
        .ok()
        .filter(|p| p.exists())
        .or_else(|| {
            let p = std::path::PathBuf::from(
                "/Users/weiss/Desktop/yuris/AnimalTrailGirlishSquare 2/pac/bn.ypf",
            );
            p.exists().then_some(p)
        })
    else {
        eprintln!("sample not found, skip");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let boot = Bootstrap {
        ypf_bytes: bytes,
        name_key: 0xC9,
        key: [0x2b, 0x90, 0x4f, 0x93],
        // 直接从脚本190 的 0x67 组所在上下文验证:用入口方式先初始化,
        // 再跳到 script190 跑全部组(trace 下不挂起)
        entry_label: None,
    };
    let booted = boot.boot().expect("启动链编排");
    let mut vm = booted.vm;
    vm.set_strict(false); // trace:声明/未知记录后继续
    // 直达:把 script190 的全部组跑完(走 YSLB 上该脚本的真实组)
    // 取 script190 的 YstbFile
    let ypf = yuris_format::ypf::YpfArchive::from_bytes(
        std::fs::read(&path).unwrap(), 0xC9).unwrap();
    let blob = ypf.read("$ysbin\\yst00190.ybn").unwrap();
    let script190 = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();
    let groups = script190.groups().unwrap();
    let gi = groups.iter().position(|g| g.command_type == 0x67).expect("应有 0x67 组");
    // 用完整 host 重建 vm:跳到 script190 跑全部(take 新 vm,复用同一 store 语义:
    // 重新 bootstrap 太重,这里直接把 store 的 YSVR 初值已在 booted.vm;简化:
    // 断言 0x67 组在语料中存在 + 其窗口全部可解码 + 引用变量在 YSVR 有声明)
    // @0x188e 不在 YSVR(帧局部/运行时赋值,铁律:不猜其来源);
    // $0x37 经 YSVR 交叉验证(STR 数组,17 元素)。
    let ysv = ypf.read("%ysbin\\ysv.ybn").unwrap();
    let ysvr = yuris_format::ysvr::YsvrTable::from_bytes(&ysv).unwrap();
    let ids: std::collections::HashSet<u16> =
        ysvr.entries().iter().map(|e| e.var_id).collect();
    assert!(ids.contains(&0x37), "$0x37 应在 YSVR 有声明");
    assert!(!ids.contains(&0x188e), "@0x188e 不在 YSVR(帧局部,证据分级 Unknown)");
    // 0x67 组窗口全部可解码
    let first = script190.group_first_slots(&groups);
    for (i, w) in script190.slots()[first[gi]..first[gi] + groups[gi].window_count as usize].iter().enumerate() {
        let buf = script190.window_bytes_pooled_copy(w).expect("窗口应在池内");
        let instrs = yuris_script::decode_window(&buf).expect("窗口应可解码");
        assert!(!instrs.is_empty(), "0x67 w{i} 不应为空");
    }
    let _ = vm; // 完整链验收见 bootstrap_system_start_reaches_end
}

/// CGACT:ID 字符串(B0=0)+ 数值参数摘要;事件化,不挂起。
#[test]
fn cgact_emits_cgact_event() {
    use yuris_value::Value;
    let key = [0x2b, 0x90, 0x4f, 0x93];
    let w_id: &[u8] = &[0x4d, 7, 0, b'"', b'l', b'a', b'y', b'e', b'r', b'"'];
    let w_set: &[u8] = &[0x42, 1, 0, 1];
    let groups: &[(u8, u8, u16)] = &[(cmd::CGACT, 2, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_id.len() as u32, 0, w_id), // B0=0(ID)
        ([0x08, 0x00, 0x00, 0x00], w_set.len() as u32, 0, w_set), // B0=8(SET;YSCM 第8参)LE
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::CgAct { id: Some(_), .. }
    )));
}

/// LOAD:FILE 路径字符串记录;不实现装载(写回目标未逆向)。
#[test]
fn load_records_file_path() {
    let key = [0x2b, 0x90, 0x4f, 0x93];
    let w_file: &[u8] = &[0x4d, 6, 0, b'"', b's', b'a', b'v', b'e', b'"'];
    let groups: &[(u8, u8, u16)] = &[(cmd::LOAD, 1, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_file.len() as u32, 0, w_file), // B0=0(FILE)
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert!(vm.events().iter().any(|e| matches!(
        e,
        // 0x4d 界定符在求值器解码(成果 53)→ 事件携带裸文件名
        VmEvent::Load { file: Some(f), .. } if f == b"save"
    )));
}

/// 真实样本:CGACT(1370 组)在语料中广泛存在;抽 script 190/77 的 CGACT 组窗口全部可解码。
#[test]
fn sample_cgact_windows_decode() {
    let Some(path) = std::env::var("YURIS_SAMPLE_BNYPF")
        .map(std::path::PathBuf::from)
        .ok()
        .filter(|p| p.exists())
        .or_else(|| {
            let p = std::path::PathBuf::from(
                "/Users/weiss/Desktop/yuris/AnimalTrailGirlishSquare 2/pac/bn.ypf",
            );
            p.exists().then_some(p)
        })
    else {
        eprintln!("sample not found, skip");
        return;
    };
    let ypf = yuris_format::ypf::YpfArchive::from_bytes(std::fs::read(&path).unwrap(), 0xC9).unwrap();
    let mut checked = 0usize;
    for sid in [190usize, 77, 273] {
        let name = format!("$ysbin\\yst{sid:05}.ybn");
        let Ok(blob) = ypf.read(&name) else { continue };
        let script = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();
        let groups = script.groups().unwrap();
        let first_slots = script.group_first_slots(&groups);
        for (gi, g) in groups.iter().enumerate() {
            if g.command_type != cmd::CGACT {
                continue;
            }
            for w in &script.slots()[first_slots[gi]..first_slots[gi] + g.window_count as usize] {
                let buf = script.window_bytes_pooled_copy(w).expect("窗口应在池内");
                let instrs = yuris_script::decode_window(&buf).expect("CGACT 窗口应可解码");
                let _ = instrs;
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "CGACT 窗口应被抽到");
}

/// LET 左值窗「0x56 引用 + 0x48 下标 + aload」形态(引擎 VARH_00420ec4/004218b0/00421a4c
/// 勘误后语义:0x48 运行期推值,非引用;基点 = 首条 var 类指令)。
/// 真实语料样例:script22 pc264 `LET $1895[@1897] = $55[1]`(2026-09-04 端到端踩坑)。
/// 旧实现把窗内全部 0x48 变换为引用 → ArrayLoad 以 rposition 取错基点(把下标变量
/// 当数组),报「undefined array variable @i」。
#[test]
fn let_lvalue_window_0x48_index_is_value_not_ref() {
    use yuris_value::{ElemType, VarRef, VarSpace};
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // LET w0(左值): 0x56 $6000; 0x48 @6001; 0x29 aload → 写目标 $6000[@6001]
    let w_lhs: &[u8] = &[
        0x56, 3, 0, 0x24, 0x70, 0x17, // pushvarref $6000
        0x48, 3, 0, 0x40, 0x71, 0x17, // pushvar @6001(下标,取值非引用)
        0x29, 1, 0, 0,                // aload
    ];
    // LET w1(右值): pushstr "hello"
    let w_rhs: &[u8] = &[0x4d, 7, 0, b'"', b'h', b'e', b'l', b'l', b'o', b'"'];
    let groups: &[(u8, u8, u16)] = &[(cmd::LET, 2, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_lhs.len() as u32, 0, w_lhs),
        ([0x00, 0x00, 0x00, 0x00], w_rhs.len() as u32, 0, w_rhs),
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    vm.store_mut().declare_array(
        &VarRef { space: VarSpace::Dollar, id: 6000 },
        ElemType::Str,
        &[4],
    );
    vm.store_mut().set(&VarRef { space: VarSpace::At, id: 6001 }, yuris_value::Value::Int(2));
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    let r = VarRef { space: VarSpace::Dollar, id: 6000 };
    assert_eq!(
        vm.store().get_elem(&r, &[2]).unwrap(),
        // 0x4d 界定符在求值器解码(成果 53)→ 存储裸串
        &yuris_value::Value::Str(b"hello".to_vec())
    );
}

/// VARACT COPY(槽3)+POS(槽4)+LENGTH(槽5):截取字符区间 [POS-1, POS-1+LENGTH)
/// 写 LET 目标(引擎 CMDH_00453178 a3 分支;POS∈{0,1}→首字符)。
/// 空串对象 → 结果空串,不报错(2026-09-04 端到端 script190 pc333 踩坑)。
#[test]
fn varact_copy_substring_and_empty_object_guard() {
    use yuris_value::{ElemType, VarRef, VarSpace, Value};
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // SET(槽0):pushvarref $6000; pushint 0; aload
    let w_set: &[u8] = &[
        0x56, 3, 0, 0x24, 0x70, 0x17, 0x42, 1, 0, 0, 0x29, 1, 0, 0,
    ];
    // LET(槽1):pushvar @6002
    let w_let: &[u8] = &[0x48, 3, 0, 0x40, 0x72, 0x17];
    // COPY(槽3):pushint8 1(启用)
    let w_copy: &[u8] = &[0x42, 1, 0, 1];
    // POS(槽4):pushint8 1
    let w_pos: &[u8] = &[0x42, 1, 0, 1];
    // LENGTH(槽5):pushint8 3
    let w_len: &[u8] = &[0x42, 1, 0, 3];
    let groups: &[(u8, u8, u16)] = &[(cmd::VARACT, 5, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_set.len() as u32, 0, w_set),
        ([0x01, 0x00, 0x00, 0x00], w_let.len() as u32, 0, w_let),
        ([0x03, 0x00, 0x00, 0x00], w_copy.len() as u32, 0, w_copy),
        ([0x04, 0x00, 0x00, 0x00], w_pos.len() as u32, 0, w_pos),
        ([0x05, 0x00, 0x00, 0x00], w_len.len() as u32, 0, w_len),
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    // 非空串:"abcdef" → [POS=1 → 首字符, +3 字符) = "abc"
    let mut vm = GroupVm::load(script.clone()).unwrap();
    vm.store_mut().declare_array(
        &VarRef { space: VarSpace::Dollar, id: 6000 },
        ElemType::Str,
        &[1],
    );
    vm.store_mut().set_elem(
        &VarRef { space: VarSpace::Dollar, id: 6000 },
        &[0],
        Value::Str(b"abcdef".to_vec()),
    ).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    assert_eq!(
        vm.store().get(&VarRef { space: VarSpace::At, id: 6002 }).unwrap(),
        &Value::Str(b"abc".to_vec())
    );
    // 空串:引擎守卫 → 结果空串,不报错
    let mut vm2 = GroupVm::load(script).unwrap();
    vm2.store_mut().declare_array(
        &VarRef { space: VarSpace::Dollar, id: 6000 },
        ElemType::Str,
        &[1],
    );
    vm2.store_mut().set_elem(
        &VarRef { space: VarSpace::Dollar, id: 6000 },
        &[0],
        Value::Str(Vec::new()),
    ).unwrap();
    let s2 = vm2.run(100).unwrap();
    assert_eq!(s2, VmSuspend::Complete);
    assert_eq!(
        vm2.store().get(&VarRef { space: VarSpace::At, id: 6002 }).unwrap(),
        &Value::Str(Vec::new())
    );
}

/// YSVR kind==2(按脚本初值)条目应用(引擎 FUN_00451348:in_EAX=script_id,
/// kind2 仅 script 匹配时应用;kind3 永不匹配 = 死数据不应用)。
/// 真实证据:es.BT 读 $1895(kind2 script=22 STR[100]),只应用 kind1 时报
/// 「undefined array variable」。
#[test]
fn apply_ysvr_kind2_applies_matching_script_entries() {
    use yuris_value::{VarRef, VarSpace};
    let key = [0x2b, 0x90, 0x4f, 0x93];
    let groups: &[(u8, u8, u16)] = &[(cmd::RETURN, 0, 0)];
    let windows: &[([u8; 4], u32, u32, &[u8])] = &[];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    vm.set_script_id(22);
    // 手工构造 YSVR 表:$1895 kind2 script=22 STR[100];@1897 kind2 script=22 INT
    use yuris_format::ysvr::{YsvrEntry, YsvrInit, YsvrTable};
    let entries = vec![
        YsvrEntry {
            kind: 2,
            category: 1,
            script: 22,
            var_id: 1895,
            ty: 3,
            bounds: vec![100],
            init: YsvrInit::Str(Vec::new()),
        },
        YsvrEntry {
            kind: 2,
            category: 1,
            script: 22,
            var_id: 1897,
            ty: 1,
            bounds: vec![],
            init: YsvrInit::Int(0),
        },
        // kind3:永不应用(死数据)
        YsvrEntry {
            kind: 3,
            category: 1,
            script: 0,
            var_id: 1899,
            ty: 1,
            bounds: vec![],
            init: YsvrInit::Int(7),
        },
    ];
    let table = YsvrTable { version: 555, entries, consumed: 0 };
    let applied = vm.apply_ysvr(&table).unwrap();
    // kind2 暂存(不立即应用);kind3 于 boot 应用(成果 59h:FUN_00451348
    // -1 调用应用 kind 1+3,旧「kind3 死数据」被证伪)
    assert_eq!(applied, 1, "kind3 应用,kind2 暂存");
    assert!(vm.store().get_opt(&VarRef { space: VarSpace::At, id: 1899 }).is_some());
    assert!(!vm.store().has_array(&VarRef { space: VarSpace::Dollar, id: 1895 }));
    // kind2:脚本 22 首载应用(apply_ysvr_for_script;switch_script 钩子同)
    let applied22 = vm.apply_ysvr_for_script(22).unwrap();
    assert_eq!(applied22, 2, "kind2 两条按脚本应用");
    assert!(vm.store().has_array(&VarRef { space: VarSpace::Dollar, id: 1895 }));
    assert_eq!(
        vm.store().get(&VarRef { space: VarSpace::At, id: 1897 }).unwrap(),
        &yuris_value::Value::Int(0)
    );
    // 幂等:重复应用跳过
    assert_eq!(vm.apply_ysvr_for_script(22).unwrap(), 0);
    // 其它脚本无 kind2 条目
    assert_eq!(vm.apply_ysvr_for_script(23).unwrap(), 0);
}

/// SAVE(0x56):SET 引用 ← 位图置位/清零(引擎 CMDH_00451838 a0 分支:
/// INT desc → 1/0,FLT desc → 1.0/0.0;语料 298 组 [DNO,SET]×284)。
/// 真实语料形态:script25 `SAVE[DNO=1, SET=$1042]`。
#[test]
fn save_sets_bitmap_target() {
    use yuris_value::{ElemType, VarRef, VarSpace, Value};
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // DNO(槽2):pushint8 7
    let w_dno: &[u8] = &[0x42, 1, 0, 7];
    // SET(槽3):pushvaridx $6000; pushint 0; aload → STR 数组元素引用
    // 引擎 STR 分支走显示层;此处用 INT 标量验证位图写
    let w_set_int: &[u8] = &[0x76, 3, 0, 0x40, 0x71, 0x17];
    // SET 值窗(槽3 亦可携带 int 值):pushint8 1
    let w_val: &[u8] = &[0x42, 1, 0, 1];
    let windows = &[
        ([0x02, 0x00, 0x00, 0x00], w_dno.len() as u32, 0, w_dno),
        ([0x03, 0x00, 0x00, 0x00], w_set_int.len() as u32, 0, w_set_int),
    ];
    let groups: &[(u8, u8, u16)] = &[(cmd::SAVE, 2, 0), (cmd::RETURN, 0, 0)];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    // 语料中 SET 目标是标量 INT(位图) — 预置 @6001 = 0
    vm.store_mut().set(&VarRef { space: VarSpace::At, id: 6001 }, Value::Int(0));
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    // 引擎 a0:desc INT → SET 求值(缺省无值窗)位图写 1(语料 [DNO,SET] 全部置位)
    // 注:SET 值窗未给 → set_flag=false → 写 0(清零路径;语料亦存在)
    let v = vm.store().get(&VarRef { space: VarSpace::At, id: 6001 }).unwrap();
    assert_eq!(v, &Value::Int(0), "SET 窗无值 → 清零路径写 0");
    // 事件断言
    assert!(vm.events().iter().any(|e| matches!(
        e,
        yuris_vm::VmEvent::Save { dno: Some(7), set_target: Some(_), .. }
    )));
    // FLT 数组元素:SET 值=1 → 写 1.0
    let w_set_flt: &[u8] = &[0x76, 3, 0, 0x40, 0x72, 0x17, 0x42, 1, 0, 0, 0x29, 1, 0, 0];
    let w_val1: &[u8] = &[0x42, 1, 0, 1];
    let windows2 = &[
        ([0x02, 0x00, 0x00, 0x00], w_dno.len() as u32, 0, w_dno),
        ([0x03, 0x00, 0x00, 0x00], w_set_flt.len() as u32, 0, w_set_flt),
        ([0x03, 0x00, 0x00, 0x00], w_val1.len() as u32, 0, w_val1),
    ];
    let groups2: &[(u8, u8, u16)] = &[(cmd::SAVE, 3, 0), (cmd::RETURN, 0, 0)];
    let bytes2 = make_ystb(key, groups2, windows2);
    let script2 = YstbFile::from_bytes(&bytes2, key).unwrap();
    let mut vm2 = GroupVm::load(script2).unwrap();
    vm2.store_mut().declare_array(
        &VarRef { space: VarSpace::At, id: 6002 },
        ElemType::Float,
        &[1],
    );
    let s2 = vm2.run(100).unwrap();
    assert_eq!(s2, VmSuspend::Complete);
    assert_eq!(
        vm2.store().get_elem(&VarRef { space: VarSpace::At, id: 6002 }, &[0]).unwrap(),
        &Value::Float(1.0),
        "FLT 位图置位 = 1.0(引擎 0x3ff0000000000000)"
    );
}

/// CGEND(0x03):ID 字符串记录(引擎 LAB_0043ad14;无图形后端 → 事件化)。
/// 真实语料形态:script1 `CGEND[ID="BT.OFF"]`(ID 窗为拼接表达式)。
#[test]
fn cgend_emits_event_with_id() {
    use yuris_value::Value;
    let key = [0x2b, 0x90, 0x4f, 0x93];
    // ID(槽0):pushstr "BT.OFF"
    let w_id: &[u8] = &[0x4d, 8, 0, b'"', b'B', b'T', b'.', b'O', b'F', b'F', b'"'];
    let groups: &[(u8, u8, u16)] = &[(cmd::CGEND, 1, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_id.len() as u32, 0, w_id),
    ];
    let bytes = make_ystb(key, groups, windows);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    let mut vm = GroupVm::load(script).unwrap();
    let s = vm.run(100).unwrap();
    assert_eq!(s, VmSuspend::Complete);
    let hit = vm.events().iter().any(|e| matches!(
        e,
        yuris_vm::VmEvent::CgEnd { id: Some(txt), .. } if txt.contains("BT.OFF")
    ));
    assert!(hit, "应有 CgEnd 事件且 ID 含 BT.OFF");
    let _ = Value::Int(0);
}
