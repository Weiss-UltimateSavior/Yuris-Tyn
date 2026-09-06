//! 词法/行解析器实现(P7.2)。
//!
//! 流水线:原始字节 → 按 `b'\n'` 切行(剥 `b'\r'`)→ 逐行按指定编码解码
//! (失败报行号)→ 注释剥离(`//` 与 `/* */`,块注释跨行;字符串内不剥离;
//! 行内字符串必须闭合)→ 标签行/令牌行解析 → 元素流 + 段表。
//!
//! 字节级扫描安全性:本语法的全部结构定界符(`\` `#` `(` `)` `,` `"` `/`)
//! 均为 ASCII,而 SJIS/Big5 多字节序列的首/续字节(≥0x81 / ≥0x80)都不会与
//! 这些 ASCII 字节冲突;且两种编码的续字节范围均不含 `0x0A`/`0x0D`,故先按
//! 字节切行再逐行解码、以及在解码后的文本上按字节扫描,都是安全的。

use std::collections::HashMap;

use encoding_rs::Encoding;

use crate::{Element, Param, Scenario, ScenarioError};

/// 解析入口(由 `lib.rs` 的公共 API 调用)。
pub(super) fn parse(
    bytes: &[u8],
    encoding: &'static Encoding,
) -> Result<Scenario, ScenarioError> {
    let mut elements: Vec<Element> = Vec::new();
    let mut labels: HashMap<String, usize> = HashMap::new();
    let mut in_block_comment = false;
    let mut line_count = 0usize;

    for (idx, raw) in split_lines(bytes).iter().enumerate() {
        let line_no = idx + 1;
        line_count = line_no;

        // ---- 解码(逐行,失败可精确报行号;不做替换字符静默吞)----
        // encoding_rs 0.8:decode_without_bom_handling 返回 (Cow, had_errors) 二元组
        let (text, had_errors) = encoding.decode_without_bom_handling(raw);
        if had_errors {
            return Err(ScenarioError::Decode {
                line: line_no,
                encoding: encoding.name(),
            });
        }

        // ---- 注释剥离(块注释状态跨行;字符串内容保留)----
        let (stripped, still_in_block) = strip_comments(&text, in_block_comment, line_no)?;
        in_block_comment = still_in_block;

        let line = stripped.trim();
        if line.is_empty() {
            continue;
        }

        parse_line(line, line_no, &mut elements, &mut labels)?;
    }

    // 文件结束仍处于块注释 = 未闭合
    if in_block_comment {
        return Err(ScenarioError::Syntax {
            line: line_count,
            message: "块注释未闭合(文件结束于 /* 之后)".to_string(),
        });
    }

    Ok(Scenario { elements, labels })
}

/// 按字节切行:以 `b'\n'` 分割,剥去行尾 `b'\r'`(语料全 CRLF)。
fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes
        .split(|&b| b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .collect()
}

/// 剥离一行中的注释。返回 `(剥离后文本, 是否仍处于块注释)`。
///
/// - `//` 行注释:丢弃其后内容(仅当不在字符串/块注释内);
/// - `/* */` 块注释:进入跨行块注释状态(仅当不在字符串内);块注释内容
///   全部丢弃(语料块注释内无引号,无需考虑其中的字符串态);
/// - 字符串(`"…"`)内的 `//`、`/*` 是字面数据,不剥离,内容**保留**;
/// - 行结束时字符串必须闭合(字符串不跨行)→ 否则报错;
/// - 块注释外的孤立 `*/` → 报错(合法结构不会出现,语料 0 例)。
fn strip_comments(
    line: &str,
    mut in_block: bool,
    line_no: usize,
) -> Result<(String, bool), ScenarioError> {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_block {
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block = false;
                i += 2;
            } else {
                i += 1;
            }
        } else if in_str {
            // 字符串是数据:内容原样保留(含其中的 // /* 等)
            let ch = line[i..].chars().next().unwrap();
            if ch == '"' {
                in_str = false;
            }
            out.push(ch);
            i += ch.len_utf8();
        } else if b == b'"' {
            in_str = true;
            out.push('"');
            i += 1;
        } else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break; // 行注释:丢弃其后全部
        } else if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            return Err(ScenarioError::Syntax {
                line: line_no,
                message: "孤立的 */(块注释未开始)".to_string(),
            });
        } else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            in_block = true;
            i += 2;
        } else {
            let ch = line[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    if in_str {
        return Err(ScenarioError::Syntax {
            line: line_no,
            message: "字符串未闭合(引号必须同行配对)".to_string(),
        });
    }
    Ok((out, in_block))
}

/// 解析单行(已剥注释、已 trim、非空)。
///
/// - `#…` → 标签行(整行必须是 `#` + `[A-Za-z0-9_]+`);
/// - 其他 → 令牌循环:命令 `\CMD.MODS(args)` / 行号标记 `(ID:n)`;
///   行首裸文本(可带 `(ID:n)` 前缀)→ 对话元素(吞到行尾);
///   命令/标记之后出现裸文本 = 未证实语法 → 报错。
fn parse_line(
    line: &str,
    line_no: usize,
    elements: &mut Vec<Element>,
    labels: &mut HashMap<String, usize>,
) -> Result<(), ScenarioError> {
    // ---- 标签行 ----
    // `#name` 与 `#=name` 均为标签定义;`#=` 形态的标签名取 `=` 之后
    // (NEKO-NIN exHeart `#=TR_2A` 实证;源码 `#=es.BT.CG.SET` 即 es 族
    // 跳转目标,与 docs/opcode/opcode-table.md 0x23 `#` 前缀记录一致)。
    let label_line = line.strip_prefix('#').unwrap_or(line);
    let label_name = label_line.strip_prefix('=').unwrap_or(label_line);
    if line.starts_with('#') {
        let name = label_name;
        let valid = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !valid {
            return Err(ScenarioError::Syntax {
                line: line_no,
                message: format!("非法标签行(应为 #[=][A-Za-z0-9_]+):{line:?}"),
            });
        }
        labels.insert(name.to_string(), elements.len());
        elements.push(Element::Label {
            line: line_no,
            name: name.to_string(),
        });
        return Ok(());
    }

    // ---- 令牌循环 ----
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    // 行首挂起的 (ID:n):下一令牌决定其形态(后随命令/标记 → LineId;
    // 后随裸文本 → Dialogue 前缀;行尾 → LineId)。
    let mut pending_id: Option<u32> = None;
    // 是否已产出命令/LineId(其后不允许再出现裸文本)。
    let mut produced = false;

    loop {
        // 跳过令牌间空白
        while i < len && (bytes[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i >= len {
            break;
        }
        if bytes[i] == b'\\' {
            // ---- 命令 ----
            if let Some(id) = pending_id.take() {
                elements.push(Element::LineId { line: line_no, id });
            }
            let (el, next) = parse_command(line, i, line_no)?;
            elements.push(el);
            produced = true;
            i = next;
        } else if bytes[i] == b'(' && bytes[i..].starts_with(b"(ID:") {
            // ---- 行号标记 (ID:数字) ----
            let mut j = i + 4;
            let dstart = j;
            while j < len && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j == dstart || j >= len || bytes[j] != b')' {
                return Err(ScenarioError::Syntax {
                    line: line_no,
                    message: "(ID:…) 标记格式非法(应为 (ID:十进制数字))".to_string(),
                });
            }
            let id: u32 = line[dstart..j]
                .parse()
                .map_err(|_| ScenarioError::Syntax {
                    line: line_no,
                    message: "(ID:…) 数字溢出 u32".to_string(),
                })?;
            if pending_id.is_none() && !produced {
                // 行首首个标记:形态待定
                pending_id = Some(id);
            } else {
                if let Some(pid) = pending_id.take() {
                    elements.push(Element::LineId { line: line_no, id: pid });
                }
                elements.push(Element::LineId { line: line_no, id });
                produced = true;
            }
            i = j + 1;
        } else {
            // ---- 裸文本 ----
            if produced {
                return Err(ScenarioError::Syntax {
                    line: line_no,
                    message: format!("命令/标记后存在裸文本(未证实语法):{line:?}"),
                });
            }
            // 吞到行尾(行已 trim,无首尾空白)
            elements.push(Element::Dialogue {
                line: line_no,
                id: pending_id.take(),
                text: line[i..].to_string(),
            });
            return Ok(());
        }
    }

    // 行 = `(ID:n)` 独行(或 (ID:n) 后仅空白):作为 LineId 产出
    if let Some(id) = pending_id {
        elements.push(Element::LineId { line: line_no, id });
    }
    Ok(())
}

/// 解析一个命令令牌。`start` 指向 `\`。
///
/// 形态:`\` NAME ( `.` NAME )* [ `(` args `)` ];
/// 首个名段以 ASCII 字母开头,后续修饰段可数字开头(`\SP.2A` 实证)。
/// 括号可选(`\GO.TITLE`/`\END` 等无括号形式)。
fn parse_command(
    line: &str,
    start: usize,
    line_no: usize,
) -> Result<(Element, usize), ScenarioError> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    debug_assert!(bytes[start] == b'\\');

    // 名段扫描:连续的 [A-Za-z0-9_],按 '.' 分段
    let mut segs: Vec<&str> = Vec::new();
    let mut i = start + 1;
    loop {
        let s = i;
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i == s {
            return Err(ScenarioError::Syntax {
                line: line_no,
                message: format!("非法命令名(\\ 后应为命令名):{line:?}"),
            });
        }
        segs.push(&line[s..i]);
        if i < len && bytes[i] == b'.' {
            i += 1; // 继续修饰段
        } else {
            break;
        }
    }
    // 首段必须字母开头(命令名);修饰段允许数字开头(SP.2A)
    if !segs[0].bytes().next().is_some_and(|b| b.is_ascii_alphabetic()) {
        return Err(ScenarioError::Syntax {
            line: line_no,
            message: format!("非法命令名(首段须字母开头):{}", segs.join(".")),
        });
    }

    // 可选括号参数
    let mut params = Vec::new();
    if i < len && bytes[i] == b'(' {
        let (ps, next) = parse_args(line, i, line_no)?;
        params = ps;
        i = next;
    }

    Ok((
        Element::Command {
            line: line_no,
            name: segs[0].to_string(),
            modifiers: segs[1..].iter().map(|s| s.to_string()).collect(),
            params,
        },
        i,
    ))
}

/// 解析命令参数列表。`open` 指向 `(`,返回 `(参数列表, 闭括号后位置)`。
///
/// - 顶层逗号分割(字符串内的逗号不分割);
/// - `()` = 零参数;`(,)` = 两个空槽;`(,800)` = [Empty, Int];
/// - 字符串内可含任意字符(含逗号/括号/CJK),不含引号本身
///   (语料实证:串内无 `"` 无转义);
/// - 串外不允许嵌套 `(`(语料 0 例,出现即报错);
/// - 行尾前未闭合 → 报错(语料实证:命令不跨行)。
fn parse_args(
    line: &str,
    open: usize,
    line_no: usize,
) -> Result<(Vec<Param>, usize), ScenarioError> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut params: Vec<Param> = Vec::new();
    let mut cur_start = open + 1;
    let mut i = open + 1;
    let mut in_str = false;
    let mut saw_comma = false;

    loop {
        if i >= len {
            return Err(ScenarioError::Syntax {
                line: line_no,
                message: "括号未闭合(命令参数不跨行)".to_string(),
            });
        }
        let b = bytes[i];
        if in_str {
            if b == b'"' {
                in_str = false;
            }
            i += 1;
        } else {
            match b {
                b'"' => in_str = true,
                b',' => {
                    params.push(classify_param(&line[cur_start..i], line_no)?);
                    saw_comma = true;
                    cur_start = i + 1;
                    // 注意:此处不得再 i += 1 —— 外层 else 的 i += 1 对所有
                    // 非 return 分支统一执行;若此处再加一次,每个逗号会
                    // 吞掉其后的一个字符(NEKO-NIN exHeart「逗号紧跟参数」
                    // 风格语料实证:参数整体错位,含空槽行报括号未闭合)。
                }
                b')' => {
                    // `()` = 零参数;否则收尾当前参数
                    if !(params.is_empty() && !saw_comma && cur_start == i) {
                        params.push(classify_param(&line[cur_start..i], line_no)?);
                    }
                    return Ok((params, i + 1));
                }
                b'(' => {
                    return Err(ScenarioError::Syntax {
                        line: line_no,
                        message: "参数中含嵌套括号(未证实语法)".to_string(),
                    });
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// 参数分类:原始片段(未 trim)→ [`Param`]。
///
/// - 空白/空 → `Empty`;
/// - `"…"`(两端引号、内部无引号)→ `Str`(剥引号;`""` = 空串);
/// - `[+-]?数字+` → `Int`;`[+-]?数字+.数字+` → `Float`;
/// - 其余 → 无引号裸词 `Str`(如 `white`/`SCENARIO_MAIN`/`pac/op.ypf`);
/// - 含引号但不构成合法字符串形态 → 报错(不做静默裸词吞)。
fn classify_param(raw: &str, line_no: usize) -> Result<Param, ScenarioError> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(Param::Empty);
    }
    if t.contains('"') {
        // 必须是完整 "…" 形态
        let inner = t
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .filter(|s| !s.contains('"'));
        return match inner {
            Some(s) => Ok(Param::Str(s.to_string())),
            None => Err(ScenarioError::Syntax {
                line: line_no,
                message: format!("字符串参数引号不配对:{t:?}"),
            }),
        };
    }
    // 前导零数字("01"/"007")按裸词 Str 保留:YU-RIS 标签常带前导零
    // (NEKO-NIN exHeart `\GO(01)` → 标签 `#01` 实证),Int 化会丢宽度
    // 导致跳转目标 "1" 查无此标签。
    if !(t.len() > 1 && t.trim_start_matches(['+', '-']).starts_with('0')) {
        if let Some(int) = parse_int(t) {
            return Ok(Param::Int(int));
        }
    }
    if let Some(f) = parse_float(t) {
        return Ok(Param::Float(f));
    }
    Ok(Param::Str(t.to_string()))
}

/// `[+-]?[0-9]+` → i64(溢出返回 None,由调用方视为裸词?否 —— 溢出报错更响;
/// 但溢出数字当裸词 Str 也无信息损失。选择:溢出 → None → 裸词 Str,
/// 因为「数字形态但超范围」在引擎侧语义未知,保留原串不丢数据)。
fn parse_int(t: &str) -> Option<i64> {
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<i64>().ok()
}

/// `[+-]?[0-9]+.[0-9]+` → f64(全库参数位 0 例,语法保留)。
fn parse_float(t: &str) -> Option<f64> {
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    let (ip, fp) = body.split_once('.')?;
    if ip.is_empty()
        || fp.is_empty()
        || !ip.bytes().all(|b| b.is_ascii_digit())
        || !fp.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    t.parse::<f64>().ok()
}
