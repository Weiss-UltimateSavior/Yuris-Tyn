//! P5.2 LOAD/YSSD 装载测试(成果 60)。
//!
//! 引擎语义(反编译 `p5_sysvar/00444648_FUN_00444648.c` +
//! `0044564d_FUN_0044564d.c`):
//! - 槽 0 = FILE(无扩展名 → 补 `.sd`)、槽 2 = DNO(1 基)、
//!   槽 3 = 写回目标(延迟引用);
//! - YSSD 块:类型一致(0x18ee8)+ strict 维数精确匹配(0x1a63a);
//! - INT/FLT = 8B/元素;STR = 逐元素 {u32 长度 + 字节}。
//!
//! 真实样本断言:vm_trace 全量对拍零分歧(95397/95397 组,
//! PROGRESS 成果 60);此处为合成链路覆盖。

mod common;

use common::{make_ystb, SAMPLE_KEY};
use yuris_format::ystb::YstbFile;
use yuris_value::{ElemType, Value, VarRef, VarSpace};
use yuris_vm::{GroupVm, VmEvent, VmSuspend, cmd};

fn var(space: VarSpace, id: u16) -> VarRef {
    VarRef { space, id }
}

/// SNP 字面量编码(变体:长度 = (tag>>2)+1;仅测试用,全字面量)。
fn snp_encode_literal(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // varint 未压长度
    let mut n = payload.len() as u64;
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
    // 分块短字面量(长度 1..=60 → tag = (len-1)<<2,tag>>2 ≤ 59 不入长形式)
    let mut rest = payload;
    while !rest.is_empty() {
        let take = rest.len().min(60);
        out.push(((take - 1) << 2) as u8);
        out.extend_from_slice(&rest[..take]);
        rest = &rest[take..];
    }
    out
}

/// 临时目录守卫(Drop 清理)。
struct TempDirGuard(std::path::PathBuf);

impl TempDirGuard {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_create(name: &str) -> TempDirGuard {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    TempDirGuard(p)
}

/// 构造合成 YSSD 文件(单块)。
fn make_yssd(dno: u32, btype: u8, strict: u8, var_id: u16, payload: &[u8]) -> Vec<u8> {
    let comp = snp_encode_literal(payload);
    let mut out = Vec::new();
    out.extend_from_slice(b"YSSD");
    out.extend_from_slice(&0x1E0u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // 块数
    out.extend_from_slice(&0x400u32.to_le_bytes()); // 偏移表容量
    out.extend_from_slice(&[0u8; 0x1000]); // 偏移表(全 0)
    let block_off = out.len();
    // 回填表项[DNO-1]
    let idx = (dno - 1) as usize;
    out[0x10 + idx * 4..0x10 + idx * 4 + 4].copy_from_slice(&(block_off as u32).to_le_bytes());
    // 块
    out.extend_from_slice(&dno.to_le_bytes());
    out.push(btype);
    out.push(strict);
    out.extend_from_slice(&var_id.to_le_bytes());
    out.extend_from_slice(&(comp.len() as u32).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&comp);
    out
}

/// INT 一维数组载荷。
fn int_payload(dims: &[u32], vals: &[i64]) -> Vec<u8> {
    let mut raw = Vec::new();
    raw.extend_from_slice(&1u32.to_le_bytes());
    raw.extend_from_slice(&(dims.len() as u32).to_le_bytes());
    for d in dims {
        raw.extend_from_slice(&d.to_le_bytes());
    }
    let body: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
    raw.extend_from_slice(&(body.len() as u32).to_le_bytes());
    raw.extend_from_slice(&body);
    raw
}

/// STR 一维数组载荷({u32 长度 + 字节}/元素)。
fn str_payload(dims: &[u32], vals: &[&[u8]]) -> Vec<u8> {
    let mut raw = Vec::new();
    raw.extend_from_slice(&3u32.to_le_bytes());
    raw.extend_from_slice(&(dims.len() as u32).to_le_bytes());
    for d in dims {
        raw.extend_from_slice(&d.to_le_bytes());
    }
    let mut body = Vec::new();
    for v in vals {
        body.extend_from_slice(&(v.len() as u32).to_le_bytes());
        body.extend_from_slice(v);
    }
    raw.extend_from_slice(&(body.len() as u32).to_le_bytes());
    raw.extend_from_slice(&body);
    raw
}

/// 建临时游戏根(pac/ + save/test.sd)并返回路径。
fn make_game_dir(payload: &[u8]) -> TempDirGuard {
    let dir = temp_create("yuris-load-test");
    std::fs::create_dir_all(dir.path().join("pac")).unwrap();
    std::fs::create_dir_all(dir.path().join("save")).unwrap();
    std::fs::write(dir.path().join("save/test.sd"), make_yssd(1, 0, 1, 9001, payload)).unwrap();
    dir
}

/// 合成 LOAD 脚本:FILE="test"(→ test.sd)+ DNO=1 + 目标 @9001。
fn load_script(target: &VarRef) -> GroupVm {
    let w_file: &[u8] = &[0x4d, 6, 0, b'"', b't', b'e', b's', b't', b'"'];
    let w_dno: &[u8] = &[0x42, 1, 0, 1]; // push 1
    let tid = target.id.to_le_bytes();
    let prefix = target.space.prefix();
    let w_target: &[u8] = &[0x76, 3, 0, prefix, tid[0], tid[1]];
    let groups: &[(u8, u8, u16)] = &[(cmd::LOAD, 3, 0), (cmd::RETURN, 0, 0)];
    let windows = &[
        ([0x00, 0x00, 0x00, 0x00], w_file.len() as u32, 0, w_file), // 槽 0 FILE
        ([0x02, 0x00, 0x00, 0x00], w_dno.len() as u32, 0, w_dno), // 槽 2 DNO
        ([0x03, 0x00, 0x00, 0x00], w_target.len() as u32, 0, w_target), // 槽 3 目标
    ];
    let bytes = make_ystb(SAMPLE_KEY, groups, windows);
    let script = YstbFile::from_bytes(&bytes, SAMPLE_KEY).unwrap();
    GroupVm::load(script).unwrap()
}

/// INT 数组装载:save/test.sd 块 1(INT[3] = 1,2,3)→ @9001。
#[test]
fn load_yssd_int_array() {
    let dir = make_game_dir(&int_payload(&[3], &[1, 2, 3]));
    let target = var(VarSpace::At, 9001);
    let mut vm = load_script(&target);
    vm.store_mut().declare_array(&target, ElemType::Int, &[3]);
    let idx = yuris_vm::host::PacFileIndex::scan_game_dir(dir.path(), 0xC9).unwrap();
    vm.set_file_probe(std::sync::Arc::new(idx));
    let s = vm.run(100).unwrap();
    assert!(matches!(s, VmSuspend::Complete), "无挂起: {s:?}");
    for (i, expect) in [1i64, 2, 3].iter().enumerate() {
        assert_eq!(
            vm.store().get_elem(&target, &[i as i64]).unwrap(),
            &Value::Int(*expect)
        );
    }
    // 事件仍发(FILE + 摘要)
    assert!(vm.events().iter().any(|e| matches!(
        e,
        VmEvent::Load { file: Some(f), .. } if f == b"test"
    )));
}

/// STR 数组装载:$9002 STR[2]。
#[test]
fn load_yssd_str_array() {
    let dir = make_game_dir(&str_payload(&[2], &[b"ab", b"cdef"]));
    let target = var(VarSpace::Dollar, 9002);
    let mut vm = load_script(&target);
    vm.store_mut().declare_array(&target, ElemType::Str, &[2]);
    let idx = yuris_vm::host::PacFileIndex::scan_game_dir(dir.path(), 0xC9).unwrap();
    vm.set_file_probe(std::sync::Arc::new(idx));
    vm.run(100).unwrap();
    assert_eq!(
        vm.store().get_elem(&target, &[0]).unwrap(),
        &Value::Str(b"ab".to_vec())
    );
    assert_eq!(
        vm.store().get_elem(&target, &[1]).unwrap(),
        &Value::Str(b"cdef".to_vec())
    );
}

/// 标量装载:@9003(INT 标量)。
#[test]
fn load_yssd_scalar() {
    let dir = make_game_dir(&int_payload(&[], &[42]));
    let target = var(VarSpace::At, 9003);
    let mut vm = load_script(&target);
    vm.store_mut().set(&target, Value::Int(0));
    let idx = yuris_vm::host::PacFileIndex::scan_game_dir(dir.path(), 0xC9).unwrap();
    vm.set_file_probe(std::sync::Arc::new(idx));
    vm.run(100).unwrap();
    assert_eq!(vm.store().get(&target).unwrap(), &Value::Int(42));
}

/// strict 维数不匹配 → 引擎 0x1a63a 错误路径。
#[test]
fn load_yssd_strict_dim_mismatch_errors() {
    let dir = make_game_dir(&int_payload(&[3], &[1, 2, 3]));
    let target = var(VarSpace::At, 9001);
    let mut vm = load_script(&target);
    // 声明 4 元素,载荷是 3 → strict 不符
    vm.store_mut().declare_array(&target, ElemType::Int, &[4]);
    let idx = yuris_vm::host::PacFileIndex::scan_game_dir(dir.path(), 0xC9).unwrap();
    vm.set_file_probe(std::sync::Arc::new(idx));
    let err = vm.run(100).unwrap_err();
    assert!(err.to_string().contains("0x1a63a"), "{err}");
}

/// 类型不匹配(INT 载荷 → STR 目标)→ 引擎 0x18ee8。
#[test]
fn load_yssd_type_mismatch_errors() {
    let dir = make_game_dir(&int_payload(&[3], &[1, 2, 3]));
    let target = var(VarSpace::Dollar, 9004);
    let mut vm = load_script(&target);
    vm.store_mut().declare_array(&target, ElemType::Str, &[3]);
    let idx = yuris_vm::host::PacFileIndex::scan_game_dir(dir.path(), 0xC9).unwrap();
    vm.set_file_probe(std::sync::Arc::new(idx));
    let err = vm.run(100).unwrap_err();
    assert!(err.to_string().contains("0x18ee8"), "{err}");
}

/// 文件不存在 → 引擎 0x18ed4 错误框语义。
#[test]
fn load_yssd_missing_file_errors() {
    let dir = temp_create("yuris-load-test2");
    std::fs::create_dir_all(dir.path().join("pac")).unwrap();
    let target = var(VarSpace::At, 9001);
    let mut vm = load_script(&target);
    vm.store_mut().declare_array(&target, ElemType::Int, &[3]);
    let idx = yuris_vm::host::PacFileIndex::scan_game_dir(dir.path(), 0xC9).unwrap();
    vm.set_file_probe(std::sync::Arc::new(idx));
    let err = vm.run(100).unwrap_err();
    assert!(err.to_string().contains("0x18ed4"), "{err}");
}
