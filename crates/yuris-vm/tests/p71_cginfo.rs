//! P7.1 CG 状态注册表测试(成果 62;P1 收尾定稿)。
//!
//! 引擎语义(反编译 0x423864 CMD_CG + 0x43b084 CGINFO + watch oracle 实证):
//! - CG 命令:FILE 槽(46)非空字符串 → 创建(注册);无 FILE / FILE="" →
//!   静默不创建(处理器 return 0);
//! - CGINFO:槽13/14 = **装载图像真实宽/高**(引擎 watch oracle:occ1 =
//!   1.0 = `cgsys\dummy.png` 1×1 占位图、occ2 = 1350.0 = 消息窗纹理
//!   1350×200;两图经封包扫描确认);COLOR(槽24)= 0x808080(occ1 实证);
//!   未注册名 → 全部查询写 0(「不存在」路径);
//!   本测试无 VFS 探针 → 图像头不可解析 → 槽13/14 = 0(诚实降级);
//! - CGEND:按名移除。
//!
//! 真实样本:s9 g1072 CG(BT.OFF, FILE=$1227[@1704]) → g1089-91 CGINFO;
//! occ1 引擎真值 (1,1,8421504) = dummy.png(1×1)+ COLOR 缺省 —— 旧
//! 「静态默认 SX=1/SY=1」模型系巧合命中,已作废(见 CgState 文档)。

mod common;

use common::{make_ystb, SAMPLE_KEY};
use yuris_format::ystb::YstbFile;
use yuris_value::{Value, VarRef, VarSpace};
use yuris_vm::{GroupVm, VmSuspend, cmd};

fn var(id: u16) -> VarRef {
    VarRef {
        space: VarSpace::At,
        id,
    }
}

/// 字面量字符串窗口(带引号定界,与语料 0x4d 形态一致)。
fn w_str(s: &str) -> Vec<u8> {
    let mut v = vec![0x4d];
    let inner: Vec<u8> = format!("\"{s}\"").into_bytes();
    v.extend_from_slice(&(inner.len() as u16).to_le_bytes());
    v.extend_from_slice(&inner);
    v
}

fn w_push1() -> Vec<u8> {
    vec![0x42, 1, 0, 1]
}

fn w_var(id: u16) -> Vec<u8> {
    let mut v = vec![0x48, 3, 0, 0x40];
    v.extend_from_slice(&id.to_le_bytes());
    v
}

struct W {
    slot: u8,
    bytes: Vec<u8>,
}

fn build(groups: &[(u8, u8, u16)], wins: &[W]) -> GroupVm {
    let key = SAMPLE_KEY;
    let windows: Vec<([u8; 4], u32, u32, Vec<u8>)> = wins
        .iter()
        .map(|w| ([w.slot, 0, 0, 0], w.bytes.len() as u32, 0, w.bytes.clone()))
        .collect();
    let refs: Vec<([u8; 4], u32, u32, &[u8])> = windows
        .iter()
        .map(|(t, l, o, b)| (*t, *l, *o, b.as_slice()))
        .collect();
    let bytes = make_ystb(key, groups, &refs);
    let script = YstbFile::from_bytes(&bytes, key).unwrap();
    GroupVm::load(script).unwrap()
}

/// CG(FILE=非空) 创建 → CGINFO COLOR = 0x808080;槽13/14 无探针 = 0
/// (图像头不可解析,诚实降级;引擎真值 = 图像尺寸)。
#[test]
fn cginfo_sx_sy_color_defaults() {
    let vm = build(
        &[
            (cmd::CG, 3, 0),
            (cmd::CGINFO, 3, 0),
            (cmd::CGINFO, 3, 0),
            (cmd::CGINFO, 5, 0),
            (cmd::RETURN, 0, 0),
        ],
        &[
            W { slot: 0, bytes: w_str("btnA") },   // CG ID
            W { slot: 46, bytes: w_str("btn.png") }, // CG FILE(非空)
            W { slot: 4, bytes: vec![0x42, 1, 0, 10] }, // X=10
            W { slot: 0, bytes: w_str("btnA") },   // CGINFO ID
            W { slot: 13, bytes: w_push1() },      // SX 查询
            W { slot: 33, bytes: w_var(9000) },    // LET → @9000
            W { slot: 0, bytes: w_str("btnA") },
            W { slot: 14, bytes: w_push1() },      // SY 查询
            W { slot: 33, bytes: w_var(9001) },
            W { slot: 0, bytes: w_str("btnA") },
            W { slot: 24, bytes: w_push1() },      // COLOR 查询
            W { slot: 34, bytes: vec![0x42, 1, 0, 0] }, // SET=0
            W { slot: 35, bytes: vec![0x42, 1, 0, 0] }, // SET2=0
            W { slot: 33, bytes: w_var(9002) },
        ],
    );
    let mut vm = vm;
    let s = vm.run(100).unwrap();
    assert!(matches!(s, VmSuspend::Complete), "{s:?}");
    assert_eq!(
        vm.store().get(&var(9000)).unwrap(),
        &Value::Int(0),
        "槽13 = 图像宽;无 VFS 探针不可解析 → 0"
    );
    assert_eq!(
        vm.store().get(&var(9001)).unwrap(),
        &Value::Int(0),
        "槽14 = 图像高;无 VFS 探针不可解析 → 0"
    );
    assert_eq!(
        vm.store().get(&var(9002)).unwrap(),
        &Value::Int(8421504),
        "COLOR 默认 0x808080"
    );
}

/// CG 无 FILE / FILE="" → 不创建 → CGINFO = 0(BT.OVER 族行为)。
#[test]
fn cginfo_no_file_not_created() {
    // CG(btnB, X) —— 无 FILE 槽
    let mut vm = build(
        &[(cmd::CG, 2, 0), (cmd::CGINFO, 3, 0), (cmd::RETURN, 0, 0)],
        &[
            W { slot: 0, bytes: w_str("btnB") },
            W { slot: 4, bytes: vec![0x42, 1, 0, 10] },
            W { slot: 0, bytes: w_str("btnB") },
            W { slot: 13, bytes: w_push1() },
            W { slot: 33, bytes: w_var(9010) },
        ],
    );
    let s = vm.run(100).unwrap();
    assert!(matches!(s, VmSuspend::Complete), "{s:?}");
    assert_eq!(
        vm.store().get(&var(9010)).unwrap(),
        &Value::Int(0),
        "无 FILE 未注册 → SX=0"
    );

    // CG(btnC, FILE="") —— 空 FILE(s9 g1114 形态:4d 02 00 22 22)
    let mut vm = build(
        &[(cmd::CG, 2, 0), (cmd::CGINFO, 3, 0), (cmd::RETURN, 0, 0)],
        &[
            W { slot: 0, bytes: w_str("btnC") },
            W { slot: 46, bytes: w_str("") },
            W { slot: 0, bytes: w_str("btnC") },
            W { slot: 13, bytes: w_push1() },
            W { slot: 33, bytes: w_var(9011) },
        ],
    );
    let s = vm.run(100).unwrap();
    assert!(matches!(s, VmSuspend::Complete), "{s:?}");
    assert_eq!(
        vm.store().get(&var(9011)).unwrap(),
        &Value::Int(0),
        "FILE=\"\" 未注册 → SX=0"
    );
}

/// CGEND 按名移除 → 后续 CGINFO = 0。
#[test]
fn cgend_unregisters() {
    let mut vm = build(
        &[
            (cmd::CG, 2, 0),
            (cmd::CGEND, 1, 0),
            (cmd::CGINFO, 3, 0),
            (cmd::RETURN, 0, 0),
        ],
        &[
            W { slot: 0, bytes: w_str("btnD") },
            W { slot: 46, bytes: w_str("d.png") },
            W { slot: 0, bytes: w_str("btnD") },
            W { slot: 0, bytes: w_str("btnD") },
            W { slot: 13, bytes: w_push1() },
            W { slot: 33, bytes: w_var(9020) },
        ],
    );
    let s = vm.run(100).unwrap();
    assert!(matches!(s, VmSuspend::Complete), "{s:?}");
    assert_eq!(
        vm.store().get(&var(9020)).unwrap(),
        &Value::Int(0),
        "CGEND 后未注册 → SX=0"
    );
}

/// 已注册 CG 的 X/Y 写入与查询(模型 Likely,引擎槽位 +0x70/+0x78)。
#[test]
fn cginfo_x_y_roundtrip() {
    let mut vm = build(
        &[
            (cmd::CG, 4, 0),
            (cmd::CGINFO, 3, 0),
            (cmd::CGINFO, 3, 0),
            (cmd::RETURN, 0, 0),
        ],
        &[
            W { slot: 0, bytes: w_str("btnE") },
            W { slot: 46, bytes: w_str("e.png") },
            W { slot: 4, bytes: vec![0x42, 1, 0, 100] }, // X=100
            W { slot: 5, bytes: vec![0x42, 1, 0, 90] }, // Y=90
            W { slot: 0, bytes: w_str("btnE") },
            W { slot: 5, bytes: w_push1() }, // X 查询(CGINFO 槽5)
            W { slot: 33, bytes: w_var(9030) },
            W { slot: 0, bytes: w_str("btnE") },
            W { slot: 6, bytes: w_push1() }, // Y 查询(CGINFO 槽6)
            W { slot: 33, bytes: w_var(9031) },
        ],
    );
    let s = vm.run(100).unwrap();
    assert!(matches!(s, VmSuspend::Complete), "{s:?}");
    assert_eq!(vm.store().get(&var(9030)).unwrap(), &Value::Int(100));
    assert_eq!(vm.store().get(&var(9031)).unwrap(), &Value::Int(90));
}
