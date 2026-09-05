//! P6.3 资源读取链路测试(成果 64):挂载 → seek 读取 → 解码。
//!
//! 实证基础(成果 63):
//! - cg.ypf 条目 = 标准 PNG(明文 stored);bgm/vo 条目 = 标准 OGG;
//! - sc.ypf scenario 文本 = zlib(bn 型布局);
//! - update1.ypf 与 cg.ypf 同名条目(cg\ev 族)→ 后挂载优先读取。
//! 样本缺失时测试跳过。

use yuris_format::ypf::YpfReader;
use yuris_resource::{ResourceError, ResourceStack};

fn game_dir() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("YURIS_SAMPLE_PAC") {
        let p = std::path::PathBuf::from(p);
        if p.is_dir() {
            // 允许直接指向 pac 目录
            if p.file_name().map(|f| f == "pac").unwrap_or(false) {
                return p.parent().map(|p| p.to_path_buf());
            }
            return Some(p);
        }
    }
    // cargo test 的 CWD = crate 根(相对路径会静默跳过断言,成果 65 勘误)
    // —— 从 manifest 目录上溯定位工作区根。
    let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..5 {
        dir.push("AnimalTrailGirlishSquare 2");
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

/// cg.ypf:seek 读取 PNG 条目 → image 解码成功(尺寸 > 0)。
#[test]
fn cg_png_decode_chain() {
    let Some(dir) = game_dir() else { return };
    let mut stack = ResourceStack::new(0xC9);
    stack.mount(dir.join("pac").join("cg.ypf")).unwrap();
    // probe_pac_layout 实测 cg.ypf 首条名
    let name = "cg\\stand\\m_030\\b_tet\\b_tet_1a\\b_tet_1a0401.png";
    let img = stack.read_image(name).unwrap();
    assert!(
        img.width() > 0 && img.height() > 0,
        "{}x{}",
        img.width(),
        img.height()
    );
    // 存在性 + 正斜杠归一
    assert!(stack.exists("cg/stand/m_030/b_tet/b_tet_1a/b_tet_1a0401.png"));
}

/// bgm.ypf:OGG 条目 → symphonia probe 成功(声道/采样率 > 0)。
#[test]
fn bgm_ogg_probe_chain() {
    let Some(dir) = game_dir() else { return };
    let mut stack = ResourceStack::new(0xC9);
    stack.mount(dir.join("pac").join("bgm.ypf")).unwrap();
    // probe 实测 bgm.ypf 条目名(虚拟根前缀 % 实存于索引)
    let name = "%bgm\\bgm14.ogg";
    let (ch, rate) = stack.read_audio_header(name).unwrap();
    assert_eq!(ch, 2, "BGM 应为立体声");
    assert!(rate >= 44100, "采样率 {rate}");
}

/// sc.ypf:zlib 文本条目(bn 型)读取 → 解压 → scenario 明文。
#[test]
fn sc_zlib_text_read() {
    let Some(dir) = game_dir() else { return };
    let mut stack = ResourceStack::new(0xC9);
    stack.mount(dir.join("pac").join("sc.ypf")).unwrap();
    // 成果 49:scenario 明文含 # 段标签(虚拟根 $ 前缀实存)。
    // 注:start.txt 实为 54 字节的跳转表(#SCENARIO_MAIN → GO);完整剧本
    // 在 scenario\maho2_*.txt(故长度断言取实测值,2026-09-05 勘误)。
    let data = stack.read("$scenario\\start.txt").unwrap();
    assert!(data.len() >= 54, "len={}", data.len());
    assert!(
        data.windows(9).any(|w| w == b"#SCENARIO"),
        "scenario start.txt 应含 #SCENARIO 段标签"
    );
}

/// FILEPRIORITY(松散文件优先于封包,Confirmed):临时根里的同名文件
/// 覆盖封包条目。注:update1/cg 实测**无**同名条目(纯增量包,成果 65),
/// 故覆盖语义以松散根为可测面。
#[test]
fn loose_file_overrides_pack() {
    let Some(dir) = game_dir() else { return };
    let mut stack = ResourceStack::new(0xC9);
    stack.mount(dir.join("pac").join("bgm.ypf")).unwrap();
    // 封包内确有该条目(剥根名索引)
    assert!(stack.read("bgm\\bgm14.ogg").is_ok());

    let tmp = std::env::temp_dir().join("yuris_p6_loose_test");
    std::fs::create_dir_all(tmp.join("bgm")).unwrap();
    std::fs::write(tmp.join("bgm").join("bgm14.ogg"), b"OVERRIDE").unwrap();
    stack.add_loose_root(&tmp);
    let data = stack.read("bgm\\bgm14.ogg").unwrap();
    assert_eq!(data, b"OVERRIDE", "松散文件应优先于封包条目");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// mount_game_dir:全包挂载 + op.ypf(ASF)跳过 + PNG 全链路仍通。
#[test]
fn mount_game_dir_skips_asf() {
    let Some(dir) = game_dir() else { return };
    let mut stack = ResourceStack::new(0xC9);
    stack.mount_game_dir(&dir).unwrap();
    // op.ypf 被 BadMagic 跳过(不阻断);cg PNG 可读
    let img = stack
        .read_image("cg\\stand\\m_030\\b_tet\\b_tet_1a\\b_tet_1a0401.png")
        .unwrap();
    assert!(img.width() > 0);
}

/// NotFound 语义:未挂载/不存在的路径。
#[test]
fn not_found_semantics() {
    let mut stack = ResourceStack::new(0xC9);
    let err = stack.read("no/such/file.png").unwrap_err();
    assert!(matches!(err, ResourceError::NotFound(_)), "{err}");
}
