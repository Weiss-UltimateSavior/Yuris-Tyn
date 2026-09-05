//! 集成测试:对**真实样本**解码(期望值来自 PROGRESS.md 与 probe_opcode_scan2.py)。
//!
//! 样本缺失时跳过;可用 `YURIS_SAMPLE_BNYPF` 指向样本。

use yuris_format::{ypf::YpfArchive, ystb::{segment_window, YstbFile}};
use yuris_script::{decode_window, Insn, VarSpace};

fn sample_path() -> Option<std::path::PathBuf> {
    let p = std::env::var("YURIS_SAMPLE_BNYPF")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(
                "/Users/weiss/Desktop/yuris/AnimalTrailGirlishSquare 2/pac/bn.ypf",
            )
        });
    p.exists().then_some(p)
}

fn open_sample() -> Option<YpfArchive> {
    let p = sample_path()?;
    let data = std::fs::read(&p).unwrap();
    Some(YpfArchive::from_bytes(data, 0xC9).unwrap())
}

/// 顺序型脚本:402 个窗口(含 tag0)全部解码,**零 Unknown**
/// (yst00000 实测 10 种 opcode 全部为已证实语义)。
#[test]
fn yst00000_decodes_with_zero_unknown() {
    let Some(ypf) = open_sample() else {
        eprintln!("sample not found, skip");
        return;
    };
    let blob = ypf.read("$ysbin\\yst00000.ybn").unwrap();
    let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();

    let mut total = 0usize;
    let mut unknown = 0usize;
    let mut pushes = 0usize;
    for s in f.slots() {
        let w = f.slot_content(s).unwrap();
        let instrs = decode_window(w)
            .unwrap_or_else(|e| panic!("slot off={} len={}: {e}", s.offset, s.len));
        for (seg_i, ins) in instrs.iter().enumerate() {
            // 与 segment_window 的分帧一致:逐条消费字节数相同
            let _ = seg_i;
            total += 1;
            match &ins.kind {
                Insn::Unknown { raw_op, .. } => {
                    unknown += 1;
                    panic!("意外 Unknown op={raw_op:#04x} @ slot off={}", s.offset);
                }
                Insn::PushInt(_) | Insn::PushFloat(_) | Insn::PushStr(_) | Insn::PushVar(_) => {
                    pushes += 1
                }
                _ => {}
            }
        }
        // 分帧一致性:两种切分的指令数相同
        assert_eq!(instrs.len(), segment_window(w).unwrap().len());
    }
    assert_eq!(total, 868, "yst00000 实测指令总数(probe 实测)");
    assert_eq!(unknown, 0);
    assert!(pushes > 0);

    // 逐 opcode 分布断言(probe_opcode_scan2 同源实测值)
    let mut hist = std::collections::BTreeMap::new();
    for s in f.slots() {
        for ins in decode_window(f.slot_content(s).unwrap()).unwrap() {
            let name = match ins.kind {
                Insn::PushInt(_) => "int",
                Insn::PushFloat(_) => "float",
                Insn::PushStr(_) => "str",
                Insn::PushVar(_) => "var",
                Insn::PushVarRef(_) => "varref",
                Insn::PushVarIndexed(_) => "varidx",
                Insn::ArrayLoad { .. } => "aload",
                Insn::Binary(yuris_script::BinOp::Add) => "add",
                Insn::GroupSep => "groupsep",
                Insn::Unknown { .. } => "unknown",
                _ => "other",
            };
            *hist.entry(name).or_insert(0usize) += 1;
        }
    }
    assert_eq!(hist.get("int"), Some(&402));  // 0x42×166 + 0x57×118 + 0x49×1 + 0x4c×117
    assert_eq!(hist.get("var"), Some(&78));   // 0x48
    assert_eq!(hist.get("varref"), Some(&123)); // 0x56
    assert_eq!(hist.get("aload"), Some(&123)); // 0x29
    assert_eq!(hist.get("str"), Some(&32));   // 0x4d
    assert_eq!(hist.get("add"), Some(&99));   // 0x2b
    assert_eq!(hist.get("groupsep"), Some(&11)); // 0x2c
    assert_eq!(hist.get("unknown").copied().unwrap_or(0), 0);
}

/// 首窗口(22 字节)的语义断言:PushVar($,0x04ca) PushInt(0x190) PushInt(1) Add ArrayLoad。
#[test]
fn yst00000_first_window_semantics() {
    let Some(ypf) = open_sample() else {
        eprintln!("sample not found, skip");
        return;
    };
    let blob = ypf.read("$ysbin\\yst00000.ybn").unwrap();
    let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();
    let s0 = &f.slots()[0];
    let instrs = decode_window(f.slot_content(s0).unwrap()).unwrap();

    assert_eq!(instrs.len(), 5);
    assert_eq!(
        instrs[0].kind,
        Insn::PushVarRef(yuris_script::VarRef {
            space: VarSpace::Dollar,
            id: 0x04ca
        })
    );
    assert_eq!(instrs[1].kind, Insn::PushInt(0x0190));
    assert_eq!(instrs[2].kind, Insn::PushInt(1));
    assert_eq!(instrs[3].kind, Insn::Binary(yuris_script::BinOp::Add));
    assert_eq!(instrs[4].kind, Insn::ArrayLoad { raw: 0 });
}

/// 池式脚本:非 tag0 窗口全部解码成功;Unknown 只允许出现在
/// 已记录的未证实 opcode(0x00/0x01/0x08)中 —— 不扩大的诚实边界。
#[test]
fn yst00034_pool_file_decodes() {
    let Some(ypf) = open_sample() else {
        eprintln!("sample not found, skip");
        return;
    };
    let blob = ypf.read("$ysbin\\yst00034.ybn").unwrap();
    let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();

    let mut decoded = 0usize;
    for s in f.slots() {
        let Ok(w) = f.slot_content(s) else {
            continue; // 末槽位越界特殊记录
        };
        if s.tag == 0x0000_0000 {
            continue; // tag0 = 窗口注记,不保证独立闭合
        }
        let instrs = decode_window(w)
            .unwrap_or_else(|e| panic!("non-text slot off={}: {e}", s.offset));
        for ins in instrs {
            decoded += 1;
            if let Insn::Unknown { raw_op, .. } = ins.kind {
                assert!(
                    matches!(raw_op, 0x00 | 0x01 | 0x08),
                    "未记录的 Unknown opcode {raw_op:#04x} —— 需更新文档"
                );
            }
        }
    }
    assert!(decoded > 100, "解码指令数应可观,实际 {decoded}");
}

/// 全游戏统一密钥下的字符串载荷:0x4d 载荷以引号包裹(SJIS)。
#[test]
fn yst00000_m_strings_are_quoted() {
    let Some(ypf) = open_sample() else {
        eprintln!("sample not found, skip");
        return;
    };
    let blob = ypf.read("$ysbin\\yst00000.ybn").unwrap();
    let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();
    let mut quoted = 0usize;
    let mut total_m = 0usize;
    for s in f.slots() {
        let instrs = decode_window(f.slot_content(s).unwrap()).unwrap();
        for ins in instrs {
            if let Insn::PushStr(p) = &ins.kind {
                total_m += 1;
                if p.starts_with(b"\"") && p.ends_with(b"\"") {
                    quoted += 1;
                }
            }
        }
    }
    assert!(total_m > 0, "yst00000 应有 0x4d 字符串");
    assert_eq!(quoted, total_m, "全部 M-串载荷应以引号包裹(编译器类型 4)");
}

/// 求值器烟雾测试:yst00000 全部窗口执行一遍。
/// 常量窗口应求值成功;含变量引用/数组/未证实 op 的窗口按预期报错
/// (Unimplemented / UnresolvedOpcode / 未定义变量)。**零 panic**。
#[test]
fn yst00000_evaluator_smoke() {
    use yuris_script::{Evaluator, VarSpace};
    use yuris_value::VariableStore;

    let Some(ypf) = open_sample() else {
        eprintln!("sample not found, skip");
        return;
    };
    let blob = ypf.read("$ysbin\\yst00000.ybn").unwrap();
    let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();

    let mut store = VariableStore::new();
    let mut ok = 0usize;
    let mut err = 0usize;
    for s in f.slots() {
        let instrs = decode_window(f.slot_content(s).unwrap()).unwrap();
        match Evaluator::new(&mut store).eval_instructions(&instrs) {
            Ok(_) => ok += 1,
            Err(e) => {
                err += 1;
                let msg = e.to_string();
                assert!(
                    msg.contains("unimplemented") || msg.contains("unresolved")
                        || msg.contains("undefined variable") || msg.contains("undefined array")
                        || msg.contains("underflow")
                        || msg.contains("Unknown"),
                    "非预期错误类别: {msg}"
                );
            }
        }
    }
    assert_eq!(ok + err, 402);
    assert!(ok > 0, "至少部分常量窗口应可求值");
}
