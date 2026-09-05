//! scenario 解析器单测。
//!
//! 注:原 `tests.rs` 在环境迁移(2026-09-04 会话 → 2026-09-05 会话)中丢失,
//! 本文件按 lib.rs 模块文档记载的语料形态**重建最小回归集**(P7.2 补录)。
//! 断言全部以解析器**实测行为**为准(2026-09-05 逐例试验)。

use super::*;

/// 语料主流对话形态:`(ID:n)\VO(x)\LE("…")\LT("…")` → 四元素。
#[test]
fn vo_le_lt_mainstream() {
    let src = b"(ID:123)\\VO(x)\\LE(\"\x83\x65\x83\x58\x83\x67\")\\LT(\"\x83\x65\x83\x58\x83\x67\")\r\n";
    let sc = parse_scenario(src).unwrap();
    let kinds: Vec<&str> = sc
        .elements
        .iter()
        .map(|e| match e {
            Element::LineId { .. } => "lineid",
            Element::Command { name, .. } => "cmd",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["lineid", "cmd", "cmd", "cmd"]);
    for e in &sc.elements {
        match e {
            Element::LineId { line, .. } => assert_eq!(*line, 1),
            Element::Command { line, .. } => assert_eq!(*line, 1),
            _ => panic!("unexpected element"),
        }
    }
}

/// 命令形态:修饰符链、首槽跳位、引号串/裸词/负整数。
#[test]
fn command_modifiers_and_params() {
    let sc =
        parse_scenario(b"\\BG(white,200,0)\r\n\\BGM(,800)\r\n\\GO.G.IF(1,-2)\r\n").unwrap();
    let cmds: Vec<&Element> = sc
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Command { .. }))
        .collect();
    assert_eq!(cmds.len(), 3);
    match cmds[0] {
        Element::Command { name, modifiers, params, .. } => {
            assert_eq!(name, "BG");
            assert!(modifiers.is_empty());
            assert_eq!(
                params,
                &[Param::Str("white".into()), Param::Int(200), Param::Int(0)]
            );
        }
        _ => unreachable!(),
    }
    match cmds[1] {
        Element::Command { name, params, .. } => {
            assert_eq!(name, "BGM");
            // 首槽跳位 → Empty(parser.rs §参数文档「(,800) = [Empty, Int]」)
            assert_eq!(params, &[Param::Empty, Param::Int(800)]);
        }
        _ => unreachable!(),
    }
    match cmds[2] {
        Element::Command { name, modifiers, params, .. } => {
            assert_eq!(name, "GO");
            assert_eq!(modifiers, &["G".to_string(), "IF".to_string()]);
            assert_eq!(params, &[Param::Int(1), Param::Int(-2)]);
        }
        _ => unreachable!(),
    }
}

/// `#标签` 段定义 + 标签表指向对应元素下标;`//` 注释/空行不产出元素。
#[test]
fn labels_and_comment_lines() {
    let src = b"// comment\r\n\r\n#SCENARIO_MAIN\r\n\\CMG()\r\n#NEXT\r\n";
    let sc = parse_scenario(src).unwrap();
    assert_eq!(sc.elements.len(), 3); // Label, Command, Label
    assert!(matches!(sc.elements[0], Element::Label { line: 3, .. }));
    assert_eq!(sc.labels.get("SCENARIO_MAIN"), Some(&0));
    assert_eq!(sc.labels.get("NEXT"), Some(&2));
}

/// 块注释跨行;字符串内 `//` 不剥离。
#[test]
fn block_comment_and_string_guard() {
    let src = b"/* head\r\n span */\\BGM(\"a//b\")\r\n";
    let sc = parse_scenario(src).unwrap();
    assert_eq!(sc.elements.len(), 1);
    match &sc.elements[0] {
        Element::Command { name, params, line, .. } => {
            assert_eq!(name, "BGM");
            assert_eq!(*line, 2);
            assert_eq!(params, &[Param::Str("a//b".into())]);
        }
        _ => panic!("unexpected element"),
    }
}

/// 对话裸文本(可带 `(ID:n)` 前缀)。
#[test]
fn dialogue_with_id() {
    let sc = parse_scenario(b"(ID:5)text\r\n").unwrap();
    assert_eq!(
        sc.elements,
        vec![Element::Dialogue {
            line: 1,
            id: Some(5),
            text: "text".into(),
        }]
    );
}

/// 非法字节序列:报 Decode 且携带 1 基行号(不做替换静默吞)。
/// (0x81 = SJIS 双字节首字节,0x20 非合法尾字节)
#[test]
fn invalid_bytes_report_line() {
    let src = b"ok\r\n\x81\x20\r\n";
    let err = parse_scenario(src).unwrap_err();
    match err {
        ScenarioError::Decode { line, encoding } => {
            assert_eq!(line, 2);
            assert_eq!(encoding, "Shift_JIS");
        }
        other => panic!("unexpected: {other:?}"),
    }
}
