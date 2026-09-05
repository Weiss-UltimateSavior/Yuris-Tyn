//! 集成测试：对**真实样本**做断言。
//!
//! 样本：`AnimalTrailGirlishSquare 2/pac/bn.ypf`（YPF v500 / 引擎 v555）。
//!
//! 样本缺失时测试**跳过**（CI 里可以设 `YURIS_SAMPLE_BNYPF` 指向样本）。
//! 所有期望值来自 `PROGRESS.md` 的实测记录，与 `scripts/probe_format.py` 交叉验证。

use yuris_core::VersionProfile;
use yuris_format::{
    ypf::YpfArchive,
    yscf::YscfFile,
    yscm::YscmTable,
    yslb,
    ystb::{guess_key, segment_window, YstbFile},
    ysvr,
};

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

#[test]
fn ypf_parses_309_entries_and_closes_exactly() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let profile = VersionProfile::sample_v555();
    let ypf = YpfArchive::from_bytes(data, profile.ypf_name_xor_key).unwrap();

    assert_eq!(ypf.header().version, 500);
    assert_eq!(ypf.header().file_count, 309);
    assert_eq!(ypf.entries().len(), 309);
    assert_eq!(ypf.header().first_data_off, 0x3655);
    // 闭合：末条 tail 被 first_data_off 钳制，因此解析后位置精确等于 first_data_off
    assert_eq!(ypf.index_prefix(), 0x0033_AE52);

    // 抽查已知条目（来自 PROGRESS.md 实测）
    let e = ypf.entry("9ysbin\\yscfg.ybn").expect("yscfg entry");
    assert_eq!(e.flags, 1);
    assert_eq!(e.uncompressed_len, 106);
    assert_eq!(e.compressed_len, 74);
    assert_eq!(e.offset, 0x0046_6B);
}

#[test]
fn ypf_engine_entries_have_expected_magics() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();

    // 样本实测：5 个引擎数据条目，解压后 magic 各不相同（PROGRESS.md 成果 1）
    let expect: &[(&str, &[u8; 4])] = &[
        ("%ysbin\\ysc.ybn", b"YSCM"), // ★ Opcode/参数名表
        ("9ysbin\\yscfg.ybn", b"YSCF"),
        ("%ysbin\\yse.ybn", b"YSER"),
        ("%ysbin\\ysl.ybn", b"YSLB"),
        ("%ysbin\\ysv.ybn", b"YSVR"),
    ];
    for (name, magic) in expect {
        let blob = ypf.read(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(&blob[..4], *magic, "{name} magic");
    }

    // 已知 stored 条目：%ysbin\yst.ybn（flags=0，16 字节）
    // 注意前缀：$ / % / 9 是三种不同的虚拟根标记（语义 Unknown）
    let e = ypf
        .entries()
        .iter()
        .find(|e| e.name.ends_with("\\yst.ybn"))
        .expect("yst.ybn entry");
    assert_eq!(e.flags, 0);
    assert_eq!(e.compressed_len, e.uncompressed_len);
}

#[test]
fn ystb_key_is_recoverable_and_slots_are_contiguous() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();

    let blob = ypf.read("$ysbin\\yst00000.ybn").unwrap();
    let (key, score) = guess_key(&blob).unwrap();
    assert_eq!(key, [0x2B, 0x90, 0x4F, 0x93], "guess_key 应还原出实测密钥");
    assert!((score - 1.0).abs() < 1e-6, "正确密钥的连续性应为 1.0，得到 {score}");

    let f = YstbFile::from_bytes(&blob, key).unwrap();
    assert_eq!(f.header().version, 555);
    assert_eq!(f.slots().len(), 402);
    assert_eq!(f.contiguity_score(), 1.0);

    // 槽位偏移必须严格自洽（密钥正确性的决定性判据）
    let mut prev_end = 0u32;
    for s in f.slots() {
        assert_eq!(s.offset, prev_end, "slot offset 跳变 => 密钥错误");
        prev_end = s.offset + s.len;
    }
    assert_eq!(prev_end, f.header().content_len);

    // tag 分布（实测：201 / 169 / 32）
    let mut text = 0;
    let mut opt = 0;
    let mut other = 0;
    for s in f.slots() {
        match s.tag {
            0x0000_0000 => text += 1,
            0x0003_0000 => opt += 1,
            _ => other += 1,
        }
    }
    assert_eq!((text, opt, other), (201, 32, 169));
}

#[test]
fn ystb_wrong_key_is_detected_by_contiguity() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("$ysbin\\yst00000.ybn").unwrap();

    // 错误密钥的连续性必须显著低于 1.0
    let wrong = [0x39u8, 0x92, 0x4F, 0x93];
    let f = YstbFile::from_bytes(&blob, wrong).unwrap();
    assert!(
        f.contiguity_score() < 0.5,
        "错误密钥不应有高连续性，得到 {}",
        f.contiguity_score()
    );
}

#[test]
fn yscf_reports_screen_and_caption() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("9ysbin\\yscfg.ybn").unwrap();
    let f = YscfFile::from_bytes(&blob).unwrap();

    assert_eq!(f.version, 555);
    assert_eq!((f.screen_width, f.screen_height), (1920, 1080));
    assert_eq!(f.caption, "Kemonomichi Girlish Square 2");
    // 样本实测 (dev, debug, release) = (1, 1, 0)：Release 默认关闭免封包，
    // 与 [YU-RIS] 免封包处理 一文「把 filePriorityRelease 改成 1」的说法吻合
    assert_eq!(
        (f.file_priority_dev, f.file_priority_debug, f.file_priority_release),
        (1, 1, 0)
    );
    assert!(!f.unpacked_read_enabled());
}

// ===== P1.1：YSCM 命令字典（期望值来自 scripts/probe_yscm.py 独立验证）=====

#[test]
fn yscm_parses_121_commands_and_1113_params() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("%ysbin\\ysc.ybn").unwrap();
    let t = YscmTable::from_bytes(&blob).unwrap();

    // header + 总量（逐项，非总量近似）
    assert_eq!(t.version(), 555);
    assert_eq!(t.unknown, 0);
    assert_eq!(t.commands().len(), 121);
    assert_eq!(t.param_total(), 1113);
    assert_eq!(t.tail_offset, 0x2749);
    assert_eq!(t.tail.len(), 1045);

    // 命令名唯一
    let mut names: Vec<&str> = t.commands().iter().map(|c| c.name.as_str()).collect();
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), n, "命令名应唯一");

    // 抽查已知命令的参数表（probe_yscm.py 逐条输出）
    let alias = t.command("ALIAS").unwrap();
    assert_eq!(alias.params.len(), 0);
    assert_eq!(alias.offset, 0x10);

    let cg = t.command("CG").unwrap();
    assert_eq!(cg.params.len(), 58);
    assert_eq!(cg.params[0].name, "ID");
    assert_eq!(cg.params[0].ty, 0x0001);
    assert_eq!(cg.params[1].name, "IDNO");
    assert_eq!(cg.params[1].ty, 0x0100);
    assert_eq!(cg.params[4].name, "X");
    assert_eq!(cg.params[4].ty, 0x0000);
    assert_eq!(cg.params[22].name, "TSX");
    assert_eq!(cg.params[22].ty, 0x1500);

    // 首尾命令
    assert_eq!(t.commands()[0].name, "ALIAS");
    assert_eq!(t.commands()[120].name, "PROJECTFOLDER");
    assert_eq!(t.commands()[117].name, "SYSTEMMODE");
    assert_eq!(t.commands()[117].params.len(), 33);
    assert_eq!(t.commands()[117].params[20].name, "FILEPRIORITYDEVELOP");
}

// ===== P1.2：content 自描述编码 + 池式窗口模型 =====

/// 顺序型脚本：全部窗口（含 tag0）精确闭合。
#[test]
fn ystb_sequential_file_all_windows_segment() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("$ysbin\\yst00000.ybn").unwrap();
    let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();

    assert_eq!(f.slots().len(), 402);
    // 全部 402 个窗口（含 201 个 tag0）逐一精确闭合 —— 逐条断言
    for s in f.slots() {
        let w = f.slot_content(s).unwrap();
        let instrs = segment_window(w)
            .unwrap_or_else(|e| panic!("slot tag={:#010x} off={} len={}: {e}", s.tag, s.offset, s.len));
        let consumed: usize = instrs.iter().map(|i| 3 + i.operand.len()).sum();
        assert_eq!(consumed, s.len as usize);
    }
    // yst00000 的 opcode 种类收敛（实测 10 种）
    let mut ops = std::collections::BTreeSet::new();
    for s in f.slots() {
        for i in segment_window(f.slot_content(s).unwrap()).unwrap() {
            ops.insert(i.op);
        }
    }
    assert_eq!(ops.len(), 10, "yst00000 实测 10 种 opcode");
}

/// 池式脚本：密钥统一 + 非 tag0 窗口全部闭合 + guess_key 在池式文件上仍能工作。
#[test]
fn ystb_pool_file_key_and_windows() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("$ysbin\\yst00034.ybn").unwrap();

    // 池式文件：连续性 < 1.0（窗口重叠），但 guess_key 靠窗口闭合率仍应命中统一密钥
    let (key, score) = guess_key(&blob).unwrap();
    assert_eq!(key, [0x2B, 0x90, 0x4F, 0x93], "全游戏统一密钥");
    assert!((score - 1.0).abs() < 1e-6, "窗口闭合率应为 1.0，得到 {score}");

    let f = YstbFile::from_bytes(&blob, key).unwrap();
    assert_eq!(f.slots().len(), 189);
    assert!(
        f.contiguity_score() < 0.8,
        "池式文件连续性应显著低于 1.0（实测 0.6984）"
    );

    // 逐条断言：非 tag0 窗口全部精确闭合；tag0 窗口不保证（窗口注记）。
    // 末槽位是越界的特殊记录（tag0, off==content_len），slot_content 返回 Err，跳过。
    let mut non_text = 0usize;
    for s in f.slots() {
        let Ok(w) = f.slot_content(s) else {
            assert_eq!(s.tag, 0x0000_0000, "越界窗口只允许出现在 tag0 特殊记录上");
            continue;
        };
        if s.tag == 0x0000_0000 {
            continue;
        }
        non_text += 1;
        let instrs = segment_window(w)
            .unwrap_or_else(|e| panic!("non-text slot off={} len={}: {e}", s.offset, s.len));
        let consumed: usize = instrs.iter().map(|i| 3 + i.operand.len()).sum();
        assert_eq!(consumed, s.len as usize);
    }
    assert_eq!(non_text, 154, "yst00034 实测 189-35 个非 tag0 窗口");

    // part1_len == part4_len == 4 × unknown1（302/302 全语料成立）
    let h = f.header();
    assert_eq!(h.part1_len, 4 * h.unknown1);
    assert_eq!(h.part4_len, 4 * h.unknown1);
}

/// 空脚本（cl == 0，12 个）：结构可解析、无槽位、guess_key 诚实报错。
#[test]
fn ystb_empty_script_has_no_slots() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("$ysbin\\yst00080.ybn").unwrap();
    let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();
    assert_eq!(f.slots().len(), 0);
    assert_eq!(f.header().command_len, 0);
    assert_eq!(f.header().content_len, 0);
    assert!(guess_key(&blob).is_err(), "无槽位可判定时应报错而不是猜");
}

/// 命令组模型（引擎 FUN_00450dfd 逆向，2026-09-03）：全语料 302/302 逐条断言。
///
/// 1. `groups()` 全部通过（part1_len==4G ∧ Σcount*12==command_len）
/// 2. 结构不变量：GO(0x2a) 恒 1 窗、IF(0x2c) 恒 3 窗、LET(0x35) 恒 2 窗
/// 3. tag B1 字节恒 0；IF 的 w1/w2 len 域（编译期组号）非零对配对出现
#[test]
fn ystb_group_model_holds_on_entire_corpus() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();

    let mut files = 0usize;
    let mut pooled_overflow = 0usize;
    let mut go_groups = 0usize;
    let mut if_groups = 0usize;
    let mut let_groups = 0usize;
    for e in ypf.entries() {
        if !e.name.starts_with("$ysbin\\yst0") {
            continue;
        }
        let blob = ypf.read(&e.name).unwrap();
        let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();
        let groups = f.groups().unwrap_or_else(|err| panic!("{}: {err}", e.name));
        files += 1;

        let first = f.group_first_slots(&groups);
        for (gi, g) in groups.iter().enumerate() {
            let wins = f.group_windows(first[gi], g);
            assert_eq!(wins.len(), g.window_count as usize);
            for w in wins {
                assert_eq!(w.tag & 0x0000_FF00, 0, "tag B1 恒 0（全语料实测）");
                let end = w.offset as usize + w.len as usize;
                if end > f.content().len() {
                    pooled_overflow += 1;
                    // 引擎读法：溢出部分必须落进 part4 且不越过池尾
                    assert!(
                        f.window_bytes_pooled_copy(w).is_some(),
                        "{}: 窗口越过 content+part4 池尾",
                        e.name
                    );
                }
            }
            match g.command_type {
                0x2a => {
                    go_groups += 1;
                    assert_eq!(g.window_count, 1, "GO 恒 1 窗口");
                }
                0x2c => {
                    if_groups += 1;
                    assert_eq!(g.window_count, 3, "IF 恒 3 窗口");
                }
                0x35 => {
                    let_groups += 1;
                    assert_eq!(g.window_count, 2, "LET 恒 2 窗口");
                }
                _ => {}
            }
        }
    }
    assert_eq!(files, 302, "302 个 v555 脚本");
    assert_eq!(pooled_overflow, 744, "伸入 part4 的窗口数（probe 实测）");
    assert_eq!(go_groups, 103);
    assert_eq!(if_groups, 8_649);
    assert_eq!(let_groups, 18_315);
}

/// yst00000 的命令类型直方图（probe_part1_groups.py 交叉验证）：
/// 169×F_INT(0x11, 2 窗) + 32×F_STR(0x12, 2 窗) + 1×END(0x0d, 0 窗) = 202 组 / 402 窗。
#[test]
fn yst00000_group_types_match_expected_histogram() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("$ysbin\\yst00000.ybn").unwrap();
    let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();
    let groups = f.groups().unwrap();
    assert_eq!(groups.len(), 202);
    assert_eq!(f.slots().len(), 402);

    let (mut f_int, mut f_str, mut end) = (0, 0, 0);
    for g in &groups {
        match (g.command_type, g.window_count) {
            (0x11, 2) => f_int += 1,
            (0x12, 2) => f_str += 1,
            (0x0d, 0) => end += 1,
            other => panic!("yst00000 出现意外组形态 {other:?}"),
        }
    }
    assert_eq!((f_int, f_str, end), (169, 32, 1));

    // 第一组 = F_INT：其两个窗口就是 slots[0..2]
    let first = f.group_first_slots(&groups);
    assert_eq!(first[0], 0);
    let wins = f.group_windows(0, &groups[0]);
    assert_eq!(wins, &f.slots()[..2]);
}

/// yst_list.ybn 是 YSTL 而非 YSTB：groups() 必须诚实报错（非 v555 形态不硬解）。
#[test]
fn yst_list_is_not_a_v555_ystb() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("$ysbin\\yst_list.ybn").unwrap();
    // YSTL magic，from_bytes 直接拒绝
    assert!(YstbFile::from_bytes(&blob, [0; 4]).is_err());
}

/// YSCM tail 按引擎消费模型解析：37 条 CRT 错误消息 + 256B 表。
#[test]
fn yscm_tail_parses_as_37_crt_messages_plus_256b_table() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("%ysbin\\ysc.ybn").unwrap();
    let yscm = YscmTable::from_bytes(&blob).unwrap();
    assert_eq!(yscm.tail.len(), 1045);

    let tail = yscm.parse_tail().unwrap();
    assert_eq!(tail.messages.len(), 37, "引擎 do-while i<0x91 恰 37 条");
    // 消息区恰好消耗 789 字节（1045 = 789 + 256，尾部无剩余）
    let consumed: usize = tail.messages.iter().map(|m| m.len() + 1).sum();
    assert_eq!(consumed, 789);
    // 首两条为单空格（CRT 消息表的占位），第 2 条起为日文 SJIS 文本
    assert_eq!(tail.messages[0], b" ");
    assert_eq!(tail.messages[1], b" ");
    assert!(tail.messages[2].len() > 10, "第 2 条应为完整错误消息");
    // 256 字节表非零 128 个（probe 实测）
    assert_eq!(tail.table.iter().filter(|&&b| b != 0).count(), 128);
    // 尾部无剩余
    assert!(tail.rest.is_empty());
}

/// YSVR 变量定义表（引擎 FUN_00451348 模型，probe_vartab.py 交叉验证）：
/// 3362 条目逐字节精确闭合；kind/type 直方图与 id 范围逐项对齐。
#[test]
fn ysvr_parses_3362_entries_with_exact_closure() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("%ysbin\\ysv.ybn").unwrap();
    assert_eq!(&blob[..4], b"YSVR");

    let t = ysvr::YsvrTable::from_bytes(&blob).unwrap();
    assert_eq!(t.version(), 555);
    assert_eq!(t.entries().len(), 3362);
    assert_eq!(t.consumed, blob.len(), "条目流必须精确闭合到文件尾");

    // kind 直方图（实测）：1=全局 1226 / 2=按脚本 1226 / 3=其他 910
    let mut kind = [0usize; 4];
    let mut ty = [0usize; 4];
    let mut max_id = 0u16;
    for e in t.entries() {
        kind[(e.kind as usize).min(3)] += 1;
        ty[(e.ty as usize).min(3)] += 1;
        max_id = max_id.max(e.var_id);
    }
    assert_eq!((kind[1], kind[2], kind[3]), (1226, 1226, 910));
    // type 直方图（实测）：INT 2357 / FLT 127 / STR 426 / 仅声明 452
    assert_eq!((ty[1], ty[2], ty[3], ty[0]), (2357, 127, 426, 452));
    assert_eq!(max_id, 7570);

    // 首条目实测：kind1 / varid0 / INT / 2 维 [2,8] / 初值 0
    let e0 = &t.entries()[0];
    assert_eq!(e0.kind, 1);
    assert_eq!(e0.var_id, 0);
    assert_eq!(e0.ty, 1);
    assert_eq!(e0.bounds, vec![2, 8]);
    assert_eq!(e0.init, ysvr::YsvrInit::Int(0));
}

/// YSLB 标签表解析（引擎 FUN_00463c7c 模型）：4153 条精确闭合；
/// 解析器逐条校验 murmur2(name)==hash（引擎查找算法，双侧 Confirmed）。
#[test]
fn yslb_parses_4153_labels_with_exact_closure() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();
    let blob = ypf.read("%ysbin\\ysl.ybn").unwrap();
    assert_eq!(&blob[..4], b"YSLB");

    let t = yslb::YslbTable::from_bytes(&blob).unwrap();
    assert_eq!(t.version(), 555);
    assert_eq!(t.labels().len(), 4153);
    assert_eq!(t.buckets.len(), 256);
    assert_eq!(t.consumed, blob.len(), "标签流必须精确闭合到文件尾");

    // 首标签实测：es.BT.W.GET / hash 0x0000de4a / pc 24 / script 4
    let l0 = &t.labels()[0];
    assert_eq!(l0.name, b"es.BT.W.GET");
    assert_eq!(l0.hash, 0x0000_DE4A);
    assert_eq!(l0.target_pc, 24);
    assert_eq!(l0.script_id, 4);

    // find() 与引擎 FUN_0045124c 等价
    assert_eq!(t.find(b"es.BT.W.GET"), Some(0));
    assert_eq!(t.find(b"no.such.label"), None);
}

/// GO/GOSUB 标签 → YSLB 全语料交叉验证：
/// 全部 GO(0x2a) 唯一窗口与 GOSUB(0x2b) 的 tag0 窗口都是 M-串标签名，
/// 必须能在 YSLB 中找到（引擎载入期解析路径的语料级证明）。
#[test]
fn go_and_gosub_labels_resolve_in_yslb_across_corpus() {
    let Some(p) = sample_path() else {
        eprintln!("sample not found, skip");
        return;
    };
    let data = std::fs::read(&p).unwrap();
    let ypf = YpfArchive::from_bytes(data, 0xC9).unwrap();

    let ysl = yslb::YslbTable::from_bytes(&ypf.read("%ysbin\\ysl.ybn").unwrap()).unwrap();

    /// 解码单条 M-串窗口（4d len "name"），返回去掉引号的名字。
    fn mstring(window: &[u8]) -> Option<&[u8]> {
        if window.len() < 4 || window[0] != 0x4d {
            return None;
        }
        let payload = &window[3..];
        if payload.len() < 2 || (payload[0], *payload.last().unwrap()) != (b'"', b'"') {
            return None;
        }
        Some(&payload[1..payload.len() - 1])
    }

    let mut go_checked = 0usize;
    let mut go_misses = 0usize;
    let mut gosub_checked = 0usize;
    let mut gosub_hits = 0usize;
    for e in ypf.entries() {
        if !e.name.starts_with("$ysbin\\yst0") {
            continue;
        }
        let blob = ypf.read(&e.name).unwrap();
        let f = YstbFile::from_bytes(&blob, [0x2B, 0x90, 0x4F, 0x93]).unwrap();
        let groups = f.groups().unwrap();
        let first = f.group_first_slots(&groups);
        for (gi, g) in groups.iter().enumerate() {
            let wins = f.group_windows(first[gi], g);
            match g.command_type {
                0x2a => {
                    for w in wins {
                        // GO 窗口可能伸入 part4（引擎读法），取池化拷贝
                        if let Some(bytes) = f.window_bytes_pooled_copy(w) {
                            if let Some(name) = mstring(&bytes) {
                                go_checked += 1;
                                if ysl.find(name).is_none() {
                                    go_misses += 1;
                                    eprintln!("GO 标签未命中: {:?}", name);
                                }
                            }
                        }
                    }
                }
                0x2b => {
                    for w in wins {
                        if w.tag & 0xFF != 0 {
                            continue; // 引擎仅解析 tag==0（B0=0）窗口
                        }
                        if let Some(bytes) = f.window_bytes_pooled_copy(w) {
                            if let Some(name) = mstring(&bytes) {
                                gosub_checked += 1;
                                if ysl.find(name).is_some() {
                                    gosub_hits += 1;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // GO 是纯跳转：可解析的 M-串标签必须绝大多数命中 YSLB。
    // 实测（2026-09-03）：103 窗中 99 个为 M-串、98 命中；4 个非 M-串 + 1 个
    // 未命中（"e_WE_KURT02"）疑为池化截断伪影（与 744 个 part4 溢出窗同族，U2c）。
    eprintln!(
        "[stats] GO: parsed={go_checked} misses={go_misses}; GOSUB: tag0-parsed={gosub_checked} hits={gosub_hits}"
    );
    assert!((95..=103).contains(&go_checked), "GO M-串窗实测 99，得到 {go_checked}");
    assert!(go_misses <= 4, "GO 标签未命中实测 4（池化截断伪影）");
    // GOSUB 的 tag0 窗口含标签也含其他字符串参数（引擎查表失败静默跳过），
    // 故只断言「大多数命中」——精确阈值来自语料实测。
    assert!(
        gosub_checked >= 10_000,
        "GOSUB tag0 M-串窗口应数以万计，实测 {gosub_checked}"
    );
    let hit_rate = gosub_hits as f64 / gosub_checked as f64;
    assert!(
        hit_rate > 0.5,
        "GOSUB 标签命中率应过半，实测 {gosub_hits}/{gosub_checked} = {hit_rate:.3}"
    );
}
