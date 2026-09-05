//! P6 资源层测试(成果 63/65 勘误后):多包索引解析 + 魔数断言。
//!
//! 引擎侧实证(P6.2/P6.3 侦查 + probe_census.py 全包普查):
//! - 名字段 = 虚拟根字节(1B) + 路径 + [类型码(1B,仅 se 型,XOR 后 0xCB=PNG/
//!   0xCF=OGG)] + NUL;
//! - bn 型条目(脚本/文本):NUL 后 flag∈{0,1}(zlib/stored);
//! - se 型条目(资源):NUL 后直接字段(明文 stored);
//! - 同一包(update1.ypf)两种布局并存;总开销均为 len+26;
//! - op.ypf/op_c.ypf 非 YPF(ASF/WMV 头)。
//! 样本缺失时测试跳过(CARGO_MANIFEST_DIR 上溯定位工作区根;成果 65 勘误:
//! cargo test 的 CWD 是 crate 根,纯相对路径会静默跳过断言)。

use yuris_format::ypf::YpfIndex;

fn pac_dir() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("YURIS_SAMPLE_PAC") {
        let p = std::path::PathBuf::from(p);
        if p.is_dir() {
            return Some(p);
        }
    }
    let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..5 {
        dir.push("AnimalTrailGirlishSquare 2/pac");
        if dir.is_dir() {
            return Some(dir);
        }
        dir.pop();
        dir.pop();
        if !dir.pop() {
            break;
        }
    }
    None
}

const NAME_KEY: u8 = 0xC9;

/// bn.ypf:bn 型布局,309 条,flag∈{0,1}(zlib/stored)。
#[test]
fn bn_ypf_bn_layout() {
    let Some(dir) = pac_dir() else { return };
    let idx = YpfIndex::from_path(&dir.join("bn.ypf"), NAME_KEY).unwrap();
    assert_eq!(idx.header.file_count, 309);
    assert_eq!(idx.entries.len(), 309);
    assert!(idx.flags.iter().all(|&f| f <= 1), "{:?}", idx.flags);
    // 双索引:带根全名($ysbin\...)与剥根名(ysbin\...)均可查
    assert!(idx.map.contains_key(&b"$ysbin\\yst00034.ybn".to_vec()));
    assert!(idx.map.contains_key(&b"ysbin\\yst00034.ybn".to_vec()));
    assert!(idx.names.contains(&b"$ysbin\\yst00034.ybn".to_vec()));
}

/// se.ypf:se 型布局(OGG 码 0xCF),742 条,双索引可查剥根名。
#[test]
fn se_ypf_se_layout() {
    let Some(dir) = pac_dir() else { return };
    let idx = YpfIndex::from_path(&dir.join("se.ypf"), NAME_KEY).unwrap();
    assert_eq!(idx.header.file_count, 742);
    // 主体 OGG(XOR 后码 0xCF);剥码后名字以 'g' 结尾
    assert!(
        idx.flags.iter().filter(|&&f| f == 0xCF).count() >= 736,
        "OGG 条目应 ≥736"
    );
    // 双索引:剥根名 se\se017.ogg 可查(根字节被剥)
    assert!(idx.map.contains_key(&b"se\\se017.ogg".to_vec()));
}

/// cg.ypf:se 型布局(PNG 码 0xCB),5655 条;首条数据头 = 标准 PNG。
#[test]
fn cg_ypf_png_stored() {
    let Some(dir) = pac_dir() else { return };
    let path = dir.join("cg.ypf");
    let idx = YpfIndex::from_path(&path, NAME_KEY).unwrap();
    assert_eq!(idx.header.file_count, 5655);
    assert_eq!(idx.entries.len(), 5655);
    assert!(idx.flags.iter().all(|&f| f == 0xCB), "PNG 码恒 0xCB");
    // 首条字段(probe 实测):uncomp=comp=15485, off=0x233edc5e
    let e = &idx.entries[0];
    assert_eq!(e.uncompressed_len, 15485);
    assert_eq!(e.compressed_len, 15485);
    assert_eq!(e.offset, 0x233edc5e);
    // 剥根名可查
    assert!(idx.map.contains_key(
        &b"cg\\stand\\m_030\\b_tet\\b_tet_1a\\b_tet_1a0401.png".to_vec()
    ));
    // 数据头(明文 stored)
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(&path).unwrap();
    f.seek(SeekFrom::Start(e.offset as u64)).unwrap();
    let mut head = [0u8; 8];
    f.read_exact(&mut head).unwrap();
    assert_eq!(&head, b"\x89PNG\r\n\x1a\n", "cg.ypf 数据应为明文 PNG");
}

/// update1.ypf:混合布局(bn 型 txt×18 + se 型 ogg×1094 / png×194)。
#[test]
fn update1_mixed_layout() {
    let Some(dir) = pac_dir() else { return };
    let idx = YpfIndex::from_path(&dir.join("update1.ypf"), NAME_KEY).unwrap();
    assert_eq!(idx.header.file_count, 1306);
    let zlib = idx.flags.iter().filter(|&&f| f == 0x01).count();
    let png = idx.flags.iter().filter(|&&f| f == 0xCB).count();
    let ogg = idx.flags.iter().filter(|&&f| f == 0xCF).count();
    assert_eq!(zlib, 18, "zlib txt 条目");
    assert_eq!(png, 194, "PNG 条目");
    assert_eq!(ogg, 1094, "OGG 条目");
    // 双索引:剥根名 scenario txt 可查
    assert!(idx.map.contains_key(&b"scenario\\maho2_23b.txt".to_vec()));
}

/// op.ypf:非 YPF(ASF/WMV)→ BadMagic(P6.2 定性)。
#[test]
fn op_ypf_is_not_ypf() {
    let Some(dir) = pac_dir() else { return };
    let err = match YpfIndex::from_path(&dir.join("op.ypf"), NAME_KEY) {
        Err(e) => e,
        Ok(_) => panic!("op.ypf 应为非 YPF(ASF/WMV)"),
    };
    assert!(matches!(err, yuris_core::Error::BadMagic { .. }));
}

/// P6.1:多包挂载 —— update1.ypf 与 cg.ypf 剥根名**交集 = 0**(成果 65
/// 实证修正:update1 是纯增量包 —— 新增 EV 场景/语音,不覆盖 cg 条目;
/// 成果 63/64「同名覆盖 ≥100」为坏解析下的错误结论)。
#[test]
fn update1_is_pure_additive_no_name_overlap() {
    let Some(dir) = pac_dir() else { return };
    let cg = YpfIndex::from_path(&dir.join("cg.ypf"), NAME_KEY).unwrap();
    let up = YpfIndex::from_path(&dir.join("update1.ypf"), NAME_KEY).unwrap();
    let strip = |idx: &YpfIndex| -> std::collections::HashSet<_> {
        idx.names.iter().map(|n| n[1..].to_vec()).collect()
    };
    let (cs, us) = (strip(&cg), strip(&up));
    let overlap = cs.intersection(&us).count();
    assert_eq!(overlap, 0, "update1/cg 剥根名交集应 = 0(纯增量),实得 {overlap}");
    assert!(us.iter().filter(|n| n.starts_with(b"cg\\")).count() >= 100);
}
