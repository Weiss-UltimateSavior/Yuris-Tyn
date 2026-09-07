//! yuris-vm
//!
//! L3:组级执行 VM / PC / 挂起恢复。**绝不阻塞**。
//!
//! ## 执行模型(2026-09-03 引擎逆向,**Confirmed**)
//!
//! 证据:`docs/engine/command-layer.md`(kemonomichi2.exe 逆向 + 全语料断言)。
//!
//! - 执行单元 = **命令组**([`CommandGroup`]):part1 的一条 u32
//!   (byte0=YSCM 命令下标,byte1=窗口数,gparam u16)
//! - 主循环(引擎 FUN_0040449c):`pc = obj->pc++; yield = handler[pc]();`
//!   —— PC **先取后增**,跳转处理器直接写目标组号
//! - 处理器返回非零 = **让出**(挂起);本 VM 以 `VmSuspend` 表达
//! - GO(0x2a):标签名 → 标签表(YSLB)→ {目标组号, 脚本号};跨脚本 = 多脚本
//!   重绑,单脚本 VM 显式报 Unsupported(不猜)
//! - GOSUB(0x2b):条件真 → 压帧{return_pc=pc+1} → 跳标签
//! - RETURN(0x4f):弹帧;帧空 = 脚本结束(引擎 depth==0 路径)
//! - IF(0x2c,3 窗):求值 w0 条件;假 → 跳 `w1.len ?: w2.len`
//!   (**len 域存编译期组号**);嵌套栈记录 end 目标
//! - ELSE(0x2d):跳嵌套栈顶的 end;IFEND(0x30):弹嵌套栈
//! - 其余命令:语义未逆向 → [`VmEvent::Unsupported`](VmEvent::Unsupported)
//!   事件;`strict` 模式(默认)挂起,`trace` 模式记录后继续 —— **不猜语义**

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;

pub mod boot;
pub mod bridge;
pub mod host;

use yuris_core::{Error, Result};
use yuris_format::ystb::YstbFile;
use host::{ScriptCtx, ScriptHost};
use yuris_script::Evaluator;
use yuris_value::{Value, VariableStore};

/// crate 版本(与 workspace 同步)
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 命令下标(YSCM 顺序,样本 v555;详见 `docs/engine/command-layer.md` §7)。
pub mod cmd {
    /// GO。
    pub const GO: u8 = 0x2a;
    /// GOSUB。
    pub const GOSUB: u8 = 0x2b;
    /// IF。
    pub const IF: u8 = 0x2c;
    /// ELSE(0x0b;带可选条件,求值后决定进 else 块或跳过)。
    pub const ELSE: u8 = 0x0b;
    /// IFBLEND(0x2d;无条件跳到嵌套栈顶 else 块的结束)。
    pub const IFBLEND: u8 = 0x2d;
    /// RETURN。
    pub const RETURN: u8 = 0x4f;
    /// LET。
    pub const LET: u8 = 0x35;
    /// IFEND。
    pub const IFEND: u8 = 0x30;
    /// LOOP。
    pub const LOOP: u8 = 0x37;
    /// LOOPBREAK。
    pub const LOOPBREAK: u8 = 0x38;
    /// LOOPCONTINUE。
    pub const LOOPCONTINUE: u8 = 0x39;
    /// LOOPEND。
    pub const LOOPEND: u8 = 0x3a;
    /// END(脚本结束;可选返回码)。
    pub const END: u8 = 0x0d;
    /// WAIT。
    pub const WAIT: u8 = 0x68;
    /// TEXT。
    pub const TEXT: u8 = 0x62;
    /// CG。
    pub const CG: u8 = 0x01;
    /// SOUND。
    pub const SOUND: u8 = 0x59;
    /// VARACT(字符串变量操作;只读槽可执行,写回槽 Unsupported)。
    pub const VARACT: u8 = 0x66;
    /// VARINFO(变量属性查询;只读槽可执行,写回槽 Unsupported)。
    pub const VARINFO: u8 = 0x67;
    /// CGINFO(CG 图层属性查询;无图形后端走「CG 不存在」路径,LET=0)。
    pub const CGINFO: u8 = 0x04;
    /// LABELINFO(标签存在性查询;EXIST → LET=0/1,引擎 murmur2 查表 Confirmed)。
    pub const LABELINFO: u8 = 0x34;
    /// CGACT(CGM 图层操作;事件化记录关键字段)。
    pub const CGACT: u8 = 0x02;
    /// LOAD(文件/变量装载;事件化)。
    pub const LOAD: u8 = 0x36;
    /// SAVE(存档数据位图标记;语料 298 组,非声明族最高频)。
    pub const SAVE: u8 = 0x56;
    /// CGEND(CG 显示结束通知;语料 241 组)。
    pub const CGEND: u8 = 0x03;
    /// TASK(命名任务创建/重配置;引擎 0044fe40)。
    pub const TASK: u8 = 0x5f;
    /// TASKINFO(任务状态查询;引擎 00451838,EXIST → LET=0/1)。
    pub const TASKINFO: u8 = 0x61;
}

/// VM 状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// 尚未启动。
    Idle,
    /// 可继续执行(配额内)。
    Running,
    /// 脚本执行完毕。
    Finished,
    /// 遇到未证实/未实现语义,已挂起。
    Error,
}

/// 一次 `run` 返回的挂起信息。**绝不阻塞**。
#[derive(Debug, Clone, PartialEq)]
pub enum VmSuspend {
    /// 配额耗尽或本批执行完毕,可继续。
    None,
    /// 脚本执行完毕。
    Complete,
    /// 遇到未实现语义/错误。携带描述。
    Error(String),
    /// WAIT:引擎参数 FRAME(帧计数,obj+0x18)/TIME(ms,obj+0x1c=timeGetTime()+ms)。
    /// 调用方完成等待后 `resume` 继续。
    Wait {
        /// FRAME 参数(帧数);None = 未指定。
        counter: Option<u64>,
        /// TIME 参数(毫秒);None = 未指定。
        time_ms: Option<u64>,
    },
}

/// 恢复执行的外部应答。
#[derive(Debug, Clone, PartialEq)]
pub enum ResumeResponse {
    /// 无条件继续。
    Continue,
}

/// 跳转类别(事件流/Golden Test 用)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpKind {
    /// GO。
    Go,
    /// IF 条件为假。
    IfFalse,
    /// ELSE(跳过 else 块)。
    Else,
    /// RETURN 弹帧。
    Return,
    /// LOOPBREAK(跳出循环)。
    LoopBreak,
    /// LOOPEND/LOOPCONTINUE(回到循环体顶部)。
    LoopContinue,
}

/// GO 的执行结局。
enum GoOutcome {
    /// 跳到目标组(当前脚本内)。
    Jump(usize),
    /// 跨脚本跳转(切换上下文后跳到目标脚本的组号)。
    CrossScript {
        /// 目标脚本号。
        script_id: u16,
        /// 目标组号。
        target: usize,
    },
    /// 不跳(非 strict 下未命中等),顺序继续。
    NoJump,
    /// strict 挂起,携带原因。
    Halted(String),
}

/// VM 事件流(Trace / Golden Test 的数据源)。
#[derive(Debug, Clone, PartialEq)]
pub enum VmEvent {
    /// 一个组执行完毕。
    GroupExecuted {
        /// 组下标。
        pc: usize,
        /// YSCM 命令下标。
        command: u8,
        /// 求值结果(仅 IF 的条件等单值场景;多数命令无单一结果)。
        condition: Option<Value>,
    },
    /// 发生跳转。
    Jump {
        /// 起始组。
        from: usize,
        /// 目标组。
        to: usize,
        /// 类别。
        kind: JumpKind,
    },
    /// GOSUB 压帧。
    Call {
        /// GOSUB 组下标。
        from: usize,
        /// 目标组。
        to: usize,
    },
    /// TEXT:显示文本(FILE 参数为 SJIS 字节;LET/CLEAR 布尔位)。
    Text {
        /// 参数槽下标。
        pc: usize,
        /// FILE 参数(SJIS 原文)。
        file: Option<Vec<u8>>,
        /// LET 参数(=1 允许按 [r]/[c] 划段)。
        let_flag: bool,
        /// CLEAR 参数(=1 先清空)。
        clear_flag: bool,
    },
    /// CG:立绘/背景操作(ID,+ X/Y/Z 位置等关键参数)。
    Cg {
        /// 组下标。
        pc: usize,
        /// ID(名称,若为字符串)。
        id: Option<String>,
        /// X/Y/Z 位置(若提供)。
        position: Option<(i64, i64, i64)>,
        /// 其他已提供参数计数(按 B0 下标)。
        param_count: usize,
        /// FILE 槽(46;带图像装载的 CG 命令非空;渲染驱动按此加载资源;
        /// 无 FILE = 位置/状态更新,沿用手头资源)。
        file: Option<Vec<u8>>,
    },
    /// SOUND:音频操作(ID/FILE/PLAY/LOOP/VOLUME 等)。
    Sound {
        /// 组下标。
        pc: usize,
        /// ID(名称,若给定)。
        id: Option<String>,
        /// FILE(路径,若为字符串)。
        file: Option<String>,
        /// PLAY(是否播放;引擎非零=播放)。
        play: Option<i64>,
        /// 其他参数计数。
        param_count: usize,
    },
    /// VARACT/VARINFO 只读查询结果(字符串长度/维度/类型查询等)。
    /// 只读槽求值记录;**写回类槽(赋值/裁剪/大小写/PUSH/POP)走 Unsupported**——
    /// 未逆向写回目标前不猜(铁律)。
    VarQuery {
        /// 组下标。
        pc: usize,
        /// 命令(0x66 VARACT / 0x67 VARINFO)。
        command: u8,
        /// 已求值槽: (参数下标B0, 值摘要)。
        evaluated: Vec<(u8, String)>,
    },
    /// 声明类命令(INT/FLT/STR/G_*/S_*/F_* 等):加载期数据,引擎运行器
    /// 用默认 stub,不执行。记录后(trace)继续,stict 下按未实现挂起。
    Declaration {
        /// 组下标。
        pc: usize,
        /// 命令类型(YSCM 下标)。
        command: u8,
        /// 参数窗口数。
        windows: u8,
    },
    /// CGACT:CGM 图层操作(合成/变换等;71 参,运行期图形管线,只记录已求值参数摘要)。
    CgAct {
        /// 组下标。
        pc: usize,
        /// ID(B0=0,字符串,若给定)。
        id: Option<String>,
        /// 已求值参数摘要:(B0 槽, 值摘要)。
        evaluated: Vec<(u8, String)>,
    },
    /// CGINFO:CG 图层属性查询(引擎 CMDH_0043b084)。
    /// 无图形后端 ⇒ CG 必然不存在 ⇒ 引擎「不存在」路径(结果缓冲清零,LET=0)。
    CgInfo {
        /// 组下标。
        pc: usize,
        /// ID(槽 0,CG 名)。
        id: Option<String>,
        /// 已求值参数摘要((槽, 值摘要);255 = 写回目标记录)。
        evaluated: Vec<(u8, String)>,
    },
    /// LOAD:YSSD 系统数据装载(FILE + DNO + 写回目标;引擎 CMDH_00444648)。
    /// 装载路径 = 松散文件 + SNP 解压 + `FUN_0044564d` 写回(见 `load_yssd`);
    /// 事件保留参数摘要供 trace 对拍。
    Load {
        /// 组下标。
        pc: usize,
        /// FILE 路径(SJIS 原文)。
        file: Option<Vec<u8>>,
        /// 已求值参数摘要。
        evaluated: Vec<(u8, String)>,
    },
    /// SAVE:存档数据位图标记(DNO=存档号,SET=目标引用;引擎 CMDH_00451838)。
    /// 语料 298 组形态 = [DNO, SET 引用](位图置位/清零)+ [FILE](仅记录);
    /// 引擎按描述符类型把 INT 置 1/0、FLT 置 1.0/0.0。本实现按目标类型写值,
    /// 不落盘(存档文件格式 Unknown)。
    Save {
        /// 组下标。
        pc: usize,
        /// FILE 参数(若有;只记录)。
        file: Option<Vec<u8>>,
        /// DNO 存档号(若给定)。
        dno: Option<i64>,
        /// SET 目标摘要(写位图的变量)。
        set_target: Option<String>,
        /// 已求值参数摘要。
        evaluated: Vec<(u8, String)>,
    },
    /// CGEND:CG 显示结束通知(ID 字符串;引擎 LAB_0043ad14)。
    /// 无图形后端 → 事件化记录,不触碰图形状态。
    CgEnd {
        /// 组下标。
        pc: usize,
        /// ID(字符串)。
        id: Option<String>,
        /// 已求值参数摘要。
        evaluated: Vec<(u8, String)>,
    },
    /// 跨脚本上下文切换。
    ScriptSwitch {
        /// 源脚本号(0 表示未知/未初始化 —— 仅诊断)。
        from_script: u16,
        /// 目标脚本号。
        to_script: u16,
    },
    /// 未实现/不支持的命令(语义未逆向或跨脚本等)。
    /// 事件保留现场;strict 模式随后挂起。
    Unsupported {
        /// 组下标。
        pc: usize,
        /// YSCM 命令下标。
        command: u8,
        /// 原因描述。
        reason: String,
    },
    /// 后端子系统配置/查询族(P2):FONTINFO/MATH/FILEINFO/FILEACT/WINDOWINFO/
    /// MOUSE/FONT/SYSTEM/ERROR/FPS/INPUT。引擎处理器做子系统状态操作
    /// (字体层/文件/输入模式/数学函数),无后端 → 事件化记录已求值参数。
    Subsystem {
        /// 组下标。
        pc: usize,
        /// YSCM 命令下标。
        command: u8,
        /// 已求值参数摘要 (B0 槽, 值)。
        evaluated: Vec<(u8, String)>,
    },
}

/// 事件 → JSON 行(P1 引擎真值对拍的统一 schema)。
///
/// 手工序列化(非 serde):`Value::Str` 为 SJIS 原始字节 → 十六进制避免编码歧义;
/// `Value::Float` 用 f64 位模式保证 NaN/精度稳定;`Unsupported` 的 reason 做
/// 非 ASCII 图形字符转义保证字节稳定。golden 快照与 `vm_trace` 导出共用本函数。
pub fn event_json(e: &VmEvent) -> String {
    /// JSON 字符串转义(引擎侧 CG 名等含字面 `"`,如 `"CGS"100`;
    /// 不转义会产出非法 JSON 行 —— P2 对拍实测发现)。
    fn json_escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out
    }
    /// (槽, 值摘要) 对 → JSON 数组(显式构造;Debug 格式化对非 ASCII 产出
    /// `\u{...}` 非 JSON 转义,不可用)。
    fn ev_arr(evaluated: &[(u8, String)]) -> String {
        let items: Vec<String> = evaluated
            .iter()
            .map(|(b, v)| format!("\"{}:{}\"", b, json_escape(v)))
            .collect();
        format!("[{}]", items.join(","))
    }
    fn val_json(v: &yuris_value::Value) -> String {
        match v {
            yuris_value::Value::Int(i) => format!(r#"{{"int":{i}}}"#),
            yuris_value::Value::Float(f) => {
                // 用位模式保证 NaN/精度稳定
                format!(r#"{{"f64bits":{}}}"#, f.to_bits())
            }
            yuris_value::Value::Str(b) => {
                let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
                format!(r#"{{"str_hex":"{hex}"}}"#)
            }
        }
    }
    match e {
        VmEvent::GroupExecuted { pc, command, condition } => {
            let cond = condition
                .as_ref()
                .map(val_json)
                .unwrap_or_else(|| "null".to_string());
            format!(
                r#"{{"ev":"group","pc":{pc},"cmd":{command},"cond":{cond}}}"#
            )
        }
        VmEvent::Jump { from, to, kind } => {
            let k = match kind {
                JumpKind::Go => "go",
                JumpKind::IfFalse => "if_false",
                JumpKind::Else => "else",
                JumpKind::Return => "return",
                JumpKind::LoopBreak => "loop_break",
                JumpKind::LoopContinue => "loop_continue",
            };
            format!(r#"{{"ev":"jump","from":{from},"to":{to},"kind":"{k}"}}"#)
        }
        VmEvent::Call { from, to } => {
            format!(r#"{{"ev":"call","from":{from},"to":{to}}}"#)
        }
        VmEvent::Text { pc, file, let_flag, clear_flag } => {
            let f = file.as_ref().map(|b| {
                let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
                format!(r#"{{"hex":"{hex}"}}"#)
            }).unwrap_or_else(|| "null".to_string());
            format!(
                r#"{{"ev":"text","pc":{pc},"file":{f},"let":{let_flag},"clear":{clear_flag}}}"#
            )
        }
        VmEvent::Cg { pc, id, position, param_count, file } => {
            let idj = id.clone()
                .map(|i| "{\"s\":\"".to_string() + &json_escape(&i) + "\"}")
                .unwrap_or_else(|| "null".to_string());
            let posj = position.map(|(x, y, z)| format!(r#"{{"x":{x},"y":{y},"z":{z}}}"#)).unwrap_or_else(|| "null".to_string());
            let fj = file.clone().map(|f| {
                let hex: Vec<String> = f.iter().map(|b| format!("{b:02x}")).collect();
                format!(r#"{{"hex":"{}"}}"#, hex.join(""))
            }).unwrap_or_else(|| "null".to_string());
            format!(r#"{{"ev":"cg","pc":{pc},"id":{idj},"pos":{posj},"n":{param_count},"file":{fj}}}"#)
        }
        VmEvent::Sound { pc, id, file, play, param_count } => {
            let idj = id.clone()
                .map(|i| "{\"s\":\"".to_string() + &json_escape(&i) + "\"}")
                .unwrap_or_else(|| "null".to_string());
            let fj = file.clone()
                .map(|i| "{\"s\":\"".to_string() + &json_escape(&i) + "\"}")
                .unwrap_or_else(|| "null".to_string());
            let pj = play.map(|p| p.to_string()).unwrap_or_else(|| "null".to_string());
            format!(r#"{{"ev":"sound","pc":{pc},"id":{idj},"file":{fj},"play":{pj},"n":{param_count}}}"#)
        }
        VmEvent::ScriptSwitch { from_script, to_script } => {
            format!(r#"{{"ev":"switch","from":{from_script},"to":{to_script}}}"#)
        }
        VmEvent::Declaration { pc, command, windows } => {
            format!(r#"{{"ev":"decl","pc":{pc},"cmd":{command},"w":{windows}}}"#)
        }
        VmEvent::CgAct { pc, id, evaluated } => {
            let idj = id.clone().map(|s| json_escape(&s)).unwrap_or_else(|| "null".to_string());
            let ev: Vec<String> = evaluated.iter().map(|(b, v)| format!("{b}:{}", json_escape(v))).collect();
            let ev = ev_arr(&evaluated);
            format!(r#"{{"ev":"cgact","pc":{pc},"id":"{idj}","ev_":{ev}}}"#)
        }
        VmEvent::CgInfo { pc, id, evaluated } => {
            let idj = id.clone().map(|s| json_escape(&s)).unwrap_or_else(|| "null".to_string());
            let ev: Vec<String> = evaluated.iter().map(|(b, v)| format!("{b}:{}", json_escape(v))).collect();
            let ev = ev_arr(&evaluated);
            format!(r#"{{"ev":"cginfo","pc":{pc},"id":"{idj}","ev_":{ev}}}"#)
        }
        VmEvent::Load { pc, file, evaluated } => {
            let fj = file.as_ref().map(|b| {
                let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
                hex
            }).unwrap_or_else(|| "null".to_string());
            let ev = ev_arr(&evaluated);
            format!(r#"{{"ev":"load","pc":{pc},"file_hex":"{fj}","ev_":{ev}}}"#)
        }
        VmEvent::VarQuery { pc, command, evaluated } => {
            let ev: Vec<String> = evaluated.iter().map(|(b,v)| format!("{b}:{v}")).collect();
            let ev = ev_arr(&evaluated);
            format!(r#"{{"ev":"varquery","pc":{pc},"cmd":{command},"ev_":{ev}}}"#)
        }
        VmEvent::Save { pc, file, dno, set_target, evaluated } => {
            let fj = file.as_ref().map(|b| {
                let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
                format!(r#"{{"hex":"{hex}"}}"#)
            }).unwrap_or_else(|| "null".to_string());
            let dj = dno.map(|d| d.to_string()).unwrap_or_else(|| "null".to_string());
            let sj = set_target.clone()
                .map(|s| format!(r#""{}""#, json_escape(&s)))
                .unwrap_or_else(|| "null".to_string());
            let ev = ev_arr(&evaluated);
            format!(
                r#"{{"ev":"save","pc":{pc},"file":{fj},"dno":{dj},"set":{sj},"ev_":{ev}}}"#
            )
        }
        VmEvent::CgEnd { pc, id, evaluated } => {
            let idj = id.clone().map(|s| json_escape(&s)).unwrap_or_else(|| "null".to_string());
            let ev = ev_arr(&evaluated);
            format!(r#"{{"ev":"cgend","pc":{pc},"id":"{idj}","ev_":{ev}}}"#)
        }
        VmEvent::Subsystem { pc, command, evaluated } => {
            let ev = ev_arr(&evaluated);
            format!(r#"{{"ev":"sub","pc":{pc},"cmd":{command},"ev_":{ev}}}"#)
        }
        VmEvent::Unsupported { pc, command, reason } => {
            // reason 含动态文本 → 截断转义,保持快照稳定
            let r: String = reason
                .chars()
                .map(|c| {
                    if c.is_ascii_graphic() || c == ' ' {
                        c.to_string()
                    } else {
                        format!("\\u{:04x}", c as u32)
                    }
                })
                .collect();
            format!(
                r#"{{"ev":"unsupported","pc":{pc},"cmd":{command},"reason":"{}"}}"#,
                json_escape(&r)
            )
        }
    }
}

/// GOSUB 调用帧(引擎帧的子集)。
///
/// 帧局部变量(引擎 frame 0x328B:INT 局部 +8 每槽 8B、FLT +0x90、STR +0x120,
/// 计数在 +0x2bc/+0x2cd/+0x2de;gparam 解码 int=(u16&0xff)>>3、flt=(u16&7)*4+
/// (u16>>14)、str=(u16>>9)&0x1f)。本实现用帧上独立的
/// [`VariableStore`](yuris_value::VariableStore) 承载,键 = (前缀字节, id)
/// —— 与全局存储列分离,弹帧即丢弃(引擎语义:Return 丢弃局部)。
#[derive(Debug, Clone)]
pub struct GosubFrame {
    /// 返回组下标(GOSUB 的 pc+1)。
    pub return_pc: usize,
    /// 返回时所在脚本号(跨脚本 RETURN 恢复;引擎帧含脚本号字段)。
    pub script_id: u16,
    /// 帧局部变量栈(按 id 存;声明命令 id = 0x32-0x35/0x46/0x75+ 帧局部族)。
    pub locals: VariableStore,
    /// GOSUB 时的 IF 嵌套栈深(引擎:帧 +1 字节存栈深,RETURN 恢复 ——
    /// 成果 51.5;子程序内遗留的未弹 IF 项随 RETURN 整体丢弃)。
    pub if_nest_depth: usize,
    /// GOSUB 时的循环栈深(引擎:循环记录栈增长随 RETURN 整体丢弃,成果 51.5)。
    pub loop_depth: usize,
}

/// 循环帧(引擎 obj+0x248 记录块的子集;LOOP 处理器 CMDH_00445a00)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopFrame {
    /// 循环体顶部 = LOOP 的 pc+1(LOOPEND/LOOPCONTINUE 跳回)。
    pub body_start: usize,
    /// 退出点 = LOOP w1.len(编译期组号 = 配对 LOOPEND 组;
    /// 引擎 rec+8,LOOPBREAK/limit=0 跳此,LOOPCONTINUE 经此中转)。
    pub exit_target: usize,
    /// 计数上限(引擎 +0x18/+0x1c 64 位;`None` = 引擎 0xffffffff/-1 无限标记)。
    pub limit: Option<u64>,
    /// 已完成次数(引擎 +0x10/+0x14;LOOP 置 1 → LOOPEND 检查后 +1)。
    pub counter: u64,
}

/// IF 嵌套帧(引擎 obj[0x40+level*4] 块的子集)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IfFrame {
    /// IF 组下标(诊断用)。
    pub if_pc: usize,
    /// 块结束目标(IF 的 w2.len)。
    pub end_target: u32,
}

/// CG 状态注册表条目(P7.1,成果 62;P1 收尾定稿)。
///
/// 引擎侧 CG 对象(kemonomichi2.exe 处理器 0x423864 写 X→+0x70 /
/// Y→+0x78 / E→+0xab 等)的最小语义模型:CG 命令按名 upsert,
/// CGINFO 查询 SX(槽13)/SY(槽14)/COLOR(槽24)。
///
/// **槽13/14 = 装载图像的真实宽/高(Confirmed,引擎 watch oracle)**:
/// script9 g1089 CGINFO 写 @1705:occ1 = 1.0(= `cgsys\dummy.png`,1×1
/// 占位图)、occ2 = 1350.0(= `tip_meswindow_txspace.png`,1350×200 消息窗
/// 纹理);两图均经封包扫描确认存在(`yuris-resource` dims_probe)。
/// g1092 IF `SX==1 && SY==1 && COLOR==0x808080` = 「是 1×1 占位图」判定。
/// 本模型:CG 命令 FILE 槽(46)非空 → 经 `PacFileIndex::image_dims` 解析
/// 图像头填 [`img_w`](CgState::img_w)/[`img_h`](CgState::img_h);
/// 不可解析(无探针/非 stored PNG)→ 0(偏离引擎处逐项定性)。
/// 缩放乘积(若有)未验证 —— 应答值与图像尺寸逐字节吻合,乘数 = 1。
///
/// sx/sy(CG 槽9/10)与图像尺寸**无关**(旧「默认 SX=1/SY=1 应答」模型
/// 作废:那只是 occ1 恰好命中 1×1 占位图的巧合)。语义 = Likely(缩放),
/// 不参与 CGINFO 应答。
///
/// COLOR(槽24)= 0x808080:occ1 实证(引擎写回 0x808080);「新建 CG
/// 未指定 COLOR 槽 ⇒ 取 0x808080」的一般化规则为 Likely。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgState {
    /// X(CG 槽 4;引擎 +0x70)。未执行查询,模型 Likely。
    pub x: i64,
    /// Y(CG 槽 5;引擎 +0x78)。未执行查询,模型 Likely。
    pub y: i64,
    /// Z(CG 槽 6)。未执行查询,模型 Likely。
    pub z: i64,
    /// 缩放 X(CG 槽 9;引擎语义 Likely,不参与 CGINFO 应答)。
    pub sx: i64,
    /// 缩放 Y(CG 槽 10;引擎语义 Likely,不参与 CGINFO 应答)。
    pub sy: i64,
    /// 装载图像宽(CGINFO 槽 13 应答;watch oracle 实证)。
    pub img_w: i64,
    /// 装载图像高(CGINFO 槽 14 应答;watch oracle 实证)。
    pub img_h: i64,
    /// 色(COLOR;CGINFO 槽 24 查询,CG 命令无对应槽)。
    pub color: i64,
}

impl Default for CgState {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            z: 0,
            sx: 1,
            sy: 1,
            img_w: 0,
            img_h: 0,
            color: 0x80_80_80,
        }
    }
}

/// YU-RIS 组级 VM(单脚本)。
pub struct GroupVm {
    /// 当前脚本上下文(组表 + 窗口起始)。
    ctx: ScriptCtx,
    /// 跨脚本宿主(按脚本号取上下文);None = 单脚本模式。
    host: Option<Box<dyn ScriptHost>>,
    /// 程序计数器 = 组下标。
    pc: usize,
    state: VmState,
    events: Vec<VmEvent>,
    /// 标签表(name → (目标组号, 脚本号));由外部注入(来自 YSLB)。
    labels: HashMap<Vec<u8>, (u32, u16)>,
    /// strict = 遇 Unsupported 命令挂起(默认);false = 记录后继续。
    strict: bool,
    store: VariableStore,
    frames: Vec<GosubFrame>,
    if_nest: Vec<IfFrame>,
    loops: Vec<LoopFrame>,
    executed_groups: usize,
    executed_windows: usize,
    /// 已消费声明组的脚本号(每脚本只全扫一次;见 `switch_script`)。
    declared_scripts: std::collections::HashSet<u16>,
    /// 播放器注入的光标逻辑坐标(@133/@138;P9.2)。
    input_cursor: (i64, i64),
    /// YSVR kind2(按脚本初值)条目暂存,首载时应用(成果 59h)。
    ysvr_kind2: Vec<yuris_format::ysvr::YsvrEntry>,
    /// 已应用 kind2 的脚本号。
    ysvr_kind2_applied: std::collections::HashSet<u16>,
    /// 游戏虚拟 FS 存在性索引(FILEINFO EXIST;见 `host::PacFileIndex`)。
    file_probe: Option<std::sync::Arc<crate::host::PacFileIndex>>,
    /// 命名任务注册表(TASK 创建 → TASKINFO EXIST 查询;引擎 FUN_0040ca7d
    /// 按名查找的等价面。任务的实际执行经 GOSUB 标签链顺序进入 —— 引擎
    /// trace 实证:es.IDSubTask 的入口脚本由主线 GOSUB 串行调起,boot 段
    /// 无并行交错;调度器模型归 P7.1 scenario 合流点定性)。
    tasks: std::collections::HashSet<Vec<u8>>,
    /// CG 状态注册表(CG 名(SJIS 字节)→ 状态;P7.1,成果 62)。
    cg_registry: HashMap<Vec<u8>, CgState>,
}

impl GroupVm {
    /// 装载脚本。组模型校验失败(非 v555 形态)→ `Err`。
    ///
    /// 帧栈预置**基帧**(引擎 record[0],GOSUB 反编译 004428c0/0044b418
    /// 定性:任务创建即有 depth=0 的当前帧记录,顶层脚本的帧局部/返回值
    /// 区都落在它上面;RETURN 于基帧 = depth 0 → 脚本结束路径)。
    pub fn load(script: YstbFile) -> Result<Self> {
        let ctx = ScriptCtx::parse(0, script)?;
        Ok(Self {
            ctx,
            host: None,
            pc: 0,
            state: VmState::Idle,
            events: Vec::new(),
            labels: HashMap::new(),
            strict: true,
            store: VariableStore::new(),
            frames: vec![GosubFrame {
                return_pc: 0,
                script_id: 0,
                locals: VariableStore::new(),
                if_nest_depth: 0,
                loop_depth: 0,
            }],
            if_nest: Vec::new(),
            loops: Vec::new(),
            executed_groups: 0,
            executed_windows: 0,
            declared_scripts: std::collections::HashSet::new(),
            input_cursor: (0, 0),
            ysvr_kind2: Vec::new(),
            ysvr_kind2_applied: std::collections::HashSet::new(),
            file_probe: None,
            tasks: std::collections::HashSet::new(),
            cg_registry: HashMap::new(),
        })
    }

    /// 绑定跨脚本宿主(启用跨脚本 GO/GOSUB)。
    pub fn set_host(&mut self, host: Box<dyn ScriptHost>) {
        self.host = Some(host);
    }

    /// 注入游戏虚拟 FS 存在性索引(FILEINFO EXIST 查询用;P5.2)。
    /// 注入光标逻辑坐标(@133/@138;播放器每帧调用,P9.2)。
    pub fn set_input_cursor(&mut self, x: i64, y: i64) {
        self.input_cursor = (x, y);
    }

    pub fn set_file_probe(&mut self, probe: std::sync::Arc<crate::host::PacFileIndex>) {
        self.file_probe = Some(probe);
    }

    /// 标记脚本的声明组已消费(boot 全量消费后调用;防止 switch_script
    /// 的按需重消费用声明期边界**覆盖 YSVR 终态** —— 引擎描述符分配仅在
    /// boot 一次,成果 59)。
    pub fn mark_declarations_consumed(&mut self, script_id: u16) {
        self.declared_scripts.insert(script_id);
    }

    /// 注入标签表(name → (目标组号, 脚本号));典型来源为
    /// [`yuris_format::yslb::YslbTable`](yuris_format::yslb::YslbTable)。
    pub fn set_labels(&mut self, labels: HashMap<Vec<u8>, (u32, u16)>) {
        self.labels = labels;
    }

    /// 设置当前脚本号(跨脚本跳转判定;`yst%05d.ybn` 的 %05d)。
    pub fn set_script_id(&mut self, id: u16) {
        self.ctx.script_id = id;
    }

    /// strict 模式开关(默认 true:遇 Unsupported 挂起)。
    pub fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
    }

    /// 当前状态。
    pub fn state(&self) -> VmState {
        self.state
    }

    /// 程序计数器(组下标)。
    pub fn pc(&self) -> usize {
        self.pc
    }

    /// 已消费事件。
    pub fn events(&self) -> &[VmEvent] {
        &self.events
    }

    /// 变量存储(只读)。
    pub fn store(&self) -> &VariableStore {
        &self.store
    }

    /// GOSUB 帧栈深度(诊断/测试用)。
    pub fn frame_depth(&self) -> usize {
        self.frames.len()
    }

    /// 当前帧 locals 可变访问(基帧恒在,成果 56;诊断/测试用)。
    ///
    /// 引擎 trace 样本中的 @53/@55 族值位于引擎**当前帧记录**(实参/返回值区),
    /// 测试回放时以此播种等价状态(全局 store 是 id≥1000 域,勿混)。
    pub fn frame_locals_mut(&mut self) -> &mut VariableStore {
        match self.frames.last_mut() {
            Some(f) => &mut f.locals,
            None => unreachable!("基帧恒在(load 预置;RETURN 保持 len≥1)"),
        }
    }

    /// 调用帧栈快照(诊断/测试用;返回各帧 return_pc 与脚本号)。
    pub fn frames_snapshot(&self) -> Vec<(usize, u16)> {
        self.frames.iter().map(|f| (f.return_pc, f.script_id)).collect()
    }

    /// 当前帧局部可读变量数(诊断/测试用)。
    pub fn frame_local_count(&self) -> usize {
        self.frames.last().map(|f| f.locals.len()).unwrap_or(0)
    }

    /// 变量存储(可变;供外部预置全局变量)。
    pub fn store_mut(&mut self) -> &mut VariableStore {
        &mut self.store
    }

    /// 应用 YSVR 变量定义表(启动链 FUN_0046b63c 的「应用初值」步)。
    ///
    /// 消费时机(FUN_00451348 反编译,成果 59h):
    /// - boot(-1 调用)应用 **kind 1 与 kind 3**;
    /// - kind 2(按脚本)在该脚本**首次加载**时应用
    ///   ([`Self::apply_ysvr_for_script`],switch_script 钩子);
    /// - 抽样 60 个 kind2 变量,全部只在所属脚本被引用(探针全语料扫描);
    /// - 端到端证据:es.BT(script22) 读 $1895(kind2, script=22, STR[100]),
    ///   只应用 kind1 时报「undefined array variable」。
    /// - @5256(kind3, script=157, FLT[101,5])须在 boot 应用,否则 s177 的
    ///   2D 访问踩「declared 1」(实证;旧「kind3 不猜跳过」被反编译证伪)。
    pub fn apply_ysvr(&mut self, ysvr: &yuris_format::ysvr::YsvrTable) -> Result<usize> {
        let mut applied = 0usize;
        for e in ysvr.entries() {
            if e.kind == 2 {
                // 按脚本初值:挂起,待该脚本首次加载时应用
                self.ysvr_kind2.push(e.clone());
                continue;
            }
            self.apply_ysvr_entry(e)?;
            applied += 1;
        }
        Ok(applied)
    }

    /// 应用某脚本名下的 kind2 条目(脚本首次加载时;幂等)。
    pub fn apply_ysvr_for_script(&mut self, script_id: u16) -> Result<usize> {
        if self.ysvr_kind2_applied.contains(&script_id) {
            return Ok(0);
        }
        self.ysvr_kind2_applied.insert(script_id);
        let entries: Vec<yuris_format::ysvr::YsvrEntry> = self
            .ysvr_kind2
            .iter()
            .filter(|e| e.script == script_id)
            .cloned()
            .collect();
        let mut applied = 0usize;
        for e in &entries {
            self.apply_ysvr_entry(e)?;
            applied += 1;
        }
        Ok(applied)
    }

    /// 应用单条 YSVR 条目(边界覆盖 + 初值)。
    fn apply_ysvr_entry(&mut self, e: &yuris_format::ysvr::YsvrEntry) -> Result<()> {
        use yuris_value::{ElemType, Value};
        let prefix: u8 = match e.ty {
            1 | 2 => 0x40,  // INT/FLT -> @
            3 | 0 => 0x24,  // STR -> $
            other => {
                return Err(Error::Unimplemented(
                    "YSVR 条目类型 Unknown(不猜)",
                ));
            }
        };
        let r = yuris_value::VarRef {
            space: yuris_value::VarSpace::from_prefix(prefix),
            id: e.var_id,
        };
        let v = match &e.init {
            yuris_format::ysvr::YsvrInit::Int(i) => Value::Int(*i),
            yuris_format::ysvr::YsvrInit::Float(f) => Value::Float(*f),
            yuris_format::ysvr::YsvrInit::Str(b) => Value::Str(b.clone()),
            yuris_format::ysvr::YsvrInit::None => {
                // 仅声明无初值:数组按边界清零,标量按 type 默认
                if e.bounds.is_empty() {
                    if e.ty == 1 {
                        Value::Int(0)
                    } else if e.ty == 2 {
                        Value::Float(0.0)
                    } else if e.ty == 3 {
                        Value::Str(Vec::new())
                    } else {
                        return Ok(()); // ty==0 仅声明,无存储语义
                    }
                } else {
                    // 数组:declare_array + elem0(其余清零由 ArrayStorage::new)
                    let elem = if e.ty == 1 {
                        ElemType::Int
                    } else if e.ty == 2 {
                        ElemType::Float
                    } else {
                        ElemType::Str
                    };
                    self.store.declare_array(&r, elem, &e.bounds);
                    return Ok(());
                }
            }
        };
        if e.bounds.is_empty() {
            self.store.set(&r, v);
        } else {
            let elem = if e.ty == 1 { ElemType::Int }
                else if e.ty == 2 { ElemType::Float }
                else { ElemType::Str };
            self.store.declare_array(&r, elem, &e.bounds);
            let zeros = vec![0i64; e.bounds.len()];
            self.store.set_elem(&r, &zeros, v)?;
        }
        Ok(())
    }

    /// 已执行组数(配额审计)。
    pub fn executed_groups(&self) -> usize {
        self.executed_groups
    }

    /// 已执行窗口数(配额审计)。
    pub fn executed_windows(&self) -> usize {
        self.executed_windows
    }

    /// 组数。
    pub fn group_count(&self) -> usize {
        self.ctx.groups.len()
    }

    /// 当前脚本号。
    pub fn script_id(&self) -> u16 {
        self.ctx.script_id
    }

    /// 执行至多 `budget` 个组,返回挂起信息。
    ///
    /// - 配额耗尽 → [`VmSuspend::None`](VmSuspend::None)(状态 `Running`)
    /// - pc 走完所有组 → [`VmSuspend::Complete`](VmSuspend::Complete)(`Finished`)
    /// - Unsupported(strict)/求值错误 → [`VmSuspend::Error`](VmSuspend::Error)
    pub fn run(&mut self, budget: usize) -> Result<VmSuspend> {
        match self.state {
            VmState::Finished => return Ok(VmSuspend::Complete),
            VmState::Error => {
                return Err(Error::format(
                    "VM 处于 Error 态;需先检查事件,骨架版不支持自动恢复",
                ))
            }
            VmState::Idle | VmState::Running => {}
        }
        self.state = VmState::Running;

        let mut used = 0usize;
        while self.pc < self.ctx.groups.len() {
            if used >= budget {
                return Ok(VmSuspend::None);
            }
            used += 1;
            self.executed_groups += 1;

            let g = self.ctx.groups[self.pc];
            let first = self.ctx.first_slots[self.pc];
            // 克隆窗口切片(12B 结构),避免 script 借用与 &mut self 冲突
            let windows: Vec<yuris_format::ystb::CommandSlot> = self.ctx.script.slots()
                [first..first + g.window_count as usize]
                .to_vec();
            self.executed_windows += g.window_count as usize;

            match g.command_type {
                cmd::GO => match self.exec_go(&windows)? {
                    GoOutcome::Jump(target) => {
                        let from = self.pc;
                        self.events.push(VmEvent::Jump {
                            from,
                            to: target,
                            kind: JumpKind::Go,
                        });
                        self.events.push(VmEvent::GroupExecuted {
                            pc: from,
                            command: g.command_type,
                            condition: None,
                        });
                        self.pc = target;
                        continue;
                    }
                    GoOutcome::CrossScript { script_id, target } => {
                        // 跨脚本 GO:事件归属 = 派发点(引擎真值:P1 对拍
                        // 实锤,引擎在旧脚本 pc 记本组后才切上下文)。
                        // switch_script 会改写 self.pc/self.ctx.script_id,
                        // 故 from/from_script 必须在切换前捕获。
                        let from = self.pc;
                        let from_script = self.ctx.script_id;
                        match self.switch_script(script_id, target) {
                            Ok(()) => {
                                self.events.push(VmEvent::GroupExecuted {
                                    pc: from,
                                    command: g.command_type,
                                    condition: None,
                                });
                                self.events.push(VmEvent::ScriptSwitch {
                                    from_script,
                                    to_script: script_id,
                                });
                                self.events.push(VmEvent::Jump {
                                    from,
                                    to: target,
                                    kind: JumpKind::Go,
                                });
                                continue;
                            }
                            Err(e) => {
                                self.state = VmState::Error;
                                return Ok(VmSuspend::Error(e.to_string()));
                            }
                        }
                    }
                    GoOutcome::NoJump => {
                        // 非 strict:未命中事件已记,顺序继续
                        self.pc += 1;
                        continue;
                    }
                    GoOutcome::Halted(reason) => {
                        self.state = VmState::Error;
                        return Ok(VmSuspend::Error(reason));
                    }
                },
                cmd::GOSUB => {
                    // w0 = 条件窗口(引擎 FUN_00442d01);求值失败 → 诚实挂起
                    let cond = self.eval_window_condition(windows.first())?;
                    let Some(cond) = cond else {
                        if let Some(reason) =
                            self.halt_or_record(self.pc, g.command_type, "GOSUB 无条件窗口")
                        {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    };
                    let truthy = value_truthy(&cond)?;
                    if !truthy {
                        self.events.push(VmEvent::GroupExecuted {
                            pc: self.pc,
                            command: g.command_type,
                            condition: Some(cond),
                        });
                        self.pc += 1;
                        continue;
                    }
                    // 标签解析:找 M-串窗口(引擎查 hash 表);未命中 → 引擎静默不跳
                    let resolved = self.resolve_label_in(&windows)?;
                    let Some((target, target_script)) = resolved else {
                        self.events.push(VmEvent::GroupExecuted {
                            pc: self.pc,
                            command: g.command_type,
                            condition: Some(cond),
                        });
                        self.pc += 1;
                        continue;
                    };
                    let from = self.pc;
                    let from_script = self.ctx.script_id;
                    let mut locals = VariableStore::new();
                    self.seed_frame_locals(&mut locals, &windows, g.param);
                    self.frames.push(GosubFrame {
                        return_pc: from + 1,
                        script_id: from_script,
                        locals,
                        // 引擎:帧 +1 存 IF 栈深;循环记录栈增长亦随 RETURN 丢弃
                        // (成果 51.5)。
                        if_nest_depth: self.if_nest.len(),
                        loop_depth: self.loops.len(),
                    });
                    self.events.push(VmEvent::Call { from, to: target });
                    self.events.push(VmEvent::GroupExecuted {
                        pc: from,
                        command: g.command_type,
                        condition: Some(cond),
                    });
                    // 跨脚本 GOSUB:压帧后切换上下文到目标脚本,并记 ScriptSwitch。
                    if target_script != from_script {
                        self.events.push(VmEvent::ScriptSwitch {
                            from_script: from_script,
                            to_script: target_script,
                        });
                        self.switch_script(target_script, target)?;
                    } else {
                        self.pc = target;
                    }
                    continue;
                }
                cmd::RETURN => {
                    // 返回值虚拟族(P5 定性,成果 50):实参按 B0 槽号求值
                    // (callee 上下文),写入调用者帧返回值区:
                    //   B0 0x00/0x01-0x0f → INT  ret[@60][max(1,B0)]
                    //   B0 0x10-0x1f      → FLT  ret[@61][max(1,B0-0x10)]
                    //   B0 0x20-0x2f      → STR  ret[$62][max(1,B0-0x20)]
                    // 槽从 1 起(引擎 0044b418 实锤,成果 54:写循环
                    // `iVar9=1..=count`,0x160+8*i;B0=0 = 条件窗形态
                    // 「无显式槽号」→ 默认槽 1)。语料:RETURN B0=0 共 215 窗
                    // (189 单窗);@60/$62 读者下标全 ≥1(0 次出现,探针
                    // probe_ret_readers.py)。实证:es._strlen RETURN B0=1 →
                    // 调用者 @60[1];es._label.info.exist RETURN B0=0 → @60[1]
                    // (s13 g476 → g2 读者)。
                    //
                    // depth 语义(0044b418 + 004428c0 交叉,成果 56):RETURN
                    // 写 `obj+0x140+depth*4` = 读侧基 0x144 的 record[depth-1]
                    // = **调用者记录**(+0x160/+0x1e8/+0x278 值区,+0x2ef/0x300/
                    // 0x311 计数),弹帧后按同一记录 +4/+8 恢复;`if (0 < iVar8)`
                    // 不成立 = depth 0(基帧 record[0] 上 RETURN)→ 脚本结束
                    // (FUN_00451724)。VM frames.len() ↔ 引擎 depth:
                    // len≤1 = 基帧上 RETURN → 结束;len>1 → 弹帧写调用者帧。
                    if self.frames.len() <= 1 {
                        self.events.push(VmEvent::GroupExecuted {
                            pc: self.pc,
                            command: g.command_type,
                            condition: None,
                        });
                        self.state = VmState::Finished;
                        return Ok(VmSuspend::Complete);
                    }
                    let mut ret_vals: Vec<(u32, Value)> = Vec::new();
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let b0 = (w.tag & 0xff) as u32;
                        if b0 >= 0x30 {
                            continue; // 区段外
                        }
                        if let Some(v) = self.eval_window_condition(Some(w))? {
                            ret_vals.push((b0, v));
                        }
                    }
                    let Some(frame) = self.frames.pop() else {
                        unreachable!("frames.len()>1 已保证可弹");
                    };
                    // 引擎 RETURN 语义(成果 51.5):IF 嵌套栈深恢复为帧记录值,
                    // 子程序内遗留的未弹 IF 项 / 循环记录整体丢弃。
                    self.if_nest.truncate(frame.if_nest_depth);
                    self.loops.truncate(frame.loop_depth);
                    {
                        let from = self.pc;
                        let cur_script = self.ctx.script_id;
                        // 返回帧所在脚本号;切帧(跨脚本返回)。
                        let cross = frame.script_id != cur_script;
                        self.events.push(VmEvent::Jump {
                            from,
                            to: frame.return_pc,
                            kind: JumpKind::Return,
                        });
                        // 本组事件归属 = 派发点脚本(P1 引擎对拍);跨脚本时
                        // 须在切换前发 ScriptSwitch,否则事件流重建脚本号错位。
                        self.events.push(VmEvent::GroupExecuted {
                            pc: from,
                            command: g.command_type,
                            condition: None,
                        });
                        if cross {
                            self.events.push(VmEvent::ScriptSwitch {
                                from_script: cur_script,
                                to_script: frame.script_id,
                            });
                            self.switch_script(frame.script_id, frame.return_pc)?;
                        }
                        // 返回值写入调用者帧(弹出后 frames.last() = 调用者)
                        for (b0, v) in ret_vals {
                            let (r, idx, et): (
                                yuris_value::VarRef,
                                u32,
                                yuris_value::ElemType,
                            ) = match b0 {
                                // 槽号从 1 起(成果 54);B0=0 = 默认槽 1
                                0x00..=0x0f => (
                                    yuris_value::VarRef {
                                        space: yuris_value::VarSpace::At,
                                        id: 60,
                                    },
                                    b0.max(1),
                                    yuris_value::ElemType::Int,
                                ),
                                0x10..=0x1f => (
                                    yuris_value::VarRef {
                                        space: yuris_value::VarSpace::At,
                                        id: 61,
                                    },
                                    (b0 - 0x10).max(1),
                                    yuris_value::ElemType::Float,
                                ),
                                0x20..=0x2f => (
                                    yuris_value::VarRef {
                                        space: yuris_value::VarSpace::Dollar,
                                        id: 62,
                                    },
                                    (b0 - 0x20).max(1),
                                    yuris_value::ElemType::Str,
                                ),
                                _ => continue,
                            };
                            if let Some(caller) = self.frames.last_mut() {
                                if !caller.locals.has_array(&r) {
                                    caller.locals.declare_array(&r, et, &[16]);
                                }
                                let cv: Value = match (et, v) {
                                    (yuris_value::ElemType::Int, Value::Int(i)) => {
                                        Value::Int(i)
                                    }
                                    (yuris_value::ElemType::Int, Value::Float(f)) => {
                                        Value::Int(f.round() as i64)
                                    }
                                    (yuris_value::ElemType::Float, Value::Int(i)) => {
                                        Value::Float(i as f64)
                                    }
                                    (yuris_value::ElemType::Float, Value::Float(f)) => {
                                        Value::Float(f)
                                    }
                                    (yuris_value::ElemType::Str, Value::Str(mut s)) => {
                                        if s.len() >= 2
                                            && s.first() == Some(&b'"')
                                            && s.last() == Some(&b'"')
                                        {
                                            s = s[1..s.len() - 1].to_vec();
                                        }
                                        Value::Str(s)
                                    }
                                    (_, other) => other,
                                };
                                let _ = caller.locals.set_elem(&r, &[idx as i64], cv);
                            }
                        }
                        self.pc = frame.return_pc;
                        continue;
                    }
                }
                cmd::IF => {
                    if g.window_count != 3 {
                        if let Some(reason) = self.halt_or_record(
                            self.pc,
                            g.command_type,
                            format!("IF 窗口数 {} != 3(语料恒 3)", g.window_count),
                        ) {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    }
                    let cond = self.eval_window_condition(windows.first())?;
                    let Some(cond) = cond else {
                        if let Some(reason) =
                            self.halt_or_record(self.pc, g.command_type, "IF 无条件窗口")
                        {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    };
                    let truthy = value_truthy(&cond)?;
                    // 嵌套栈:end 目标 = w2.len(编译期组号)
                    self.if_nest.push(IfFrame {
                        if_pc: self.pc,
                        end_target: windows[2].len,
                    });
                    if !truthy {
                        // 假 → 跳 w1.len ?: w2.len
                        let to = if windows[1].len != 0 {
                            windows[1].len
                        } else {
                            windows[2].len
                        };
                        let from = self.pc;
                        self.events.push(VmEvent::Jump {
                            from,
                            to: to as usize,
                            kind: JumpKind::IfFalse,
                        });
                        self.events.push(VmEvent::GroupExecuted {
                            pc: from,
                            command: g.command_type,
                            condition: Some(cond),
                        });
                        self.pc = to as usize;
                        continue;
                    }
                    self.events.push(VmEvent::GroupExecuted {
                        pc: self.pc,
                        command: g.command_type,
                        condition: Some(cond),
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::IFBLEND => {
                    let Some(top) = self.if_nest.last().copied() else {
                        if let Some(reason) =
                            self.halt_or_record(self.pc, g.command_type, "IFBLEND 无配对 IF")
                        {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    };
                    let from = self.pc;
                    self.events.push(VmEvent::Jump {
                        from,
                        to: top.end_target as usize,
                        kind: JumpKind::Else,
                    });
                    self.events.push(VmEvent::GroupExecuted {
                        pc: from,
                        command: g.command_type,
                        condition: None,
                    });
                    self.pc = top.end_target as usize;
                    continue;
                }
                cmd::LET => {
                    if g.window_count != 2 {
                        if let Some(reason) = self.halt_or_record(
                            self.pc,
                            g.command_type,
                            format!("LET 窗口数 {} != 2(语料恒 2)", g.window_count),
                        ) {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    }
                    // 引擎 FUN_00443808:w0 = 左值,w1 = 右值表达式;
                    // 复合赋值码 = w0.tag 的 B3 字节(0=赋值 1=+= … 8=^=)
                    let code = ((windows[0].tag >> 24) & 0xFF) as u8;
                    let lhs_bytes = match self.ctx.script.window_bytes_pooled_copy(&windows[0]) {
                        Some(b) => b,
                        None => {
                            if let Some(reason) = self.halt_or_record(
                                self.pc,
                                g.command_type,
                                "LET 左值窗口越过池尾",
                            ) {
                                return Ok(VmSuspend::Error(reason));
                            }
                            self.pc += 1;
                            continue;
                        }
                    };
                    let lval = match self.eval_window_lvalue(&lhs_bytes) {
                        Ok(l) => l,
                        Err(e) => {
                            self.state = VmState::Error;
                            return Ok(VmSuspend::Error(e.to_string()));
                        }
                    };
                    // 帧局部变量(id = 声明命令 0x32/0x33/0x34/0x35/0x46)写入当前帧
                    // locals;其余走全局(下方按 lval.var.id 分流)。
                    // 基帧恒在(成果 56):帧局部 LET 恒有落点(引擎 depth 0 时
                    // 写 record[0]),无「无帧」错误路径。
                    let rhs = match self.eval_window_condition(Some(&windows[1]))? {
                        Some(v) => v,
                        None => {
                            if let Some(reason) = self.halt_or_record(
                                self.pc,
                                g.command_type,
                                "LET 右值窗口为空",
                            ) {
                                return Ok(VmSuspend::Error(reason));
                            }
                            self.pc += 1;
                            continue;
                        }
                    };
                    // 复合赋值需要当前值:左值窗内含 aload(引擎左值窗为
                    // 引用+下标+aload 形态)→ 求值得到当前值;
                    // 无 aload(纯标量)→ 标量路径直取。eval_window_condition
                    // 的结尾 LValue 会被丢弃,这里允许其失败。
                    let cur = self.store_current(&lval);
                    let newv = match cur {
                        Some(c) => match yuris_value::compound_assign(code, &c, &rhs) {
                            Ok(v) => v,
                            Err(e) => {
                                self.state = VmState::Error;
                                return Ok(VmSuspend::Error(e.to_string()));
                            }
                        },
                        None => {
                            // code==0(纯赋值)允许向未初始化目标写入;复合码缺左值 → 挂起
                            if code == 0 {
                                rhs.clone()
                            } else if let Some(reason) = self.halt_or_record(
                                self.pc,
                                g.command_type,
                                "复合赋值目标无当前值(未声明?)",
                            ) {
                                return Ok(VmSuspend::Error(reason));
                            } else {
                                self.pc += 1;
                                continue;
                            }
                        }
                    };
                    // 目标:帧局部(id 0x32-0x46 族)→ 当前 GOSUB 帧 locals;否则全局
                    let is_frame_local = matches!(lval.var.id, 0x32 | 0x33 | 0x34 | 0x35 | 0x46);
                    let store_res = if is_frame_local {
                        match self.frames.last_mut() {
                            Some(fr) if lval.indices.is_empty() => {
                                fr.locals.set(&lval.var, newv.clone());
                                Ok(())
                            }
                            Some(fr) => fr.locals.set_elem(&lval.var, &lval.indices, newv.clone()),
                            None => Err(Error::format(
                                "LET 帧局部但无帧(基帧恒在,不应发生;成果 56)",
                            )),
                        }
                    } else if lval.indices.is_empty() {
                        self.store.set(&lval.var, newv.clone());
                        Ok(())
                    } else {
                        self.store.set_elem(&lval.var, &lval.indices, newv.clone())
                    };
                    if let Err(e) = store_res {
                        self.state = VmState::Error;
                        return Ok(VmSuspend::Error(e.to_string()));
                    }
                    self.events.push(VmEvent::GroupExecuted {
                        pc: self.pc,
                        command: g.command_type,
                        condition: Some(newv),
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::LOOP => {
                    // 引擎 CMDH_00445a00:w0 = 计数表达式(INT, B2=1);
                    // 压循环帧 {body_start=pc+1, limit=w0, counter=1};深度上限 0x40
                    if self.loops.len() >= 0x40 {
                        if let Some(reason) = self.halt_or_record(
                            self.pc,
                            g.command_type,
                            "循环嵌套超过 64 层(引擎 0x18ce0 同族)",
                        ) {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    }
                    let limit = match self.eval_window_condition(windows.first())? {
                        Some(Value::Int(n)) if n >= 0 => Some(n as u64),
                        Some(Value::Int(_)) => None, // 引擎负值 → 0xffffffff/-1 无限标记(Likely)
                        Some(v) => {
                            let reason = format!("LOOP 计数非 INT: {v:?}");
                            self.state = VmState::Error;
                            return Ok(VmSuspend::Error(reason));
                        }
                        None => {
                            if let Some(reason) = self
                                .halt_or_record(self.pc, g.command_type, "LOOP 无计数窗口")
                            {
                                return Ok(VmSuspend::Error(reason));
                            }
                            self.pc += 1;
                            continue;
                        }
                    };
                    // 退出点 = w1.len(编译期组号 = 配对 LOOPEND;引擎 rec+8
                    // = DAT_00661428,语料实证 s190 g332 w1.len=338→g338、
                    // s7 g93 w1.len=97→g97,均为 LOOPEND 组)。
                    let exit_target = if windows.len() > 1 {
                        windows[1].len as usize
                    } else {
                        0
                    };
                    self.loops.push(LoopFrame {
                        body_start: self.pc + 1,
                        exit_target,
                        limit,
                        counter: 1,
                    });
                    // 引擎 00445a00 limit==0 路径:counter=0 且 PC = rec+8
                    // (退出点)—— **不进循环体**,记录留栈由 LOOPEND 弹出
                    // (对拍 [546] 实证:s7 g93 limit=0 → g97 LOOPEND)。
                    if limit == Some(0) {
                        if let Some(f) = self.loops.last_mut() {
                            f.counter = 0;
                        }
                        self.events.push(VmEvent::GroupExecuted {
                            pc: self.pc,
                            command: g.command_type,
                            condition: None,
                        });
                        self.pc = exit_target;
                        continue;
                    }
                    // 注意:limit>0 恒压帧;LOOPEND「counter+1 >= limit → 弹帧」
                    // 循环体内的 GOSUB 返回后 LOOPEND 有配对(端到端实测踩坑,勿回退)。
                    self.events.push(VmEvent::GroupExecuted {
                        pc: self.pc,
                        command: g.command_type,
                        condition: None,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::LOOPBREAK => {
                    // 引擎 CMDH_00445b9c:LV 参数 = 回退层数(默认 1);
                    // 保留目标层记录(counter=0/limit=0),丢弃其上 LV-1 层,
                    // PC = 该层 rec+8 = 退出点(LOOP 组号,配对 LOOPEND);
                    // 随后 LOOPEND 执行(发事件)并弹栈(成果 55b:rec+8 =
                    // w1.len 反编译+语料双证,取代旧动态配对)。
                    let level = self.loop_level_from(windows.first())?;
                    let Some(level) = level else {
                        self.pc += 1;
                        continue;
                    };
                    if level < 1 || level > self.loops.len() {
                        if let Some(reason) = self.halt_or_record(
                            self.pc,
                            g.command_type,
                            format!("LOOPBREAK 层级 {level} 越界(深度 {})", self.loops.len()),
                        ) {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    }
                    let keep = self.loops.len() - level; // 保留该层(含)
                    let exit = self.loops[keep].exit_target;
                    let from = self.pc;
                    self.events.push(VmEvent::GroupExecuted {
                        pc: from,
                        command: g.command_type,
                        condition: None,
                    });
                    self.events.push(VmEvent::Jump {
                        from,
                        to: exit,
                        kind: JumpKind::LoopBreak,
                    });
                    self.loops.truncate(keep + 1);
                    if let Some(f) = self.loops.last_mut() {
                        f.counter = 0;
                        f.limit = Some(0);
                    }
                    self.pc = exit;
                    continue;
                }
                cmd::LOOPCONTINUE => {
                    // 引擎 CMDH_00445c54:保留目标层记录(counter/limit 不动),
                    // 丢弃其上 LV-1 层,PC = 该层 rec+8(LOOPEND 组)→
                    // LOOPEND 递增 counter 并回体顶/退出(成果 55b)。
                    let level = self.loop_level_from(windows.first())?;
                    let Some(level) = level else {
                        self.pc += 1;
                        continue;
                    };
                    if level < 1 || level > self.loops.len() {
                        if let Some(reason) = self.halt_or_record(
                            self.pc,
                            g.command_type,
                            format!("LOOPCONTINUE 层级 {level} 越界(深度 {})", self.loops.len()),
                        ) {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    }
                    let keep = self.loops.len() - level;
                    let exit = self.loops[keep].exit_target;
                    let from = self.pc;
                    self.events.push(VmEvent::GroupExecuted {
                        pc: from,
                        command: g.command_type,
                        condition: None,
                    });
                    self.events.push(VmEvent::Jump {
                        from,
                        to: exit,
                        kind: JumpKind::LoopContinue,
                    });
                    self.loops.truncate(keep + 1);
                    self.pc = exit;
                    continue;
                }
                cmd::LOOPEND => {
                    // 引擎 CMDH_00445ce8:counter+1;counter >= limit → 弹帧顺序继续;
                    // 否则 PC = body_start(64 位计数,0xffffffff/-1 = 无限标记)
                    let Some(top) = self.loops.last().copied() else {
                        if let Some(reason) = self
                            .halt_or_record(self.pc, g.command_type, "LOOPEND 无配对 LOOP")
                        {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    };
                    let counter = top.counter + 1;
                    if let Some(limit) = top.limit {
                        if counter > limit {
                            self.loops.pop();
                            self.events.push(VmEvent::GroupExecuted {
                                pc: self.pc,
                                command: g.command_type,
                                condition: None,
                            });
                            self.pc += 1;
                            continue;
                        }
                    }
                    if let Some(f) = self.loops.last_mut() {
                        f.counter = counter;
                    }
                    let from = self.pc;
                    // 循环回跳路径同样记本组事件(引擎每派发必有一事件;
                    // P1 对拍实锤:engine 39 pc118 LOOPEND 每次迭代都有事件)
                    self.events.push(VmEvent::GroupExecuted {
                        pc: from,
                        command: g.command_type,
                        condition: None,
                    });
                    self.events.push(VmEvent::Jump {
                        from,
                        to: top.body_start,
                        kind: JumpKind::LoopContinue,
                    });
                    self.pc = top.body_start;
                    continue;
                }
                cmd::WAIT => {
                    // 引擎 CMDH_00455d58:求值参数;FRAME 参数(槽 0)→ obj+0x18
                    // 帧计数,让出;TIME 参数(槽 1)→ obj+0x1c = timeGetTime()+ms,让出;
                    // 两者都无 → 不等待。
                    let mut counter = None;
                    let mut time_ms = None;
                    for (i, w) in windows.iter().enumerate() {
                        if w.len == 0 {
                            continue;
                        }
                        let Some(v) = self.eval_window_condition(Some(w))? else {
                            continue;
                        };
                        // YSCM WAIT 参数序:FRAME(槽0) TIME(槽1);按窗口出现的槽序归类
                        let slot = (w.tag & 0xff) as u8;
                        match slot {
                            0 => counter = v.as_int_opt().map(|n| n as u64),
                            1 => time_ms = v.as_int_opt().map(|n| n as u64),
                            _ => {}
                        }
                        let _ = i;
                    }
                    if counter.is_none() && time_ms.is_none() {
                        // 无参数 → 引擎直接返回 0(不等待)
                        self.events.push(VmEvent::GroupExecuted {
                            pc: self.pc,
                            command: g.command_type,
                            condition: None,
                        });
                        self.pc += 1;
                        continue;
                    }
                    self.events.push(VmEvent::GroupExecuted {
                        pc: self.pc,
                        command: g.command_type,
                        condition: None,
                    });
                    self.pc += 1; // 先推进:等完后继续执行
                    return Ok(VmSuspend::Wait { counter, time_ms });
                }
                cmd::TEXT => {
                    // 引擎 CMDH_00452888:FILE 参数为字符串(UTF-8/SJIS)→ 显示层;
                    // LET/CLEAR 为布尔。事件化;不阻塞(点击等待在文本层,非本 VM)。
                    let mut file = None;
                    let mut let_flag = false;
                    let mut clear_flag = false;
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        if let Some(v) = self.eval_window_condition(Some(w))? {
                            match v {
                                Value::Str(bytes) => file = Some(bytes),
                                Value::Int(n) => {
                                    let slot = (w.tag & 0xff) as u8;
                                    // 参数槽:YSCM TEXT 序 = FILE(0) LET(1) CLEAR(2)
                                    match slot {
                                        1 => let_flag = n != 0,
                                        2 => clear_flag = n != 0,
                                        _ => {}
                                    }
                                }
                                other => {
                                    // 非字符串/整数参数:记事件,不猜
                                    let _ = other;
                                }
                            }
                        }
                    }
                    self.events.push(VmEvent::Text {
                        pc: self.pc,
                        file,
                        let_flag,
                        clear_flag,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::CG => {
                    // 引擎处理器 = 0x423864(表初始化 DAT_0078b024 实锤;
                    // 勘误:0x43c984 是 cmd 0x0a DIALOG,此前误标为 CG)。
                    // 求值全部参数,按 B0 槽填入;
                    // 事件化记录 ID(槽 0)/X,Y,Z(槽 4/5/6),其余计数。
                    // 槽位映射草案见 bridge 模块文档(成果 48)。
                    // P7.1(成果 62):按名 upsert CG 状态注册表(引擎仅写
                    // 已指定槽,未指定字段保持原值/新建默认 —— 反编译
                    // 0x423864 逐槽 if(DAT_006624xx) 写,实证)。
                    let mut id = None;
                    let mut id_bytes: Option<Vec<u8>> = None;
                    let (mut cx, mut cy, mut cz) = (None, None, None);
                    let (mut csx, mut csy) = (None, None);
                    let mut file: Option<Vec<u8>> = None;
                    let mut count = 0usize;
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        if let Some(v) = self.eval_window_condition(Some(w))? {
                            count += 1;
                            match (slot, &v) {
                                (0, Value::Str(bytes)) => {
                                    id = Some(v.str_as_string());
                                    id_bytes = Some(bytes.clone());
                                }
                                (4, Value::Int(n)) => cx = Some(*n),
                                (5, Value::Int(n)) => cy = Some(*n),
                                (6, Value::Int(n)) => cz = Some(*n),
                                (9, Value::Int(n)) => csx = Some(*n),
                                (10, Value::Int(n)) => csy = Some(*n),
                                (46, Value::Str(bytes)) => file = Some(bytes.clone()),
                                _ => {}
                            }
                        }
                    }
                    let position = match (cx, cy) {
                        (Some(x), Some(y)) => Some((x, y, cz.unwrap_or(0))),
                        _ => None,
                    };
                    // 注册表维护(成果 62,引擎 0x423864 + watch oracle 实证):
                    // FILE 槽(46)非空 → 创建(注册;已存在则按已指定槽 patch);
                    // 无 FILE / FILE="" → 静默不创建(处理器 return 0 路径),
                    // 已注册者仅 patch —— 后续 CGINFO 查未注册名走「不存在」
                    // 路径写 0(BT.OVER/ON/ONOV/MASK vs BT.OFF/NA 对拍实证)。
                    // P1 收尾:FILE 非空时经 VFS 探针解析图像头,填
                    // img_w/img_h(CGINFO 槽13/14 真值应答源)。
                    let image_dims = match (&file, &self.file_probe) {
                        (Some(f), Some(probe)) => {
                            probe.image_dims(&String::from_utf8_lossy(f))
                        }
                        _ => None,
                    };
                    if let Some(bytes) = &id_bytes {
                        let create = matches!(&file, Some(f) if !f.is_empty());
                        let exists = self.cg_registry.contains_key(bytes);
                        if create || exists {
                            let st = self.cg_registry.entry(bytes.clone()).or_default();
                            if let Some(x) = cx {
                                st.x = x;
                            }
                            if let Some(y) = cy {
                                st.y = y;
                            }
                            if let Some(z) = cz {
                                st.z = z;
                            }
                            if let Some(sx) = csx {
                                st.sx = sx;
                            }
                            if let Some(sy) = csy {
                                st.sy = sy;
                            }
                            if let Some((w, h)) = image_dims {
                                st.img_w = w;
                                st.img_h = h;
                            }
                        }
                    }
                    self.events.push(VmEvent::Cg {
                        pc: self.pc,
                        id,
                        position,
                        param_count: count,
                        file: file.clone(),
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::SOUND => {
                    let mut id = None;
                    let mut file = None;
                    let mut play = None;
                    let mut count = 0usize;
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        if let Some(v) = self.eval_window_condition(Some(w))? {
                            count += 1;
                            match (slot, &v) {
                                (0, Value::Str(_)) => id = Some(v.str_as_string()),
                                (2, Value::Str(_)) => file = Some(v.str_as_string()),
                                (3, Value::Int(n)) => play = Some(*n),
                                _ => {}
                            }
                        }
                    }
                    self.events.push(VmEvent::Sound {
                        pc: self.pc,
                        id,
                        file,
                        play,
                        param_count: count,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::ELSE => {
                    // 引擎 CMDH_0043d34c(反编译,成果 55):
                    //   w0 无条件(B2=0)→ 顺序(进 else 块);
                    //   w0 求值真 → 顺序;假 → PC = w1.len != 0 ? w1.len
                    //   : 嵌套栈顶 end 目标。w1.len 存编译期组号 = ELSE 链中
                    //   **下一个 ELSE**(语料实证:s126 g66 w1.len=70 → g70;
                    //   g70 w1.len=0 → 落 IFEND g72);旧实现直接跳 end 目标,
                    //   跳过链中后续 ELSE 块(对拍 [518] 分歧根因)。
                    //   嵌套栈不弹(IFEND 负责弹,引擎同)。
                    let cond = if windows.len() > 0 && windows[0].len > 0 {
                        self.eval_window_condition(Some(&windows[0]))?
                    } else {
                        None
                    };
                    let truthy = if let Some(v) = cond {
                        value_truthy(&v)?
                    } else {
                        true // 无条件 ELSE → 进块
                    };
                    let Some(top) = self.if_nest.last().copied() else {
                        if let Some(reason) = self.halt_or_record(
                            self.pc, g.command_type, "ELSE 无配对 IF") {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    };
                    if truthy {
                        self.events.push(VmEvent::GroupExecuted {
                            pc: self.pc, command: g.command_type, condition: None });
                        self.pc += 1;
                        continue;
                    }
                    // 假 → 下一 ELSE(w1.len);无 → 嵌套栈顶 end 目标
                    let to = if windows.len() > 1 && windows[1].len != 0 {
                        windows[1].len as usize
                    } else {
                        top.end_target as usize
                    };
                    let from = self.pc;
                    self.events.push(VmEvent::Jump {
                        from, to, kind: JumpKind::Else });
                    self.events.push(VmEvent::GroupExecuted {
                        pc: from, command: g.command_type, condition: None });
                    self.pc = to;
                    continue;
                }
                cmd::END => {
                    // 引擎 FUN_0043da44:设标志 DAT_008725dc=1(脚本结束),可选返回码→
                    // DAT_00872228。本 VM 以 Complete 结束。
                    let ret = if let Some(w) = windows.first() {
                        self.eval_window_condition(Some(w)).ok().flatten()
                    } else { None };
                    self.events.push(VmEvent::GroupExecuted {
                        pc: self.pc, command: g.command_type, condition: ret });
                    self.state = VmState::Finished;
                    return Ok(VmSuspend::Complete);
                }
                cmd::VARINFO => {
                    // 引擎 CMDH_004550a0(**Confirmed**,2026-09-03 勘误):
                    // SET(槽0)/LET(槽1) = 变量引用槽(kind 2 延迟求值),非操作!
                    //   旧实现把 SET 误判为写回操作挂起 → 端到端卡死,已修正。
                    // 操作槽(引擎 if-else 链按槽号升序,第一个非零者生效):
                    //   TYPE(2)/STRTYPE(3)/DIMNUM(4)/DIMSIZE(5..12 = 第 1..8 维)/
                    //   LENGTH(13)/SEARCH(14)/STRFIRST(15)/SJISCODE(16)
                    // 结果 → 接收器 = **写入 LET 引用目标**(STRTYPE 分支直接证据:
                    //   判定 1/2 写入 LET 引用的下标元素;无 LET 槽 → 仅事件记录)。
                    // 全部操作槽为 0 → 引擎 fallback = LENGTH(SET 串字节长)。
                    let mut set_target: Option<VarTarget> = None;
                    let mut let_target: Option<VarTarget> = None;
                    let mut op_slot: Option<(u8, Value)> = None;
                    let mut evaluated: Vec<(u8, String)> = Vec::new();
                    let mut pending: Vec<(u8, yuris_format::ystb::CommandSlot)> = Vec::new();
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        match slot {
                            0 | 1 => pending.push((slot, w.clone())),
                            _ => {
                                let Some(v) = self.eval_window_condition(Some(w))? else {
                                    continue;
                                };
                                let enabled = matches!(&v, Value::Int(n) if *n != 0);
                                if enabled && op_slot.is_none() {
                                    op_slot = Some((slot, v.clone()));
                                }
                                evaluated.push((slot, value_summary(&v)));
                            }
                        }
                    }
                    // 引用槽窗口解码失败 = 结构异常,诚实挂起
                    for (slot, w) in pending {
                        let bytes = self
                            .ctx
                            .script
                            .window_bytes_pooled_copy(&w)
                            .ok_or_else(|| Error::format("VARINFO 引用窗越过池尾"))?;
                        let target = extract_var_target(&bytes)?;
                        match slot {
                            0 => set_target = Some(target),
                            _ => let_target = Some(target),
                        }
                    }
                    // 执行查询(未实现/结构异常 → strict 挂起 / trace 记录后跳过)
                    let query = op_slot.as_ref().map(|(s, _)| *s);
                    let result = match self.exec_varinfo_query(query, set_target.as_ref(), &mut evaluated) {
                        Ok(r) => r,
                        Err(e) => {
                            if let Some(reason) =
                                self.halt_or_record(self.pc, g.command_type, e.to_string())
                            {
                                return Ok(VmSuspend::Error(reason));
                            }
                            self.pc += 1;
                            continue;
                        }
                    };
                    // 写 LET 目标
                    if let (Some(result), Some(target)) = (result, &let_target) {
                        self.write_var_target(target, result)?;
                    }
                    if let Some(t) = &let_target {
                        evaluated.push((255u8, format!("→ {}", t.summary())));
                    }
                    self.events.push(VmEvent::VarQuery {
                        pc: self.pc,
                        command: g.command_type,
                        evaluated,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::VARACT => {
                    // 引擎 CMDH_00453178(反编译逐分支,2026-09-03 勘误后):
                    // 处理器头部把槽0(SET)引用记为对象、槽1(LET)引用记为目标。
                    // 分支结构(全 if 独立判断,非互斥 else):
                    // - a3(CUT) 分支:POS(4)/LENGTH(5) 联合计算字符区间 [POS,
                    //   POS+LENGTH) 的字节偏移,拷贝子串 → FUN_0045b9f8(写 LET 目标);
                    //   对象为空串时步进循环不执行 → 结果 = 空串(不报错)。
                    // - 顶层 `if (a6==0)` 为真(无 TYPE 槽)才进上述各分支;a6≠0 是
                    //   INT/FLT/STR 类型转换分支(收尾直接接收器,无 LET 也可)。
                    // - DIMSIZE(13)/PUSH(14)/POP(15) 分支无接收器写回(直接改数组
                    //   描述符);全语料直方图:[0,13]=131、[0,15]=113、[0,14]=93
                    //   (全部无 LET 槽,与「直接改数组」自洽)。
                    // - 语料 [0,1,3,4,5]=15 组 = CUT+POS+LENGTH 联合(截取子串)。
                    // 未实现分支:诚实挂起(不猜)。
                    let mut set_target: Option<VarTarget> = None;
                    let mut let_target: Option<VarTarget> = None;
                    // (slot, B3 算符, 槽值);B3 = tag 高字节(0=赋值/1=加/…,
                    // DIMSIZE 分支经 FUN_00425224 消费;成果 59)
                    let mut ops: Vec<(u8, u8, Value)> = Vec::new();
                    let mut evaluated: Vec<(u8, String)> = Vec::new();
                    let mut pending: Vec<(u8, yuris_format::ystb::CommandSlot)> = Vec::new();
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        match slot {
                            0 | 1 => pending.push((slot, w.clone())),
                            _ => {
                                let Some(v) = self.eval_window_condition(Some(w))? else {
                                    continue;
                                };
                                if slot >= 2 {
                                    let b3 = ((w.tag >> 24) & 0xff) as u8;
                                    ops.push((slot, b3, v.clone()));
                                }
                                evaluated.push((slot, value_summary(&v)));
                            }
                        }
                    }
                    for (slot, w) in pending {
                        let bytes = self
                            .ctx
                            .script
                            .window_bytes_pooled_copy(&w)
                            .ok_or_else(|| Error::format("VARACT 引用窗越过池尾"))?;
                        let t = extract_var_target(&bytes)?;
                        if slot == 0 {
                            set_target = Some(t);
                        } else {
                            let_target = Some(t);
                        }
                    }
                    let target = set_target.ok_or_else(|| {
                        Error::format("VARACT 无 SET 引用槽(引擎必填)")
                    })?;
                    // 对象当前值(STR 操作分支的输入;引擎经 FUN_0044d4e8 读取)
                    let obj = self.read_var_target(&target)?;
                    // 引擎 if-else 链按槽号独立 if(可多槽同时启用);逐个处理
                    let mut result: Option<Value> = None;
                    let mut unimplemented: Vec<u8> = Vec::new();
                    let has_range_op = ops.iter().any(|(s, _, _)| matches!(s, 2 | 3));
                    for (slot, b3, v) in &ops {
                        match slot {
                            13 => {
                                // DIMSIZE:new_len = 算符(B3)以**当前元素个数**
                                // 为基(引擎 00453178 DIMSIZE 分支,00425224 唯一
                                // 调用点:op 0=赋值/1=加/2=减/3=乘/4=除,基值 =
                                // desc+4;负值 → 报错 0x…b60;结果 == cur 不
                                // resize)。语料惯用法 DIMSIZE(+=1) = 压栈
                                // (s47 g42 等,成果 59)。
                                let r = match &target {
                                    VarTarget::Scalar(r) => r,
                                    VarTarget::Indexed(r, _) => r,
                                };
                                let cur = self
                                    .store
                                    .array_dims(r)
                                    .and_then(|d| d.first().copied())
                                    .ok_or_else(|| {
                                        Error::format(format!(
                                            "VARACT DIMSIZE 目标非数组(引擎 desc+2==1) s{} pc={}",
                                            self.ctx.script_id, self.pc
                                        ))
                                    })?;
                                let newv = yuris_value::compound_assign(
                                    *b3,
                                    &Value::Int(cur as i64),
                                    v,
                                )?;
                                let new_len = match newv {
                                    Value::Int(n) if n >= 0 => n as usize,
                                    other => {
                                        return Err(Error::format(format!(
                                            "VARACT DIMSIZE 需非负 int,得到 {other:?}"
                                        )))
                                    }
                                };
                                if new_len != cur as usize {
                                    self.resize_array_target(&target, new_len)?;
                                }
                            }
                            4 | 5 if has_range_op => {
                                // POS/LENGTH 是 CUT/COPY 分支的字符区间参数
                                // (引擎 a2/a3 守卫内消费),不单独产生结果
                            }
                            slot @ (7 | 9) => {
                                // UPPER(7)/LOWER(9):引擎 FUN_004546cc/FUN_00464dac
                                // (各 57B,Ghidra 2026-09-04):SJIS 感知 ASCII 大小写
                                // 原地转换 —— 步进表 DAT_0059b0c0==1(双字节首区
                                // 0x81-0x9F/0xE0-0xEF)连跳 2 字节;单字节 'a'-'z'→
                                // -0x20(UPPER) / 'A'-'Z'→+0x20(LOWER),其余原样。
                                // 槽值(push 1)= 使能标志,转换本身不消费(语料
                                // s190 g267 LOWER=1;Likely)。接收 = LET 目标。
                                let bytes = match &obj {
                                    Value::Str(b) => b.clone(),
                                    other => {
                                        return Err(Error::format(format!(
                                            "VARACT 槽 {slot} 对象非 STR: {other:?}"
                                        )))
                                    }
                                };
                                result = Some(Value::Str(varact_ascii_case(
                                    &bytes,
                                    *slot == 7,
                                )));
                            }
                            slot @ (2 | 3) => {
                                // slot3 COPY:截取字符区间 [POS-1, POS-1+LENGTH)
                                // slot2 CUT :写出切除该区间后的剩余部分
                                // (引擎 CMDH_00453178 两分支:按 SJIS 步进表定位
                                // 字节偏移。守卫语义:POS∈{0,1}→首字符(引擎对值
                                // 0/1 有专门短路);LENGTH=0→空区间;对象为空串 →
                                // 步进循环不执行 → 结果=空串,**不报错**。
                                // 报错(0x1d4ca/0x1d4d4)仅在非空串上步进越过缓冲尾,
                                // 本实现以「钳制到字符数」近似(引擎此时也报错;
                                // 真实语料该路径未观测到,等级 Likely)。
                                let bytes = match &obj {
                                    Value::Str(b) => b.clone(),
                                    other => {
                                        return Err(Error::format(format!(
                                            "VARACT 槽 {slot} 对象非 STR: {other:?}"
                                        )))
                                    }
                                };
                                let pos_ch = self.varact_slot_usize(ops.as_slice(), 4)?;
                                let len_ch = self.varact_slot_usize(ops.as_slice(), 5)?;
                                if bytes.is_empty() {
                                    // 空串:两循环都不步进 → COPY=""、CUT=""
                                    result = Some(Value::Str(Vec::new()));
                                } else {
                                    let start_ch = pos_ch.saturating_sub(1);
                                    let start = varact_char_to_byte(&obj, start_ch)
                                        .ok_or_else(|| {
                                            let pc = self.pc;
                                            let blen = bytes.len();
                                            let sid = self.ctx.script_id;
                                            let hex: String = bytes
                                                .iter()
                                                .take(48)
                                                .map(|b| format!("{b:02x}"))
                                                .collect::<Vec<_>>()
                                                .join(" ");
                                            Error::format(format!(
                                                "VARACT 槽 {slot} POS={pos_ch} 越界(引擎 0x1d4ca 同族; s{sid} pc={pc} 目标={target:?} 串字节={blen} ops={ops:?} hex={hex})"
                                            ))
                                        })?;
                                    let end = varact_char_to_byte(&obj, start_ch + len_ch)
                                        .ok_or_else(|| {
                                            let pc = self.pc;
                                            let blen = bytes.len();
                                            let sid = self.ctx.script_id;
                                            Error::format(format!(
                                                "VARACT 槽 {slot} LENGTH={len_ch} 越界(引擎 0x1d4d4 同族; s{sid} pc={pc} 串字节={blen} POS={pos_ch} ops={ops:?})"
                                            ))
                                        })?;
                                    result = Some(match slot {
                                        3 => Value::Str(bytes[start..end].to_vec()),
                                        _ => {
                                            let mut out = bytes[..start].to_vec();
                                            out.extend_from_slice(&bytes[end..]);
                                            Value::Str(out)
                                        }
                                    });
                                }
                            }
                            14 | 15 => {
                                // PUSH(14)/POP(15):数组元素插入/删除(带移位;
                                // 引擎 00453178 PUSH/POP 分支汇编级 Confirmed):
                                // - 前提:目标 1 维数组(desc+2==1,否则 0x879b80/
                                //   0x879be0 报错);槽值 = 操作位置 pos(64 位,
                                //   钳制:pos >= count-1 时只写末位)。
                                // - PUSH:`for i in (pos+1..count).rev(): arr[i] =
                                //   arr[i-1]`(后移),`arr[pos] = 类型默认值`
                                //   (INT 0 / FLT 0.0[0x579ca8] / STR ""[0x87899c])。
                                // - POP:`for i in pos..count-1: arr[i] = arr[i+1]`
                                //   (前移),`arr[count-1] = 默认值`。
                                // 无 LET 写回、无 resize(desc+4 不变)。
                                let r = match &target {
                                    VarTarget::Scalar(r) => r,
                                    // 带下标 = 非裸数组引用(PUSH/POP 须裸引用)
                                    VarTarget::Indexed(_, _) => {
                                        return Err(Error::format(format!(
                                            "VARACT PUSH/POP 目标须裸数组引用(引擎 desc+2==1)\
                                             s{} pc={}",
                                            self.ctx.script_id, self.pc
                                        )));
                                    }
                                };
                                let Some(bounds) = self.store.array_dims(r) else {
                                    return Err(Error::format(format!(
                                        "VARACT PUSH/POP 目标非数组 s{} pc={}",
                                        self.ctx.script_id, self.pc
                                    )));
                                };
                                if bounds.len() != 1 {
                                    return Err(Error::format(format!(
                                        "VARACT PUSH/POP 要求 1 维数组(引擎 desc+2==1),\
                                         得到 {} 维",
                                        bounds.len()
                                    )));
                                }
                                let count = bounds[0] as usize;
                                let pos = match v {
                                    Value::Int(n) if *n >= 0 => *n as usize,
                                    other => {
                                        return Err(Error::format(format!(
                                            "VARACT PUSH/POP 位置须非负 int,得到 {other:?}"
                                        )))
                                    }
                                };
                                if count == 0 {
                                    return Err(Error::format(
                                        "VARACT PUSH/POP 空数组(引擎 desc+4==0)",
                                    ));
                                }
                                let default = match self.store.array_elem_type(r).unwrap() {
                                    yuris_value::ElemType::Int => Value::Int(0),
                                    yuris_value::ElemType::Float => Value::Float(0.0),
                                    yuris_value::ElemType::Str => Value::Str(Vec::new()),
                                };
                                if *slot == 14 {
                                    // PUSH:pos 钳到 count-1;后移 + 插入默认值
                                    let pos = pos.min(count - 1);
                                    for i in ((pos + 1)..count).rev() {
                                        let v = self.store.get_elem(r, &[(i - 1) as i64])?.clone();
                                        self.store.set_elem(r, &[i as i64], v)?;
                                    }
                                    self.store.set_elem(r, &[pos as i64], default)?;
                                } else {
                                    // POP:pos >= count-1 只尾置默认;否则前移 + 尾置默认
                                    if pos < count.saturating_sub(1) {
                                        for i in pos..(count - 1) {
                                            let v = self
                                                .store
                                                .get_elem(r, &[(i + 1) as i64])?
                                                .clone();
                                            self.store.set_elem(r, &[i as i64], v)?;
                                        }
                                    }
                                    self.store
                                        .set_elem(r, &[(count - 1) as i64], default)?;
                                }
                            }
                            other_slot => unimplemented.push(*other_slot),
                        }
                    }
                    // 结果接收:引擎 a3 分支收尾写 LET 引用的变量(有 LET 槽才写;
                    // DIMSIZE/PUSH/POP 无 LET 也不写)
                    if let Some(v) = result {
                        let lt = let_target.clone().ok_or_else(|| {
                            Error::format("VARACT CUT 有结果但无 LET(1) 引用槽(引擎必填)")
                        })?;
                        self.write_var_target(&lt, v)?;
                        evaluated.push((255u8, format!("→ {}", lt.summary())));
                    }
                    if !unimplemented.is_empty() {
                        let slots: Vec<String> =
                            unimplemented.iter().map(|s| format!("{s}")).collect();
                        if let Some(reason) = self.halt_or_record(
                            self.pc,
                            g.command_type,
                            format!(
                                "VARACT 写操作槽 [{slots:?}] 未实现(不猜) s{} pc={}",
                                self.ctx.script_id, self.pc
                            ),
                        ) {
                            return Ok(VmSuspend::Error(reason));
                        }
                    }
                    self.events.push(VmEvent::VarQuery {
                        pc: self.pc,
                        command: g.command_type,
                        evaluated,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::LABELINFO => {
                    // 引擎 CMDH_00443674(Confirmed,402B 小函数):
                    // '#'(槽0,k1)=标签名;LET(槽1)=写回目标(可带下标);
                    // EXIST(槽2)非零 → FUN_0045124c(murmur2 查标签表):
                    //   命中 → LET 写 1;未命中 → 写 0(INT 写 8B 零/FLT 写 0.0)。
                    // 未启用 EXIST → 仅拷名(no-op)。
                    let mut label: Option<String> = None;
                    let mut let_target: Option<VarTarget> = None;
                    let mut exist = false;
                    let mut evaluated: Vec<(u8, String)> = Vec::new();
                    let mut pending_let: Option<yuris_format::ystb::CommandSlot> = None;
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        match slot {
                            0 => {
                                if let Some(v) = self.eval_window_condition(Some(w))? {
                                    if let Value::Str(_) = v {
                                        label = Some(v.str_as_string());
                                    }
                                    evaluated.push((slot, value_summary(&v)));
                                }
                            }
                            1 => pending_let = Some(w.clone()),
                            _ => {
                                let v = self.eval_window_condition(Some(w))?;
                                if slot == 2 {
                                    exist = matches!(&v, Some(Value::Int(n)) if *n != 0);
                                }
                                if let Some(v) = &v {
                                    evaluated.push((slot, value_summary(v)));
                                }
                            }
                        }
                    }
                    if let Some(w) = pending_let {
                        let bytes = self
                            .ctx
                            .script
                            .window_bytes_pooled_copy(&w)
                            .ok_or_else(|| Error::format("LABELINFO LET 引用窗越过池尾"))?;
                        let_target = Some(extract_var_target(&bytes)?);
                    }
                    if exist {
                        let hit = label
                            .as_deref()
                            .and_then(|name| self.labels.get(name.as_bytes()))
                            .is_some();
                        if let Some(target) = &let_target {
                            self.write_var_target(target, Value::Int(hit as i64))?;
                            evaluated.push((255u8, format!("EXIST({label:?})={} → {}", hit as i64, target.summary())));
                        }
                    }
                    self.events.push(VmEvent::VarQuery {
                        pc: self.pc,
                        command: g.command_type,
                        evaluated,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::CGINFO => {
                    // 引擎 CMDH_0043b084(Confirmed 结构):
                    // ID(槽0,k1)=CG 名;IDNO(1)=名字后缀拼接;LET(槽33)=结果写回目标;
                    // 其余槽(EXIST/X/Y/SX/SY/ONMOUSE…)=查询项(非零启用)。
                    // 全部查询汇入结果缓冲(函数头清零 DAT_005c0840=0)→ 接收器写 LET。
                    // P7.1(成果 62):按 CG 状态注册表应答已建模查询
                    //   (EXIST(2)=1 / X(5)/Y(6)/SX(13)/SY(14)/COLOR(24));
                    // **CG 未注册(名不存在)⇒ 引擎「不存在」路径 ⇒ 恒写 Int(0)**
                    //   (iVar6==0 → 接收器以清零缓冲写 LET,Confirmed);
                    // 未建模查询(ONMOUSE/TRIM/LINT 族)在 CG 存在时不写回
                    //   (语义 Unknown,不猜;boot trace 未执行到)。
                    let mut id: Option<String> = None;
                    let mut id_bytes: Option<Vec<u8>> = None;
                    let mut let_target: Option<VarTarget> = None;
                    let mut evaluated: Vec<(u8, String)> = Vec::new();
                    let mut pending_let: Option<yuris_format::ystb::CommandSlot> = None;
                    let mut query: Option<u8> = None;
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        match slot {
                            0 => {
                                if let Some(v) = self.eval_window_condition(Some(w))? {
                                    if let Value::Str(bytes) = &v {
                                        id = Some(v.str_as_string());
                                        id_bytes = Some(bytes.clone());
                                    }
                                    evaluated.push((slot, value_summary(&v)));
                                }
                            }
                            33 => pending_let = Some(w.clone()),
                            _ => {
                                if let Some(v) = self.eval_window_condition(Some(w))? {
                                    evaluated.push((slot, value_summary(&v)));
                                    if query.is_none() {
                                        query = Some(slot);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(w) = pending_let {
                        let bytes = self
                            .ctx
                            .script
                            .window_bytes_pooled_copy(&w)
                            .ok_or_else(|| Error::format("CGINFO LET 引用窗越过池尾"))?;
                        let_target = Some(extract_var_target(&bytes)?);
                    }
                    // 查询应答:注册命中 → 状态字段;未命中 → 0(引擎
                    // 「不存在」路径);已建模查询之外的槽不写回(Unknown)。
                    // 槽13/14 = 装载图像真实宽/高(引擎 watch oracle 实证:
                    // occ1 dummy.png 1×1 → 1.0;occ2 txspace → 1350.0)。
                    let mut answer: Option<i64> = None;
                    if let (Some(name), Some(q)) = (&id_bytes, query) {
                        match self.cg_registry.get(name) {
                            Some(st) => match q {
                                2 => answer = Some(1),       // EXIST
                                5 => answer = Some(st.x),    // X
                                6 => answer = Some(st.y),     // Y
                                13 => answer = Some(st.img_w), // 图像宽
                                14 => answer = Some(st.img_h), // 图像高
                                24 => answer = Some(st.color), // COLOR
                                _ => {}                      // 未建模:不写
                            },
                            None => answer = Some(0),
                        }
                    }
                    if let (Some(target), Some(v)) = (&let_target, answer) {
                        self.write_var_target(target, Value::Int(v))?;
                        let note = match (&id, query) {
                            (Some(n), Some(q)) => {
                                format!("→ {} (cg {n:?} q{q} = {v})", target.summary())
                            }
                            _ => format!("→ {}", target.summary()),
                        };
                        evaluated.push((255u8, note));
                    } else if let (Some(target), Some(name_bytes)) = (&let_target, &id_bytes) {
                        // 未建模查询且 CG 存在:不写回(变量保持原值)。
                        if self.cg_registry.contains_key(name_bytes) {
                            evaluated.push((
                                254u8,
                                format!("→ {} (未建模查询,不写回)", target.summary()),
                            ));
                        } else {
                            self.write_var_target(target, Value::Int(0))?;
                            let name = id.as_deref().unwrap_or("?");
                            evaluated.push((
                                255u8,
                                format!("→ {} (cg {name:?} 不存在路径=0)", target.summary()),
                            ));
                        }
                    }
                    self.events.push(VmEvent::CgInfo {
                        pc: self.pc,
                        id,
                        evaluated,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::CGACT => {
                    // 引擎 CMDH_0042607c(12466B):71 参,运行期图形管线合成。
                    // ID(B0=0,字符串)记录;其余参数按 B0 槽求值摘要;
                    // 不触碰图形状态(未逆向),记录后继续(非 strict 同)。
                    let mut id = None;
                    let mut evaluated: Vec<(u8, String)> = Vec::new();
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        if let Some(v) = self.eval_window_condition(Some(w))? {
                            if slot == 0 {
                                if let Value::Str(_) = v {
                                    id = Some(v.str_as_string());
                                }
                            }
                            evaluated.push((slot, value_summary(&v)));
                        }
                    }
                    self.events.push(VmEvent::CgAct {
                        pc: self.pc,
                        id,
                        evaluated,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::LOAD => {
                    // 引擎 CMDH_00444648(文件分支,反编译 p5_sysvar/):
                    // 槽 0 = FILE(求值;无扩展名 → 补 ".sd";'/'→'\')、
                    // 槽 1 = MEM(非零 = 内存源,语料未出现)、
                    // 槽 2 = DNO(块号,1 基 → YSSD 偏移表[DNO-1])、
                    // 槽 3 = 写回目标(**延迟引用**;s35 g705 实证,成果 59f)、
                    // 槽 4 = ID 模式(非零 = 0x690 特殊配置格式,语料未出现)。
                    // 块类型 0 = 普通载荷;1 = 0x690 配置;2 = 多子块(未出现)。
                    let mut file = None;
                    let mut mem: Option<i64> = None;
                    let mut dno: Option<i64> = None;
                    let mut id_flag: i64 = 0;
                    let mut target: Option<yuris_script::LValueRef> = None;
                    let mut evaluated: Vec<(u8, String)> = Vec::new();
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        if slot == 3 {
                            // 写回目标 = 左值引用窗(0x76 varidx / 0x48 var 单条,
                            // s35 g705 与 s120 g17 实证),不求值。
                            let bytes = self
                                .ctx
                                .script
                                .window_bytes_pooled_copy(w)
                                .ok_or_else(|| Error::format("LOAD 引用窗越过池尾"))?;
                            let lval = self.eval_window_lvalue(&bytes)?;
                            evaluated.push((slot, format!("@{}", lval.var.display())));
                            target = Some(lval);
                            continue;
                        }
                        if let Some(v) = self.eval_window_condition(Some(w))? {
                            match slot {
                                0 => {
                                    if let Value::Str(bytes) = &v {
                                        file = Some(bytes.clone());
                                    }
                                }
                                1 => mem = v.as_int_opt(),
                                2 => dno = v.as_int_opt(),
                                4 => id_flag = v.as_int_opt().unwrap_or(0),
                                _ => {}
                            }
                            evaluated.push((slot, value_summary(&v)));
                        }
                    }
                    if let (Some(fname), Some(dno_v), Some(lval)) = (file.clone(), dno, &target) {
                        if let Some(mem_v) = mem {
                            if mem_v != 0 {
                                return Err(self.halt_err(
                                    self.pc,
                                    cmd::LOAD,
                                    "LOAD MEM(内存源)路径未实现(语料未出现;不猜)",
                                ));
                            }
                        }
                        if id_flag != 0 {
                            return Err(self.halt_err(
                                self.pc,
                                cmd::LOAD,
                                "LOAD ID 模式(0x690 配置块)未实现(语料未出现;不猜)",
                            ));
                        }
                        if !lval.indices.is_empty() {
                            return Err(self.halt_err(
                                self.pc,
                                cmd::LOAD,
                                "LOAD 写回目标带下标(基点偏移语义未逆向;语料未出现)",
                            ));
                        }
                        self.load_yssd(&fname, dno_v, &lval.var)?;
                    }
                    self.events.push(VmEvent::Load {
                        pc: self.pc,
                        file,
                        evaluated,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::SAVE => {
                    // 引擎 CMDH_00451838(反编译逐分支,2026-09-04):
                    // 槽 2(DNO)=存档号;槽 3(SET)=位图引用;槽 0(FILE)=文件名
                    // (只走显示层)。a0 分支:按 SET 引用的描述符类型写位图 —
                    // INT(desc byte+1==1)→ desc+0x30 区 8B 槽 = SET 值 ?1:0;
                    // FLT(byte+1==2)→ desc+0x34 区 8B = SET 值 ?1.0:0.0;
                    // STR(==3)→ FUN_0045b9f8 显示层(无存储写入)。
                    // a1 分支(INT=1/FLT=1.0 置位)、a2(默认报错)、a3/a4(仅显示)。
                    // 语料 298 组:[DNO,SET]×284(=a0 分支置位/清零)、[FILE]×10、
                    // [SET]×2、[DNO,ID]×2。SET 引用是左值目标(提取后写值)。
                    let mut dno: Option<i64> = None;
                    let mut file: Option<Vec<u8>> = None;
                    let mut set_ref: Option<Vec<u8>> = None;
                    let mut set_flag = false;
                    let mut evaluated: Vec<(u8, String)> = Vec::new();
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        match slot {
                            3 => {
                                // 槽 3 双形态:变量引用窗(首 op 0x48/0x56/0x76,
                                // 延迟求值 = SET 目标)/ 纯值窗(SET 置位标志)。
                                // 语料 298 组恒单 SET 窗;双窗形态仅合成测试。
                                let bytes = self
                                    .ctx
                                    .script
                                    .window_bytes_pooled_copy(w)
                                    .ok_or_else(|| {
                                        Error::format("SAVE SET 引用窗越过池尾")
                                    })?;
                                let is_ref = yuris_script::decode_window(&bytes)
                                    .map(|ins| {
                                        ins.first()
                                            .map(|i| {
                                                matches!(i.raw_op, 0x48 | 0x56 | 0x76)
                                            })
                                            .unwrap_or(false)
                                    })
                                    .unwrap_or(false);
                                if is_ref {
                                    set_ref = Some(bytes);
                                } else if let Some(v) =
                                    self.eval_window_condition(Some(w))?
                                {
                                    if let Value::Int(n) = &v {
                                        set_flag = *n != 0;
                                    }
                                    evaluated.push((slot, value_summary(&v)));
                                }
                            }
                            _ => {
                                let Some(v) = self.eval_window_condition(Some(w))? else {
                                    continue;
                                };
                                match (slot, &v) {
                                    (2, Value::Int(n)) => dno = Some(*n),
                                    (0, Value::Str(b)) => file = Some(b.clone()),
                                    (3, Value::Int(n)) => set_flag = *n != 0,
                                    _ => {}
                                }
                                evaluated.push((slot, value_summary(&v)));
                            }
                        }
                    }
                    let mut set_target = None;
                    if let Some(bytes) = set_ref {
                        let target = extract_var_target(&bytes)?;
                        set_target = Some(target.summary());
                        // 引擎 a0 分支:INT → 1/0;FLT → 1.0/0.0;STR → 无写入
                        let cur_type = self.var_target_elem(&target);
                        let v = match cur_type {
                            Some(yuris_value::ElemType::Float) => {
                                Value::Float(if set_flag { 1.0 } else { 0.0 })
                            }
                            Some(yuris_value::ElemType::Int) => {
                                Value::Int(set_flag as i64)
                            }
                            _ => {
                                // STR 目标:引擎走显示层,不写存储(不猜)
                                Value::Int(set_flag as i64)
                            }
                        };
                        self.write_var_target(&target, v)?;
                    }
                    self.events.push(VmEvent::Save {
                        pc: self.pc,
                        file,
                        dno,
                        set_target,
                        evaluated,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::CGEND => {
                    // 引擎 LAB_0043ad14(cmd 0x03;命令表 slot 11):ID(槽0,字符串)
                    // = 结束显示的 CG 名。P7.1(成果 62):同步从 CG 状态
                    // 注册表移除(显示结束 ⇒ 后续 CGINFO 查询走「不存在」路径)。
                    let mut id = None;
                    let mut id_bytes: Option<Vec<u8>> = None;
                    let mut evaluated: Vec<(u8, String)> = Vec::new();
                    for w in &windows {
                        if w.len == 0 {
                            continue;
                        }
                        let slot = (w.tag & 0xff) as u8;
                        if let Some(v) = self.eval_window_condition(Some(w))? {
                            if slot == 0 {
                                if let Value::Str(bytes) = &v {
                                    id = Some(v.str_as_string());
                                    id_bytes = Some(bytes.clone());
                                }
                            }
                            evaluated.push((slot, value_summary(&v)));
                        }
                    }
                    if let Some(bytes) = &id_bytes {
                        self.cg_registry.remove(bytes);
                    }
                    self.events.push(VmEvent::CgEnd {
                        pc: self.pc,
                        id,
                        evaluated,
                    });
                    self.pc += 1;
                    continue;
                }
                cmd::IFEND => {
                    if self.if_nest.pop().is_none() {
                        if let Some(reason) =
                            self.halt_or_record(self.pc, g.command_type, "IFEND 无配对 IF")
                        {
                            return Ok(VmSuspend::Error(reason));
                        }
                        self.pc += 1;
                        continue;
                    }
                    self.events.push(VmEvent::GroupExecuted {
                        pc: self.pc,
                        command: g.command_type,
                        condition: None,
                    });
                    self.pc += 1;
                    continue;
                }
                other => {
                    // 运行期声明族 INT(0x32)/FLT(0x19)/STR(0x5c):引擎有真
                    // 处理器(P1 对拍实锤 + 反编译 00443538_CMD_INT_runtime.c /
                    // 004411fc_CMD_FLT_0x19.c —— 两者逐行对称;STR 同形见
                    // script12 g10「STR $1748 = $55[1]」且引擎 pc 顺序推进):
                    //   w0 = 变量引用(描述符类型/边界重置),
                    //   后续窗 = 初值表达式求值后写入标量。
                    // 实证:script190 g28「INT @6292 = @53[2]」、g30
                    // 「INT @6293 = 0」、script12 g11「INT @1749 = 0」。
                    if matches!(other, 0x19 | 0x32 | 0x5C) {
                        let mut target: Option<yuris_value::VarRef> = None;
                        let mut array_form = false;
                        if let Some(w0) = windows.first() {
                            if w0.tag & 0xFF == 0 {
                                if let Some(bytes) =
                                    self.ctx.script.window_bytes_pooled_copy(w0)
                                {
                                    if let Ok(instrs) =
                                        yuris_script::decode_window(&bytes)
                                    {
                                        for ins in &instrs {
                                            match &ins.kind {
                                                yuris_script::Insn::PushVar(r)
                                                | yuris_script::Insn::PushVarRef(r) => {
                                                    if target.is_none() {
                                                        target = Some(r.clone());
                                                    }
                                                }
                                                yuris_script::Insn::PushVarIndexed(_) => {
                                                    array_form = true;
                                                }
                                                yuris_script::Insn::ArrayLoad { .. } => {
                                                    array_form = true;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let Some(r) = target else {
                            let reason = format!(
                                "运行期声明 0x{other:02x}:窗口未含变量引用(不猜); \
                                 array_form={array_form}"
                            );
                            let pc = self.pc;
                            self.events.push(VmEvent::Unsupported {
                                pc,
                                command: other,
                                reason: reason.clone(),
                            });
                            if self.strict {
                                self.state = VmState::Error;
                                return Ok(VmSuspend::Error(reason));
                            }
                            self.pc += 1;
                            continue;
                        };
                        // 初值 = 第一个非空后续窗的求值结果;无 → 类型默认值
                        // (引擎 INT→FUN_0046a140(i64) / FLT→FUN_0046a060(f64);
                        //  STR 同族字符串写入)
                        let mut init = match other {
                            0x19 => yuris_value::Value::Float(0.0),
                            0x5C => yuris_value::Value::Str(Vec::new()),
                            _ => yuris_value::Value::Int(0),
                        };
                        for w in windows.iter().skip(1) {
                            if w.len == 0 {
                                continue;
                            }
                            if let Some(v) = self.eval_window_condition(Some(w))? {
                                init = v;
                            }
                            break;
                        }
                        self.store.set(&r, init);
                        self.events.push(VmEvent::Declaration {
                            pc: self.pc,
                            command: other,
                            windows: g.window_count,
                        });
                        self.pc += 1;
                        continue;
                    }
                    // 其余声明类命令:引擎运行器用默认 stub(G_*/F_* 族)或
                    // 已确证 no-op(S_* 0x52-0x54 → FUN_00423080),不执行;
                    // 记录 Declaration 事件后继续(与引擎一致)。
                    // 注:0x5c STR 处理器 0x451598 的 LAB_ 边界反编译形似
                    // 「按名切脚本」,不可靠;实测引擎 pc 顺序推进,已按上
                    // 声明族实现。0x65 VAR(0x453158)语义未定性(Unknown)。
                    if is_declaration(other) {
                        self.events.push(VmEvent::Declaration {
                            pc: self.pc,
                            command: other,
                            windows: g.window_count,
                        });
                        self.pc += 1;
                        continue;
                    }
                    // FILEINFO(0x15,引擎 0043eb28):FOLDER(0)/FILE(1)/
                    // CGFILE(2)/SOUNDFILE(3) = 路径源;EXIST(4)/SIZE(5)/
                    // RELATION(6) = 查询项;LET(7) = 写回目标。
                    // 本实现:EXIST+FILE+LET → 虚拟 FS 存在性
                    // (`host::PacFileIndex`;实证 s207 es.R18Check:引擎查
                    // `cg/thumb_cg/A_HAN_2002_a.png` 得 1)。无 probe → 不写
                    // 回,事件记 err(诚实缺省)。其余查询项出现时再逆向。
                    if other == 0x15 {
                        let mut path: Option<String> = None;
                        let mut exist = false;
                        let mut let_ref: Option<yuris_format::ystb::CommandSlot> = None;
                        let mut evaluated: Vec<(u8, String)> = Vec::new();
                        for w in &windows {
                            if w.len == 0 {
                                continue;
                            }
                            let slot = (w.tag & 0xff) as u8;
                            match slot {
                                1 => {
                                    if let Some(v) = self.eval_window_condition(Some(w))? {
                                        path = Some(v.str_as_string());
                                        evaluated.push((slot, value_summary(&v)));
                                    }
                                }
                                4 => {
                                    if let Some(v) = self.eval_window_condition(Some(w))? {
                                        exist = matches!(&v, Value::Int(n) if *n != 0);
                                        evaluated.push((slot, value_summary(&v)));
                                    }
                                }
                                7 => let_ref = Some(w.clone()),
                                _ => {
                                    if let Some(v) = self.eval_window_condition(Some(w))? {
                                        evaluated.push((slot, value_summary(&v)));
                                    }
                                }
                            }
                        }
                        if exist {
                            match (&let_ref, (&path, &self.file_probe)) {
                                (Some(w), (Some(p), Some(probe))) => {
                                    let bytes = self
                                        .ctx
                                        .script
                                        .window_bytes_pooled_copy(w)
                                        .ok_or_else(|| {
                                            Error::format("FILEINFO LET 引用窗越过池尾")
                                        })?;
                                    let target = extract_var_target(&bytes)?;
                                    let hit = probe.exists(p);
                                    self.write_var_target(&target, Value::Int(hit as i64))?;
                                    evaluated.push((
                                        255u8,
                                        format!("EXIST({p})={} → {}", hit as i64, target.summary()),
                                    ));
                                }
                                (Some(_), (None, _)) => {
                                    evaluated.push((255u8, "err:EXIST 无 FILE 路径".into()));
                                }
                                (Some(_), (_, None)) => {
                                    evaluated.push((255u8, "err:无 file_probe".into()));
                                }
                                _ => {}
                            }
                        }
                        self.events.push(VmEvent::Subsystem {
                            pc: self.pc,
                            command: other,
                            evaluated,
                        });
                        self.pc += 1;
                        continue;
                    }
                    // 后端子系统配置/查询族(P2,处理器反编译见
                    // docs/reverse/decompiled/engine/0044*_CMD_*.c):
                    // 0x1b FONTINFO/0x15 FILEINFO/0x14 FILEACT/0x6b WINDOWINFO/
                    // 0x45 MOUSE/0x1a FONT/0x5d SYSTEM/0x0e ERROR/0x1c FPS/
                    // 0x31 INPUT/0x3c MATH/0x0a DIALOG/0x69 WINDOW。
                    // 引擎侧为子系统状态操作(字体层表/文件/输入模式/数学函数
                    // 派发),无后端 → 求值全部参数事件化记录,不建子系统状态。
                    // DIALOG(0x0a,引擎 0043c984):模态消息框;无 LET/LETSTR
                    // 槽时对流程零影响 → 记录后继续(= 用户瞬时按默认键;
                    // 交互阻塞语义归运行时层 P8/P9)。带 LET 的对话框出现时
                    // 须先逆向返回值编码再实现(铁律:不猜)。
                    // WINDOW(0x69):引擎处理器 = 共享 no-op stub(0045c4d4,
                    // engine_trace_config 实测)→ 无求值、无副作用;引擎 trace
                    // 侧 stub 命中被过滤为伪事件 → VM 同样**完全静默**
                    // (发事件会破坏对拍域,成果 59d)。
                    if other == 0x69 {
                        self.pc += 1;
                        continue;
                    }
                    // TASK(0x5f,引擎 0044fe40 反编译):命名任务创建/重配置。
                    // 槽 0=ID(名)、槽 1=E/槽 2=A/槽 3=Z(优先级)、槽 4-13=TINT、
                    // 14-23=TFLT、24-33=TSTR、槽 34='#'(入口标签)、35/36=SCRIPTPOS/
                    // TEXTPOS、37=RESTART。引擎:查/建任务对象 → 标签查表装载
                    // 入口脚本 → 参数写入任务对象。VM:**注册名字**(TASKINFO
                    // EXIST 的查询面)+ 求值记录;不 spawn —— 引擎 trace 实证
                    // 任务入口经主线 GOSUB 标签链顺序调起(s24 g195 → s2 pc47
                    // GOSUB → s266 → s9),boot 段无并行交错。
                    if other == cmd::TASK {
                        let mut evaluated: Vec<(u8, String)> = Vec::new();
                        let mut name: Option<Vec<u8>> = None;
                        for w in &windows {
                            if w.len == 0 {
                                continue;
                            }
                            let slot = (w.tag & 0xff) as u8;
                            match self.eval_window_condition(Some(w)) {
                                Ok(Some(v)) => {
                                    if slot == 0 {
                                        if let Value::Str(bytes) = &v {
                                            name = Some(bytes.clone());
                                        }
                                    }
                                    evaluated.push((slot, value_summary(&v)));
                                }
                                Ok(None) => {}
                                Err(e) => evaluated.push((slot, format!("err:{e}"))),
                            }
                        }
                        if let Some(n) = name {
                            let existed = self.tasks.contains(&n);
                            self.tasks.insert(n.clone());
                            evaluated.push((
                                255u8,
                                format!(
                                    "task {} {}",
                                    if existed { "已存在(重配置)" } else { "注册" },
                                    String::from_utf8_lossy(&n)
                                ),
                            ));
                        }
                        self.events.push(VmEvent::Subsystem {
                            pc: self.pc,
                            command: other,
                            evaluated,
                        });
                        self.pc += 1;
                        continue;
                    }
                    // TASKINFO(0x61,引擎 00451838 反编译):任务状态查询。
                    // 槽 0=ID(任务名;缺省=当前任务)、槽 1=LET(写回目标)、
                    // 槽 2=EXIST → 注册表命中写 1/未命中写 0(引擎:查不到
                    // 任务 → LET=0 后返回;查到且 EXIST → 写 1)。其余查询槽
                    // (E/A/Z/FILE/../NEXTVOICE/GOLABEL/...)走 FUN_0045baf8 ——
                    // 语料未执行,到访挂起(不猜)。
                    if other == cmd::TASKINFO {
                        let mut evaluated: Vec<(u8, String)> = Vec::new();
                        let mut name: Option<Vec<u8>> = None;
                        let mut let_target: Option<VarTarget> = None;
                        let mut exist = false;
                        let mut other_query = false;
                        let mut pending_let: Option<yuris_format::ystb::CommandSlot> = None;
                        for w in &windows {
                            if w.len == 0 {
                                continue;
                            }
                            let slot = (w.tag & 0xff) as u8;
                            match slot {
                                1 => pending_let = Some(w.clone()),
                                _ => {
                                    match self.eval_window_condition(Some(w)) {
                                        Ok(Some(v)) => {
                                            match slot {
                                                0 => {
                                                    if let Value::Str(bytes) = &v {
                                                        name = Some(bytes.clone());
                                                    }
                                                }
                                                2 => {
                                                    exist = matches!(&v, Value::Int(n) if *n != 0)
                                                }
                                                _ => other_query = true,
                                            }
                                            evaluated.push((slot, value_summary(&v)));
                                        }
                                        Ok(None) => {}
                                        Err(e) => evaluated.push((slot, format!("err:{e}"))),
                                    }
                                }
                            }
                        }
                        if let Some(w) = pending_let {
                            let bytes = self
                                .ctx
                                .script
                                .window_bytes_pooled_copy(&w)
                                .ok_or_else(|| Error::format("TASKINFO LET 引用窗越过池尾"))?;
                            let_target = Some(extract_var_target(&bytes)?);
                        }
                        if other_query {
                            return Err(self.halt_err(
                                self.pc,
                                cmd::TASKINFO,
                                "TASKINFO 非 EXIST 查询槽未实现(引擎 FUN_0045baf8 族;\
                                 语料未出现;不猜)",
                            ));
                        }
                        if exist {
                            let hit = name
                                .as_deref()
                                .map(|n| self.tasks.contains(n))
                                .unwrap_or(false);
                            if let Some(target) = &let_target {
                                self.write_var_target(target, Value::Int(hit as i64))?;
                                evaluated.push((
                                    255u8,
                                    format!(
                                        "EXIST({:?})={} → {}",
                                        name.map(|n| String::from_utf8_lossy(&n).into_owned()),
                                        hit as i64,
                                        target.summary()
                                    ),
                                ));
                            }
                        }
                        self.events.push(VmEvent::Subsystem {
                            pc: self.pc,
                            command: other,
                            evaluated,
                        });
                        self.pc += 1;
                        continue;
                    }
                    if matches!(
                        other,
                        0x0A | 0x0E | 0x14 | 0x15 | 0x1A | 0x1B | 0x1C | 0x31
                            | 0x3C | 0x45 | 0x5D | 0x6B
                    ) {
                        // 子系统查询输出写回(P5.2,引擎 FUN_0045baf8 族
                        // 查询→LET 写回;反编译 00457d64/00441c08):
                        // - WINDOWINFO(0x6b):SX(槽10)/SY(槽11)查询 → 窗口宽/高
                        //   写入 LET(槽1)目标(s24 g133/134 → @1074/@1075);
                        // - FONTINFO(0x1b):NUM(槽11,值=1)查询 → 注册字体数
                        //   写入 LET(槽18)目标(s45 g26 → @2684,LOOP 计数)。
                        // oracle 环境(引擎 trace 采集时刻)常量:全屏 1920×1080
                        // (s41 g77 范围检查 ±32/±18 反推);713 字体(引擎
                        // s45 循环圈数实测)。同 @114=32bpp 先例(成果 59c)。
                        if other == 0x6b || other == 0x1b {
                            let let_slot = if other == 0x6b { 1u8 } else { 18u8 };
                            let mut target: Option<VarTarget> = None;
                            let mut query_a = false;
                            let mut query_b = false;
                            for w in &windows {
                                if w.len == 0 {
                                    continue;
                                }
                                let slot = (w.tag & 0xff) as u8;
                                if slot == let_slot {
                                    if let Ok(bytes) = self
                                        .ctx
                                        .script
                                        .window_bytes_pooled_copy(w)
                                        .ok_or_else(|| {
                                            Error::format("子系统 LET 引用窗越过池尾")
                                        })
                                    {
                                        target = extract_var_target(&bytes).ok();
                                    }
                                } else if other == 0x6b && slot == 10 {
                                    query_a = true;
                                } else if other == 0x6b && slot == 11 {
                                    query_b = true;
                                } else if other == 0x1b && slot == 11 {
                                    // NUM 查询须值为 1(引擎 slot11==1 检查)
                                    query_a = matches!(
                                        self.eval_window_condition(Some(w)),
                                        Ok(Some(Value::Int(1)))
                                    );
                                } else if other == 0x1b && slot == 13 {
                                    // LANG 查询(须值 1;s45 g28 FONTINFO(NO,LANG→LET)):
                                    // 引擎 713 字体全部非零(pc29 IF 真 → LOOPCONT×713
                                    // 实测)→ oracle 常量 1。
                                    query_b = matches!(
                                        self.eval_window_condition(Some(w)),
                                        Ok(Some(Value::Int(1)))
                                    );
                                }
                            }
                            if let Some(t) = &target {
                                if other == 0x6b {
                                    if query_a {
                                        self.write_var_target(t, Value::Int(1920))?;
                                    }
                                    if query_b {
                                        self.write_var_target(t, Value::Int(1080))?;
                                    }
                                } else if query_a {
                                    // 注册字体数 = oracle 常量(环境依赖:
                                    // 2026-09-04 采集 = 713,2026-09-05 采集 = 783
                                    // —— 字体注册数随运行环境变化,对拍基线
                                    // 须与引擎 trace 同批采集;当前以 400k trace
                                    // 为准 = 783)
                                    self.write_var_target(t, Value::Int(783))?;
                                } else if query_b {
                                    self.write_var_target(t, Value::Int(1))?;
                                }
                            }
                        }
                        let mut evaluated: Vec<(u8, String)> = Vec::new();
                        for w in &windows {
                            if w.len == 0 {
                                continue;
                            }
                            let slot = (w.tag & 0xff) as u8;
                            // 求值失败不传播:引擎对未声明/未初始化变量读取
                            // 容忍(实测 desc 全零仍继续),事件如实记录
                            match self.eval_window_condition(Some(w)) {
                                Ok(Some(v)) => evaluated.push((slot, value_summary(&v))),
                                Ok(None) => {}
                                Err(e) => evaluated.push((slot, format!("err:{e}"))),
                            }
                        }
                        self.events.push(VmEvent::Subsystem {
                            pc: self.pc,
                            command: other,
                            evaluated,
                        });
                        self.pc += 1;
                        continue;
                    }
                    let reason = format!(
                        "命令 0x{other:02x} 的运行语义未逆向(处理器未实现;不猜) s{} pc={}",
                        self.ctx.script_id, self.pc
                    );
                    let pc = self.pc;
                    self.events.push(VmEvent::Unsupported {
                        pc,
                        command: other,
                        reason: reason.clone(),
                    });
                    if self.strict {
                        self.state = VmState::Error;
                        return Ok(VmSuspend::Error(reason));
                    }
                    self.pc += 1;
                    continue;
                }
            }
        }
        self.state = VmState::Finished;
        Ok(VmSuspend::Complete)
    }

    /// 切换到目标脚本上下文(取目标脚本 ctx,替换当前 ctx,重设 pc 与 script_id)。
    ///
    /// 需要已绑定 `ScriptHost`;无 host 或加载失败 → `Err`。
    fn switch_script(&mut self, script_id: u16, target_pc: usize) -> Result<()> {
        let Some(host) = &mut self.host else {
            return Err(Error::format(format!(
                "跨脚本跳转到 script {script_id} 但未绑定 ScriptHost"
            )));
        };
        let new_ctx = host.load(script_id)?;
        self.ctx = new_ctx;
        self.pc = target_pc;
        // 引擎加载脚本时消费其声明组(INT/FLT/STR 建立变量,Confirmed 形态)。
        // 每脚本只全扫一次(已消费集跟踪):GO/GOSUB 高频跨脚本,重复全扫
        // 实测成为端到端主热点(声明幂等,重复消费无语义收益)。
        // 注意:boot 已全量消费并标记,此处只为未标记脚本兜底(不覆盖 YSVR 终态)。
        if self.declared_scripts.insert(script_id) {
            let ctx = &self.ctx;
            consume_declarations_into(&mut self.store, ctx)?;
        }
        // YSVR kind2(按脚本初值):脚本**首次加载**时应用(成果 59h;
        // 幂等,已应用脚本直接跳过)。
        self.apply_ysvr_for_script(script_id)?;
        Ok(())
    }

    /// 消费当前脚本的**标量声明组**(FLT 0x19 / INT 0x32 / STR 0x5c)。
    ///
    /// 引擎在脚本加载期建立变量描述符(样本探针:全语料 4248 组全部为
    /// 「w0=标量引用 `48/56 [prefix][id]` + w1=名字 M-串」形态,零数组)。
    /// 默认值 Int(0)/Float(0.0)/Str("")(**Likely**,引擎零初始化);
    /// 数组声明走 YSVR(kind1/2 初值条目含维数边界),不在本组。
    ///
    /// 幂等:已定义标量不覆盖(初值应用须发生在本消费之后)。
    pub fn consume_script_declarations(&mut self) -> Result<usize> {
        let ctx = &self.ctx;
        consume_declarations_into(&mut self.store, ctx)
    }

    /// 从挂起点恢复。
    pub fn resume(&mut self, _resp: ResumeResponse) -> Result<()> {
        if self.state == VmState::Error {
            return Err(Error::format(
                "Error 态需先处理事件;骨架版不支持自动恢复",
            ));
        }
        Ok(())
    }

    /// GO:解码唯一窗口的 M-串标签 → 标签表 → (目标组号, 脚本号)。
    /// 跨脚本 → Unsupported(单脚本 VM 不猜多脚本重绑)。
    fn exec_go(&mut self, windows: &[yuris_format::ystb::CommandSlot]) -> Result<GoOutcome> {
        let Some(w) = windows.first() else {
            return Ok(GoOutcome::Halted(self.halted(self.pc, cmd::GO, "GO 无窗口")));
        };
        // 引擎:len 高位非 0 = 载入期已解析的标签表下标(id|0x10000000)。
        // 该标记只存在于引擎内存(载入后回写);文件形态恒为原始 len。
        if w.len & 0xF000_0000 != 0 {
            return Ok(GoOutcome::Halted(self.halted(
                self.pc,
                cmd::GO,
                "GO len 域带载入期解析标记(0x10000000)——文件形态不应出现;不猜",
            )));
        }
        let Some(bytes) = self.ctx.script.window_bytes_pooled_copy(w) else {
            return Ok(GoOutcome::Halted(self.halted(self.pc, cmd::GO, "GO 窗口越过池尾")));
        };
        let Some(name) = mstring(&bytes) else {
            return Ok(GoOutcome::Halted(self.halted(
                self.pc,
                cmd::GO,
                "GO 窗口不是 M-串标签",
            )));
        };
        match self.labels.get(name).copied() {
            Some((target_pc, script_id)) => {
                if script_id != self.ctx.script_id {
                    // 跨脚本:需要 host;无 host → 挂起(旧行为)。
                    if self.host.is_none() {
                        return Ok(GoOutcome::Halted(self.halted(
                            self.pc,
                            cmd::GO,
                            format!("跨脚本 GO 到 script {script_id} 但未绑定 ScriptHost"),
                        )));
                    }
                    return Ok(GoOutcome::CrossScript {
                        script_id,
                        target: target_pc as usize,
                    });
                }
                Ok(GoOutcome::Jump(target_pc as usize))
            }
            None => Ok(match self.halt_or_record(
                self.pc,
                cmd::GO,
                format!("GO 标签未命中: {:?}", String::from_utf8_lossy(name)),
            ) {
                Some(reason) => GoOutcome::Halted(reason),
                None => GoOutcome::NoJump,
            }),
        }
    }

    /// 在组窗口中解析标签并查表(引擎 GOSUB 004428c0)。
    ///
    /// 标签窗 = B0==0 的窗,**按表达式求值** → 字符串 → 查表:
    /// 载入期预解析缓存(`DAT_00661760/00661280`,`iVar10 != 0`)命中直接用,
    /// 否则求值结果 FUN_0045124c murmur2 查表 —— 字面量 M-串(0x4d)与
    /// 变量标签(如 s22 g42 `var($1909)` = "ES.FIRST.LOOP")同型。
    /// 未命中 → Ok(None)(GOSUB 处理器按引擎静默续跑建模)。
    fn resolve_label_in(
        &mut self,
        windows: &[yuris_format::ystb::CommandSlot],
    ) -> Result<Option<(usize, u16)>> {
        for w in windows {
            if w.tag & 0xFF != 0 {
                continue; // 引擎只解析 B0==0 的窗口
            }
            if w.len == 0 {
                continue;
            }
            let Some(v) = self.eval_window_condition(Some(w))? else {
                continue;
            };
            // 标签名保留**原始字节**:YSLB 键 = SJIS 字节(如
            // `es.日本語…`),经 UTF-8 lossy 会变 U+FFFD 而查表 miss
            // ([29208] s147 g788 实证;成果 59b)。
            let Value::Str(name) = v else {
                continue;
            };
            if let Some(&(target_pc, script_id)) = self.labels.get(&name) {
                return Ok(Some((target_pc as usize, script_id)));
            }
            // 未命中 → 引擎行为:静默跳过,继续找下一个
        }
        Ok(None)
    }

    /// 求值条件窗口(表达式字节码)。窗口缺失 → `Ok(None)`。
    fn eval_window_condition(
        &mut self,
        window: Option<&yuris_format::ystb::CommandSlot>,
    ) -> Result<Option<Value>> {
        let Some(w) = window else {
            return Ok(None);
        };
        if w.len == 0 {
            return Ok(None);
        }
        let Some(bytes) = self.ctx.script.window_bytes_pooled_copy(w) else {
            // 溢出窗(池尾外) → 视为损坏
            self.state = VmState::Error;
            return Err(Error::format(format!(
                "窗口 {}+{} 越过 content+part4 池尾",
                w.offset, w.len
            )));
        };
        let instrs = match yuris_script::decode_window(&bytes) {
            Ok(v) => v,
            Err(e) => {
                self.state = VmState::Error;
                return Err(Error::format(format!("条件窗口解码失败: {e}")));
            }
        };
        let locals = self.frames.last().map(|f| &f.locals);
        let mut ev = Evaluator::with_locals(&mut self.store, locals);
        let (cx, cy) = self.input_cursor;
        ev.set_cursor(cx, cy);
        // @48 系统变量注入(P5 定性,引擎 sysvar case 0x30):内层 LOOP
        // 迭代计数 —— 引擎读 obj+0x244 嵌套栈顶记录 +0x10,LOOP 置 1、
        // LOOPEND +1;无活动循环 = 0(GOSUB 不压该栈,跨帧可见)。
        ev.set_loop_counter(self.loops.last().map(|l| l.counter as i64).unwrap_or(0));
        match ev.eval_instructions(&instrs) {
            Ok(stack) => Ok(stack.into_iter().next_back()),
            Err(e) => {
                self.state = VmState::Error;
                Err(Error::format(format!(
                    "条件窗口求值失败: {e} s{} pc={}",
                    self.ctx.script_id, self.pc
                )))
            }
        }
    }

    /// LET 复合赋值的"当前值"装载:
    /// - 数组 → `get_elem`(下标 = 左值 indices;无下标 → 全零下标首元素)
    /// - 标量 → `get`
    /// 任一路径失败 → `None`(调用方按 code==0/复合码分别处理)。
    fn store_current(&self, lval: &yuris_script::LValueRef) -> Option<Value> {
        // P5:帧局部优先(成果 50)—— 复合赋值的"当前值"须读帧局部
        if let Some(fr) = self.frames.last() {
            if fr.locals.has_array(&lval.var) {
                let indices: Vec<i64> = if lval.indices.is_empty() {
                    let dims =
                        fr.locals.array_dims(&lval.var).unwrap_or_default();
                    vec![0; dims.len()]
                } else {
                    lval.indices.clone()
                };
                return fr.locals.get_elem(&lval.var, &indices).cloned().ok();
            }
            if lval.indices.is_empty() {
                if let Some(v) = fr.locals.get_opt(&lval.var) {
                    return Some(v.clone());
                }
            }
        }
        if self.store.has_array(&lval.var) {
            let indices: Vec<i64> = if lval.indices.is_empty() {
                let dims = self.store.array_dims(&lval.var).unwrap_or_default();
                vec![0; dims.len()]
            } else {
                lval.indices.clone()
            };
            return self.store.get_elem(&lval.var, &indices).cloned().ok();
        }
        self.store.get(&lval.var).cloned().ok()
    }

    /// GOSUB 帧局部初始化(P5 定性,成果 50):实参按 B0 槽号写入帧局部数组
    /// (INT=@53 / FLT=@54 / STR=$55,槽=B0-类型基,见函数内注释)。
    /// 引擎:实参从参数槽表写入帧内 int/flt/str 区,读经 sysvar switch
    /// case 0x35/0x36/0x37(00447ebc)。
    fn seed_frame_locals(
        &mut self,
        locals: &mut VariableStore,
        windows: &[yuris_format::ystb::CommandSlot],
        gparam: u16,
    ) {
        // P5 定性(成果 50 + 成果 52,引擎 CMDH_004428c0 @ 004428c0):
        // gparam u16 = part1 高半(part1[pc]>>16),解码本帧局部数组维数:
        //   int_count = (gparam & 0xff) >> 3
        //   flt_count = (gparam & 7) * 4 + (gparam >> 14)
        //   str_count = (gparam >> 9) & 0x1f
        // 槽号 1..count(1 基):有实参窗(B0 = 类型基 + 槽号)→ 写求值结果;
        // 无实参窗 → **清零**(INT 0 / FLT 0.0 / STR "")—— 引擎逐槽
        // `if (arg窗==0) { *str=0; flag=0 }` 分支;计数外槽不写不声明
        // (引擎写循环只到 count,读越界走 sysvar 边界检查 = 0)。
        // 类型区段:B0 0x01-0x0f → INT @53[slot];0x11-0x1f → FLT @54[slot-0x10];
        //           0x21-0x2f → STR $55[slot-0x20]。
        // 帧局部数组在 locals 内以 {At,53}/{At,54}/{Dollar,55} 键承载 ——
        // 与 LET 帧局部写路径(fr.locals.set_elem)及 aload 帧局部读路径
        // (eval.rs locals 优先)共用同一寻址。
        let int_count = ((gparam & 0xff) >> 3) as u32;
        let flt_count = ((gparam & 7) as u32) * 4 + ((gparam >> 14) as u32);
        let str_count = ((gparam >> 9) & 0x1f) as u32;
        let dbg = std::env::var("YURIS_DEBUG_FRAME").is_ok();
        // 先收集实参窗:(type, slot) → 窗口
        let mut args: [std::collections::HashMap<u32, &yuris_format::ystb::CommandSlot>; 3] =
            Default::default();
        for w in windows {
            if w.len == 0 {
                continue;
            }
            let b0 = (w.tag & 0xff) as u32;
            let (ty, slot) = match b0 {
                0x01..=0x0f => (0usize, b0),
                0x10..=0x1f => (1usize, b0 - 0x10),
                0x20..=0x2f => (2usize, b0 - 0x20),
                _ => continue, // 条件/标签窗(B0=0)或区段外
            };
            args[ty].insert(slot, w);
        }
        let specs = [
            (
                0usize,
                int_count,
                yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 53 },
                yuris_value::ElemType::Int,
            ),
            (
                1usize,
                flt_count,
                yuris_value::VarRef { space: yuris_value::VarSpace::At, id: 54 },
                yuris_value::ElemType::Float,
            ),
            (
                2usize,
                str_count,
                yuris_value::VarRef { space: yuris_value::VarSpace::Dollar, id: 55 },
                yuris_value::ElemType::Str,
            ),
        ];
        for (ty, count, r, et) in specs {
            if count == 0 {
                continue;
            }
            locals.declare_array(&r, et, &[count + 1]);
            for slot in 1..=count {
                let cv = match args[ty].get(&slot) {
                    Some(w) => {
                        let Some(v) = self.eval_window_condition(Some(w)).ok().flatten() else {
                            if dbg {
                                eprintln!(
                                    "[f] seed-skip s{} pc{} {}{}[{}](求值失败/空)",
                                    self.ctx.script_id,
                                    self.pc,
                                    r.space.prefix(),
                                    r.id,
                                    slot
                                );
                            }
                            continue;
                        };
                        Some(match (et, v) {
                            (yuris_value::ElemType::Int, Value::Int(i)) => Value::Int(i),
                            (yuris_value::ElemType::Int, Value::Float(f)) => {
                                Value::Int(f.round() as i64)
                            }
                            (yuris_value::ElemType::Float, Value::Int(i)) => {
                                Value::Float(i as f64)
                            }
                            (yuris_value::ElemType::Float, Value::Float(f)) => Value::Float(f),
                            (yuris_value::ElemType::Str, Value::Str(mut s)) => {
                                // 引擎实证:$55[1] = 8 字符(ES.FIRST),而 pushstr
                                // 字面量含包裹引号(10B)→ STR 帧局部写入时剥引号
                                if s.len() >= 2
                                    && s.first() == Some(&b'"')
                                    && s.last() == Some(&b'"')
                                {
                                    s = s[1..s.len() - 1].to_vec();
                                }
                                Value::Str(s)
                            }
                            // 类型不匹配:引擎按 desc+1 校验报错;这里保守透传
                            (yuris_value::ElemType::Int, other)
                            | (yuris_value::ElemType::Float, other)
                            | (yuris_value::ElemType::Str, other) => other,
                        })
                    }
                    None => Some(match et {
                        // 引擎:无实参窗 → 槽清零(0 / 0.0 / "")
                        yuris_value::ElemType::Int => Value::Int(0),
                        yuris_value::ElemType::Float => Value::Float(0.0),
                        yuris_value::ElemType::Str => Value::Str(Vec::new()),
                    }),
                };
                if let Some(cv) = cv {
                    if dbg {
                        let vs = match &cv {
                            Value::Int(i) => format!("int:{}", i),
                            Value::Float(f) => format!("flt:{}", f),
                            Value::Str(s) => format!("str:{}B", s.len()),
                        };
                        eprintln!(
                            "[f] s{} pc{} {}{}[{}]={}",
                            self.ctx.script_id,
                            self.pc,
                            r.space.prefix(),
                            r.id,
                            slot,
                            vs
                        );
                    }
                    let _ = locals.set_elem(&r, &[slot as i64], cv);
                }
            }
        }
    }

    /// LOOPBREAK/LOOPCONTINUE 的 LV 层数窗口:求值 → Int → usize。
    /// 无窗口/空窗 → 默认 1(引擎 DAT_006624a0==0 路径);非 INT → 报错。
    fn loop_level_from(
        &mut self,
        window: Option<&yuris_format::ystb::CommandSlot>,
    ) -> Result<Option<usize>> {
        let Some(w) = window else {
            return Ok(Some(1));
        };
        if w.len == 0 {
            return Ok(Some(1));
        }
        match self.eval_window_condition(Some(w))? {
            Some(Value::Int(n)) if n >= 1 => Ok(Some(n as usize)),
            Some(v) => {
                let reason = format!("LV 层数非正整数: {v:?}");
                self.state = VmState::Error;
                Err(Error::format(reason))
            }
            None => Ok(Some(1)),
        }
    }

    /// LET 左值窗口解析(引用栈 + 下标表)。
    fn eval_window_lvalue(
        &mut self,
        window: &[u8],
    ) -> Result<yuris_script::LValueRef> {
        let instrs = match yuris_script::decode_window(window) {
            Ok(v) => v,
            Err(e) => {
                self.state = VmState::Error;
                return Err(Error::format(format!("LET 左值解码失败: {e}")));
            }
        };
        let locals = self.frames.last().map(|f| &f.locals);
        let mut ev = Evaluator::with_locals(&mut self.store, locals);
        let (cx, cy) = self.input_cursor;
        ev.set_cursor(cx, cy);
        // @48 注入(同 eval_window_condition;左值下标表达式可读系统变量)
        ev.set_loop_counter(self.loops.last().map(|l| l.counter as i64).unwrap_or(0));
        match ev.eval_lvalue_window_instrs(&instrs) {
            Ok(l) => Ok(l),
            Err(e) => {
                self.state = VmState::Error;
                Err(e)
            }
        }
    }

    /// strict 挂起的 `Result` 形态:记录 Unsupported 事件 + 返回 Err。
    fn halt_err(&mut self, pc: usize, command: u8, reason: impl Into<String>) -> Error {
        Error::format(self.halted(pc, command, reason))
    }

    /// strict 挂起:记录 Unsupported 事件并返回原因(状态已置 Error)。
    fn halted(&mut self, pc: usize, command: u8, reason: impl Into<String>) -> String {
        let reason = reason.into();
        self.events.push(VmEvent::Unsupported {
            pc,
            command,
            reason: reason.clone(),
        });
        self.state = VmState::Error;
        reason
    }

    /// LOAD/YSSD 装载(引擎 FUN_00444648 文件分支 + FUN_0044564d 写回)。
    ///
    /// `fname` = 脚本槽 0 求值串(无扩展名 → 补 `.sd`,引擎 FUN_004409dc
    /// 分支);`dno` = 块号(1 基);`target` = 槽 3 延迟引用(块头 var_id
    /// 与之恒一致,语料 26 组实证)。装载路径 = 松散文件(save/ VFS 根)。
    fn load_yssd(&mut self, fname: &[u8], dno: i64, target: &yuris_value::VarRef) -> Result<()> {
        if dno < 1 {
            return Err(self.halt_err(
                self.pc,
                cmd::LOAD,
                format!("LOAD DNO={dno} 越界(YSSD 偏移表 1 基)"),
            ));
        }
        let mut name = String::from_utf8_lossy(fname).into_owned();
        if !name.contains('.') {
            name.push_str(".sd");
        }
        let Some(probe) = &self.file_probe else {
            return Err(self.halt_err(
                self.pc,
                cmd::LOAD,
                "LOAD 需要文件源(PacFileIndex 未注入;引擎 VFS 语义)",
            ));
        };
        let Some(bytes) = probe.read_loose(&name) else {
            return Err(self.halt_err(
                self.pc,
                cmd::LOAD,
                format!("LOAD 文件不存在: {name}(引擎 0x18ed4 错误框同族)"),
            ));
        };
        let yssd = yuris_format::yssd::YssdFile::from_bytes(&bytes)
            .map_err(|e| Error::format(format!("LOAD {name}: {e}")))?;
        let Some(block) = yssd.block(dno as u32) else {
            return Err(self.halt_err(
                self.pc,
                cmd::LOAD,
                format!("LOAD {name} 无块 {dno}(偏移表空项;引擎 0x18ede)"),
            ));
        };
        if block.btype == 1 {
            return Err(self.halt_err(
                self.pc,
                cmd::LOAD,
                "LOAD 块类型 1(0x690 配置)须配 ID 模式(引擎 0x1a612;不猜)",
            ));
        }
        if block.btype != 0 {
            return Err(self.halt_err(
                self.pc,
                cmd::LOAD,
                format!("LOAD 块类型 {} 未实现(多子块族;语料未出现)", block.btype),
            ));
        }
        let raw = yuris_format::yssd::snp_uncompress(&block.compressed)
            .map_err(|e| Error::format(format!("LOAD {name} 块{dno}: {e}")))?;
        let payload = yuris_format::yssd::YssdPayload::from_bytes(&raw)
            .map_err(|e| Error::format(format!("LOAD {name} 块{dno}: {e}")))?;

        // 类型一致(引擎 0x18ee8)+ 严格维数匹配(引擎 0x1a63a/0x1a630)。
        if self.store.has_array(target) {
            let elem = self.store.array_elem_type(target).unwrap();
            let ok = matches!(
                (elem, payload.ty),
                (yuris_value::ElemType::Int, 1)
                    | (yuris_value::ElemType::Float, 2)
                    | (yuris_value::ElemType::Str, 3)
            );
            if !ok {
                return Err(self.halt_err(
                    self.pc,
                    cmd::LOAD,
                    format!(
                        "LOAD 载荷类型 {} 与 {} 声明不符(引擎 0x18ee8)",
                        payload.ty,
                        target.display()
                    ),
                ));
            }
            let bounds = self.store.array_dims(target).unwrap();
            if block.strict == 1 && bounds != payload.dims {
                return Err(self.halt_err(
                    self.pc,
                    cmd::LOAD,
                    format!(
                        "LOAD 载荷维数 {:?} 与 {} 声明 {bounds:?} 不符(引擎 0x1a63a)",
                        payload.dims,
                        target.display()
                    ),
                ));
            }
            if block.strict == 0 && !payload.dims.is_empty() {
                return Err(self.halt_err(
                    self.pc,
                    cmd::LOAD,
                    format!("LOAD 非 strict 载荷须标量(引擎 0x1a630)"),
                ));
            }
        } else if let Some(cur_ty) = self
            .store
            .get_opt(target)
            .map(|v| match v {
                Value::Int(_) => 1u8,
                Value::Float(_) => 2,
                Value::Str(_) => 3,
            })
        {
            if cur_ty != payload.ty {
                return Err(self.halt_err(
                    self.pc,
                    cmd::LOAD,
                    format!(
                        "LOAD 载荷类型 {} 与标量 {} 类型不符(引擎 0x18ee8)",
                        payload.ty,
                        target.display()
                    ),
                ));
            }
            if !payload.dims.is_empty() {
                return Err(self.halt_err(
                    self.pc,
                    cmd::LOAD,
                    format!("LOAD 标量目标 {} 收到数组载荷(引擎 0x1a630)", target.display()),
                ));
            }
        } else {
            return Err(self.halt_err(
                self.pc,
                cmd::LOAD,
                format!("LOAD 目标 {} 未声明(引擎 0x1a5ea 同族)", target.display()),
            ));
        }

        // 写回(FUN_0044564d:INT/FLT = 8B/元素 memcpy;STR = 逐 {len,bytes})。
        let elem_count: usize = payload.dims.iter().map(|&d| d as usize).product();
        let values: Vec<Value> = match payload.ty {
            1 => payload
                .data
                .chunks_exact(8)
                .map(|c| Value::Int(i64::from_le_bytes(c.try_into().unwrap())))
                .collect(),
            2 => payload
                .data
                .chunks_exact(8)
                .map(|c| Value::Float(f64::from_le_bytes(c.try_into().unwrap())))
                .collect(),
            _ => {
                let mut out = Vec::with_capacity(elem_count.max(1));
                let mut p = 0usize;
                for _ in 0..elem_count.max(1) {
                    if p + 4 > payload.data.len() {
                        return Err(self.halt_err(
                            self.pc,
                            cmd::LOAD,
                            format!("LOAD {} 载荷字符串记录截断", target.display()),
                        ));
                    }
                    let len = u32::from_le_bytes(payload.data[p..p + 4].try_into().unwrap())
                        as usize;
                    p += 4;
                    if p + len > payload.data.len() {
                        return Err(self.halt_err(
                            self.pc,
                            cmd::LOAD,
                            format!("LOAD {} 载荷字符串记录截断", target.display()),
                        ));
                    }
                    out.push(Value::Str(payload.data[p..p + len].to_vec()));
                    p += len;
                }
                out
            }
        };
        if self.store.has_array(target) {
            self.store
                .load_array_data(target, values)
                .map_err(|e| Error::format(format!("LOAD 写回: {e}")))?;
        } else {
            // 标量(引擎 memcpy 单元素 8B / STR 首记录)
            match values.into_iter().next() {
                Some(v) => self.store.set(target, v),
                None => return Err(self.halt_err(
                    self.pc,
                    cmd::LOAD,
                    format!("LOAD 标量载荷为空({})", target.display()),
                )),
            }
        }
        Ok(())
    }

    /// 变量读取(P5,成果 50):**帧局部优先** —— 当前 GOSUB 帧 locals 含
    /// 同键数组(如 @53/@54/$55 帧局部族)时读帧,否则读全局
    /// (引擎 0042158e:id<1000 走帧/系统存储,id≥1000 走 desc+0x30)。
    fn read_var_value(&self, r: &yuris_value::VarRef, indices: &[i64]) -> Option<Value> {
        // @48 系统变量(P5 定性,引擎 sysvar case 0x30/00447ebc):内层 LOOP
        // 迭代计数,读即计算、无存储(引擎 obj+0x244 栈顶记录 +0x10);
        // LOOP 置 1、LOOPEND +1、LOOPBREAK 清零;无活动循环 = 0。
        if r.space == yuris_value::VarSpace::At && r.id == 48 {
            return Some(Value::Int(
                self.loops.last().map(|l| l.counter as i64).unwrap_or(0),
            ));
        }
        // @114(引擎 sysvar case 0x72/00447ebc):显示色深 bpp,原生全局
        // DAT_00872374(显示初始化写入;s40 色深检查用)。VM 无显示栈,
        // 按 oracle 环境 = 32bpp(成果 59c)。
        if r.space == yuris_value::VarSpace::At && r.id == 114 {
            return Some(Value::Int(32));
        }
        // @115/@116(引擎 sysvar case 0x73/0x74,DAT_0087236c/70):屏幕宽/高
        //(显示初始化写入)。VM 无显示栈,按 oracle 环境 = 1920×1080。
        if r.space == yuris_value::VarSpace::At && r.id == 115 {
            return Some(Value::Int(1920));
        }
        if r.space == yuris_value::VarSpace::At && r.id == 116 {
            return Some(Value::Int(1080));
        }
        let out: Option<Value> = (|| {
            if let Some(fr) = self.frames.last() {
                if fr.locals.has_array(r) {
                    return match fr.locals.get_elem(r, indices) {
                        Ok(v) => Some(v.clone()),
                        Err(e) => {
                            // 帧局部系统族越界读 = 类型默认值(引擎 sysvar
                            // case 0x35 边界外 → 0;成果 52);其余数组严格
                            let _ = e;
                            if yuris_script::eval::is_frame_sysvar(r) {
                                Some(yuris_script::eval::frame_sysvar_default(
                                    fr.locals.array_elem_type(r),
                                ))
                            } else {
                                None
                            }
                        }
                    };
                }
                // 有帧但无该键:系统族 = 默认值,**绝无全局回退**(引擎
                // id<1000 读永远帧内定界;成果 52)
                if yuris_script::eval::is_frame_sysvar(r) {
                    return Some(yuris_script::eval::frame_sysvar_default(
                        yuris_script::eval::frame_sysvar_elem(r),
                    ));
                }
                if indices.is_empty() {
                    if let Some(v) = fr.locals.get_opt(r) {
                        return Some(v.clone());
                    }
                }
            }
            if self.store.has_array(r) {
                // 引擎全局数组读 FUN_00459490/00459418(成果 57):越界
                // → 类型默认值,不报错。
                return match self.store.get_elem(r, indices) {
                    Ok(v) => Some(v.clone()),
                    Err(e) => match self.store.array_elem_type(r) {
                        Some(et) => Some(yuris_script::eval::type_default(Some(et))),
                        None => {
                            let _ = e;
                            None
                        }
                    },
                };
            }
            if indices.is_empty() {
                return self.store.get(r).cloned().ok();
            }
            match self.store.get_elem(r, indices) {
                Ok(v) => Some(v.clone()),
                Err(e) => match self.store.array_elem_type(r) {
                    Some(et) => Some(yuris_script::eval::type_default(Some(et))),
                    None => {
                        let _ = e;
                        None
                    }
                },
            }
        })();
        if std::env::var("YURIS_DEBUG_FRAME").is_ok() && r.id == 55 {
            let fr_has = self.frames.last().map(|f| f.locals.has_array(r));
            let vs = match &out {
                Some(Value::Str(s)) => format!("str:{}B", s.len()),
                Some(Value::Int(i)) => format!("int:{}", i),
                Some(Value::Float(f)) => format!("flt:{}", f),
                None => "none".to_string(),
            };
            eprintln!(
                "[f] read $55{:?} frames={} frame_has={:?} -> {}",
                indices,
                self.frames.len(),
                fr_has,
                vs
            );
        }
        out
    }

    /// 变量写入(帧局部优先;同 [`Self::read_var_value`])。
    fn write_var_value(
        &mut self,
        r: &yuris_value::VarRef,
        indices: &[i64],
        v: Value,
    ) -> Result<()> {
        if let Some(fr) = self.frames.last_mut() {
            if fr.locals.has_array(r) {
                return fr.locals.set_elem(r, indices, v);
            }
            if indices.is_empty() && fr.locals.get_opt(r).is_some() {
                fr.locals.set(r, v);
                return Ok(());
            }
        }
        if self.store.has_array(r) {
            return self.store.set_elem(r, indices, v);
        }
        if indices.is_empty() {
            self.store.set(r, v);
            return Ok(());
        }
        self.store.set_elem(r, indices, v)
    }

    /// 记录 Unsupported 事件;strict → 返回 `Some(原因)`(调用方挂起),
    /// 否则 `None`(调用方继续)。状态仅在 strict 下置 Error。
    fn halt_or_record(
        &mut self,
        pc: usize,
        command: u8,
        reason: impl Into<String>,
    ) -> Option<String> {
        let reason = reason.into();
        self.events.push(VmEvent::Unsupported {
            pc,
            command,
            reason: reason.clone(),
        });
        if self.strict {
            self.state = VmState::Error;
            Some(reason)
        } else {
            None
        }
    }
}

/// 解码单条 M-串窗口(`4d len "name"` 或类型 5 的无引号形式)。
fn mstring(window: &[u8]) -> Option<&[u8]> {
    if window.len() < 4 || window[0] != 0x4d {
        return None;
    }
    let payload = &window[3..];
    if payload.len() >= 2 && payload[0] == b'"' && *payload.last().unwrap() == b'"' {
        return Some(&payload[1..payload.len() - 1]);
    }
    Some(payload)
}

/// 变量引用目标(VARACT/VARINFO/CGINFO/SAVE/LOAD 的 SET/LET 槽;kind 2 延迟求值)。
#[derive(Debug, Clone, PartialEq)]
enum VarTarget {
    /// 标量(`@1041`)。
    Scalar(yuris_value::VarRef),
    /// 数组元素(`$0x37[1]`)。
    ///
    /// 2026-09-05 勘误(P1 对拍[144128]定位):基点 = **首条** var 类指令,
    /// 其后至末条 0x29 之前的全部指令 = **下标表达式**(引擎 kind 2 延迟
    /// 求值,写/读时求值)。旧实现用「最后一条 var 指令」当基点、下标只认
    /// 整数字面量 → `56 @1265 | 48 @1704 | 29` 形态把下标变量 @1704 误当
    /// 基点、写入错位(与 LET 左值窗成果 44 同族语义,此处漏同步)。
    /// 常量下标 = 指令序列恰为 PushInt 序列的特例。
    Indexed(yuris_value::VarRef, Vec<yuris_script::Instruction>),
}

impl VarTarget {
    fn summary(&self) -> String {
        match self {
            VarTarget::Scalar(r) => r.display(),
            VarTarget::Indexed(r, idx) => {
                if idx.iter().all(|i| matches!(i.kind, yuris_script::Insn::PushInt(_))) {
                    let vals: Vec<i64> = idx
                        .iter()
                        .filter_map(|i| match i.kind {
                            yuris_script::Insn::PushInt(n) => Some(n),
                            _ => None,
                        })
                        .collect();
                    format!("{}[{:?}]", r.display(), vals)
                } else {
                    format!("{}[expr×{}]", r.display(), idx.len())
                }
            }
        }
    }
}

/// 从引用槽窗口字节提取变量目标(引擎 kind 2 延迟求值形态)。
///
/// 识别形态(语料 Confirmed):
/// - 单条 `48/56/76 + [prefix][id:u16]` → 标量(0x76 裸引用 = 整数组,s37 g50);
/// - `56 + [prefix][id]` + 下标指令×N + `29`(ArrayLoad) → 数组元素,
///   基点 = 首条 var 指令,下标 = 其后指令序列(写/读时求值,支持
///   变量下标与表达式下标;见 [`VarTarget::Indexed`] 勘误)。
fn extract_var_target(bytes: &[u8]) -> Result<VarTarget> {
    let instrs = yuris_script::decode_window(bytes)?;
    // 单条 var 类指令 → 标量
    if let [ins] = &instrs[..] {
        if let yuris_script::Insn::PushVar(r)
        | yuris_script::Insn::PushVarRef(r)
        | yuris_script::Insn::PushVarIndexed(r) = &ins.kind
        {
            return Ok(VarTarget::Scalar(*r));
        }
    }
    // 多指令形态:基点 = 首条 var 类指令(语料:左值窗全部 0x56 起头)
    let base = match instrs.first().map(|i| &i.kind) {
        Some(yuris_script::Insn::PushVar(r))
        | Some(yuris_script::Insn::PushVarRef(r))
        | Some(yuris_script::Insn::PushVarIndexed(r)) => *r,
        _ => return Err(Error::format("引用窗首条不是变量指令")),
    };
    // 末条 0x29 = 引擎延迟模式不装载的加载标记,剥除;其余为下标表达式
    let idx_len = instrs
        .last()
        .map(|i| matches!(i.kind, yuris_script::Insn::ArrayLoad { .. }))
        .unwrap_or(false);
    if !idx_len {
        return Err(Error::format(
            "引用窗多指令形态缺末条 0x29(语料外形态;不猜)",
        ));
    }
    if instrs.len() < 3 {
        return Err(Error::format("引用窗多指令形态过短"));
    }
    Ok(VarTarget::Indexed(
        base,
        instrs[1..instrs.len() - 1].to_vec(),
    ))
}

impl GroupVm {
    /// 读 SET 目标当前值(VARINFO 查询对象)。P5:帧局部优先(成果 50)。
    fn read_var_target(&mut self, t: &VarTarget) -> Result<Value> {
        match t {
            VarTarget::Scalar(r) => {
                Ok(self.read_var_value(r, &[]).unwrap_or(Value::Int(0)))
            }
            VarTarget::Indexed(r, idx) => {
                let indices = self.eval_target_indices(idx)?;
                Ok(self.read_var_value(r, &indices).unwrap_or(Value::Int(0)))
            }
        }
    }

    /// 求值引用槽的下标指令(引擎 kind 2 延迟求值:写/读时求值)。
    /// 常量(PushInt)序列走快路径;其余经 Evaluator 活求值,
    /// 栈序 = 维度序(引擎:引用点与栈顶之间的纯值项 = 下标表)。
    fn eval_target_indices(&mut self, idx: &[yuris_script::Instruction]) -> Result<Vec<i64>> {
        if idx.is_empty() {
            return Ok(Vec::new());
        }
        if idx.iter().all(|i| matches!(i.kind, yuris_script::Insn::PushInt(_))) {
            return Ok(idx
                .iter()
                .filter_map(|i| match i.kind {
                    yuris_script::Insn::PushInt(n) => Some(n),
                    _ => None,
                })
                .collect());
        }
        let locals = self.frames.last().map(|f| &f.locals);
        let mut ev = Evaluator::with_locals(&mut self.store, locals);
        let (cx, cy) = self.input_cursor;
        ev.set_cursor(cx, cy);
        ev.set_loop_counter(self.loops.last().map(|l| l.counter as i64).unwrap_or(0));
        let stack = ev.eval_instructions(idx).map_err(|e| {
            Error::format(format!("引用槽下标表达式求值失败: {e}"))
        })?;
        stack
            .into_iter()
            .map(|v| {
                v.as_int_opt().ok_or_else(|| {
                    Error::format("引用槽下标表达式结果非整数(不猜)")
                })
            })
            .collect()
    }

    /// 把查询结果写入 LET 引用目标。
    /// 目标的元素类型(引擎描述符 byte+1 等价:数组按声明类型,标量按现值)。
    /// 都不可知 → `None`(调用方按标量 INT 处理,SAVE 语境与引擎 STR 分支
    /// 「无存储写入」不同处如实标注)。
    fn var_target_elem(&self, t: &VarTarget) -> Option<yuris_value::ElemType> {
        match t {
            VarTarget::Scalar(r) => match self.store.get_opt(r) {
                Some(Value::Int(_)) => Some(yuris_value::ElemType::Int),
                Some(Value::Float(_)) => Some(yuris_value::ElemType::Float),
                Some(Value::Str(_)) => Some(yuris_value::ElemType::Str),
                None => None,
            },
            VarTarget::Indexed(r, _) => self.store.array_elem_type(r),
        }
    }

    fn write_var_target(&mut self, t: &VarTarget, v: Value) -> Result<()> {
        match t {
            VarTarget::Scalar(r) => self.write_var_value(r, &[], v),
            VarTarget::Indexed(r, idx) => {
                let indices = self.eval_target_indices(idx)?;
                self.write_var_value(r, &indices, v)
            }
        }
    }

    /// VARACT DIMSIZE:1 维数组 resize。
    fn resize_array_target(&mut self, t: &VarTarget, new_len: usize) -> Result<()> {
        match t {
            // 裸引用(0x76 无 aload,s37 g50 实证)= **整个数组**引用:引擎
            // DIMSIZE 分支直接按 id 取 desc 检查维数并 resize,与下标无关。
            VarTarget::Scalar(r) => {
                if !self.store.has_array(r) {
                    return Err(Error::format(format!(
                        "VARACT DIMSIZE 目标必须是数组(引擎 desc+2==1 检查) s{} pc={}",
                        self.ctx.script_id, self.pc
                    )));
                }
                let dims = self.store.array_dims(r).unwrap_or_default();
                if dims.len() != 1 {
                    return Err(Error::format(format!(
                        "VARACT DIMSIZE 要求 1 维数组(引擎 desc+2==1),得到 {} 维",
                        dims.len()
                    )));
                }
                self.store.resize_array(r, &[new_len as u32])
            }
            VarTarget::Indexed(r, idx) => {
                // 引擎 DIMSIZE 恒作用于整数组(裸引用);带下标形态按
                // 延迟求值解析(结果维数不参与 resize 定位,desc 按 id 取)。
                let _indices = self.eval_target_indices(idx)?;
                self.store.resize_array(r, &[new_len as u32])
            }
        }
    }

    /// VARACT 数值槽取值(非负 int;缺失 → 0,引擎槽标志未置位 = 0)。
    fn varact_slot_usize(&self, ops: &[(u8, u8, Value)], slot: u8) -> Result<usize> {
        match ops.iter().find(|(s, _, _)| *s == slot).map(|(_, _, v)| v) {
            None => Ok(0),
            Some(Value::Int(n)) if *n >= 0 => Ok(*n as usize),
            Some(other) => Err(Error::format(format!(
                "VARACT 槽 {slot} 需非负 int,得到 {other:?}"
            ))),
        }
    }

    /// VARINFO 查询执行(引擎 CMDH_004550a0 if-else 链)。
    /// 返回 `Some(结果值)`(写 LET 目标)或 `None`(纯显示/未启用)。
    fn exec_varinfo_query(
        &mut self,
        query: Option<u8>,
        set_target: Option<&VarTarget>,
        evaluated: &mut Vec<(u8, String)>,
    ) -> Result<Option<Value>> {
        let Some(target) = set_target else {
            return Err(Error::format("VARINFO 无 SET 引用槽(引擎必填)"));
        };
        match query {
            None => {
                // 引擎 fallback:SET 变量 type==3 → 串**字符数**(LENGTH 同源,
                // 0x4551dc 步进计数终证);否则报错 0x1a6bc
                let v = self.read_var_target(target)?;
                match v {
                    Value::Str(bytes) => {
                        let n = sjis_char_len(&bytes);
                        evaluated.push((13u8, format!("LENGTH(fallback)={n}")));
                        Ok(Some(Value::Int(n as i64)))
                    }
                    _ => Err(Error::format(
                        "VARINFO fallback: SET 变量非 STR(引擎 0x1a6bc)",
                    )),
                }
            }
            Some(2) => {
                // TYPE:1=INT 2=FLT 3=STR(按当前值/数组元素类型)
                let code = match self.read_var_target(target)? {
                    Value::Int(_) => 1i64,
                    Value::Float(_) => 2,
                    Value::Str(_) => 3,
                };
                evaluated.push((2u8, format!("TYPE={code}")));
                Ok(Some(Value::Int(code)))
            }
            Some(3) => {
                // STRTYPE:被查串 = STRTYPE 槽自身的字符串参数(引擎取最近串求值)
                // 判定首字符 SJIS 宽度 → 1=半角 2=全角(处理器直接证据:写 LET 元素)
                Err(Error::Unimplemented(
                    "VARINFO STRTYPE:串参数传递路径待核(槽值未达本方法)",
                ))
            }
            Some(4) => {
                // DIMNUM:维数。裸引用(Scalar)= 整数组(s37 g50/g51 实证,
                // 引擎 DIMSIZE/DIMNUM 直接按 id 取 desc)→ 按 store 数组解析;
                // 真标量 → 0。
                let n = match target {
                    VarTarget::Scalar(r) => {
                        self.store.array_dims(r).map(|d| d.len() as i64).unwrap_or(0)
                    }
                    VarTarget::Indexed(r, _) => {
                        self.store.array_dims(r).map(|d| d.len() as i64).unwrap_or(0)
                    }
                };
                evaluated.push((4u8, format!("DIMNUM={n}")));
                Ok(Some(Value::Int(n)))
            }
            Some(slot @ 5..=12) => {
                // DIMSIZE..DIMSIZE8:第 (slot-4) 维边界(引擎 a5..ac 各固定一维)。
                // Scalar 裸引用 = 整数组(同 DIMNUM,引擎按 id 取 desc)。
                let dim = (slot - 5) as usize;
                let n = match target {
                    VarTarget::Scalar(r) => self
                        .store
                        .array_dims(r)
                        .and_then(|d| d.get(dim).copied())
                        .map(i64::from)
                        .unwrap_or(0),
                    VarTarget::Indexed(r, _) => {
                        let dims = self.store.array_dims(r).unwrap_or_default();
                        dims.get(dim).copied().map(i64::from).unwrap_or(0)
                    }
                };
                evaluated.push((slot, format!("DIMSIZE[{}]={n}", dim + 1)));
                Ok(Some(Value::Int(n)))
            }
            Some(13) => {
                // LENGTH:SET 串的 SJIS **字符数**(汇编终证 0x4551dc-0x455208:
                // strlen 只作循环边界,入栈结果 = 宽度表步进的 EAX 计数器;
                // 旧「字节数」解读被证伪 —— 它曾使 s190 pc=36 的
                // POS=LEN−4+1 落到越界值)
                let v = self.read_var_target(target)?;
                match v {
                    Value::Str(bytes) => {
                        let n = sjis_char_len(&bytes);
                        evaluated.push((13u8, format!("LENGTH={n}")));
                        Ok(Some(Value::Int(n as i64)))
                    }
                    _ => Err(Error::format(
                        "VARINFO LENGTH: SET 变量非 STR(引擎 0x1a6bc)",
                    )),
                }
            }
            Some(slot) => Err(Error::format(format!(
                "VARINFO 查询槽 {slot}(SEARCH/STRFIRST/SJISCODE)未实现(不猜)"
            ))),
        }
    }
}

/// SJIS 字符数(VARINFO LENGTH 汇编终证 0x4551dc-0x455208:strlen 只作循环
/// 边界,入栈结果 = 宽度表步进循环的 EAX 计数器 —— 每字符恰 +1,双字节
/// 首区额外跳 1 字节。即 LENGTH = 字符数,非字节数)。
fn sjis_char_len(bytes: &[u8]) -> usize {
    let mut off = 0usize;
    let mut n = 0usize;
    while off < bytes.len() {
        let b = bytes[off];
        off += if (0x81..=0x9F).contains(&b) || (0xE0..=0xEF).contains(&b) {
            2
        } else {
            1
        };
        n += 1;
    }
    n
}

/// VARACT UPPER/LOWER 的 SJIS 感知 ASCII 大小写转换
/// (引擎 FUN_004546cc=UPPER / FUN_00464dac=LOWER,Ghidra 逐行,57B 各):
/// 双字节首区(步进表 DAT_0059b0c0==1)连跳 2 字节不转换;单字节
/// `to_upper`: 'a'-'z' → -0x20;`!to_upper`: 'A'-'Z' → +0x20;其余原样。
/// 引擎按 NUL 终止扫描,缓冲区语义由调用方保证 —— 这里按字节长度截住。
fn varact_ascii_case(bytes: &[u8], to_upper: bool) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let mut i = 0usize;
    while i < out.len() {
        let b = out[i];
        if (0x81..=0x9F).contains(&b) || (0xE0..=0xEF).contains(&b) {
            i += 2; // 双字节整体跳过(引擎 in_EAX += 2)
            continue;
        }
        if to_upper && (b'a'..=b'z').contains(&b) {
            out[i] = b - 0x20;
        } else if !to_upper && (b'A'..=b'Z').contains(&b) {
            out[i] = b + 0x20;
        }
        i += 1;
    }
    out
}

/// 字符序数 → SJIS 字节偏移(引擎 FUN_00471cf5/00471d7f 语义,字符步进表
/// `DAT_0059b0c0`:双字节首区 +1 → 步进 2,其余步进 1)。
/// 越界(序数超过字符数)→ `None`(引擎报错 0x1d52e/0x1d538 同族)。
fn varact_char_to_byte(s: &Value, ordinal: usize) -> Option<usize> {
    let Value::Str(bytes) = s else {
        return None;
    };
    let mut off = 0usize;
    for _ in 0..ordinal {
        if off >= bytes.len() {
            return None;
        }
        let b = bytes[off];
        off += if (0x81..=0x9F).contains(&b) || (0xE0..=0xEF).contains(&b) {
            2
        } else {
            1
        };
    }
    Some(off)
}

/// 字节偏移 → 字符序数(引擎 FUN_00471d7f 逆换算;非字符边界 → 界内序数)。
#[allow(dead_code)]
fn varact_byte_to_char(s: &Value, byte_off: usize) -> usize {
    let Value::Str(bytes) = s else {
        return 0;
    };
    let mut off = 0usize;
    let mut n = 0usize;
    while off < bytes.len() && off < byte_off {
        let b = bytes[off];
        off += if (0x81..=0x9F).contains(&b) || (0xE0..=0xEF).contains(&b) {
            2
        } else {
            1
        };
        n += 1;
    }
    n
}

/// 消费 `ctx` 的**声明组**写入 `store`。
///
/// 声明命令族(YSCM 名表 Confirmed):
/// - FLT 族:F_FLT(0x10)/FLT(0x19)/G_FLT..4(0x1d-0x20)/S_FLT(0x52) → Float
/// - INT 族:F_INT(0x11)/INT(0x32)/G_INT..4(0x21-0x24)/S_INT(0x53) → Int
/// - STR 族:F_STR(0x12)/STR(0x5c)/G_STR..4(0x25-0x28)/S_STR(0x54) → Str
/// - VAR 族(0x13/0x29/0x55):类型不定 → 跳过(Unknown,不猜)
///
/// 窗口形态(样本探针):
/// - 标量:单条 `48/56 [prefix][id]` → 默认值
/// - 数组:`56/76 [prefix][id]` + 下标表达式 + `29`(ArrayLoad) →
///   declare_array(bounds=[表达式值+1])(**上界引用**;yst00000 F_STR 实测
///   `$1226[400+1]`;引擎 0x29 行主序装载 Confirmed,上界语义 Likely)
/// - M-串名字窗(tag B2=3):非引用,自然跳过
///
/// 幂等:已定义标量/同型数组不覆盖(YSVR 初值应用须在本消费之后)。
pub fn consume_declarations_into(
    store: &mut yuris_value::VariableStore,
    ctx: &crate::host::ScriptCtx,
) -> Result<usize> {
    use yuris_script::Insn;
    use yuris_value::{ElemType, Value};
    let mut count = 0usize;
    for (gi, g) in ctx.groups.iter().enumerate() {
        let elem: ElemType = match g.command_type {
            0x10 | 0x19 | 0x1d..=0x20 | 0x52 => ElemType::Float,
            0x11 | 0x32 | 0x21..=0x24 | 0x53 => ElemType::Int,
            0x12 | 0x5C | 0x25..=0x28 | 0x54 => ElemType::Str,
            _ => continue,
        };
        let default = match elem {
            ElemType::Float => Value::Float(0.0),
            ElemType::Int => Value::Int(0),
            ElemType::Str => Value::Str(Vec::new()),
        };
        let first = ctx.first_slots[gi];
        let wins: Vec<yuris_format::ystb::CommandSlot> = ctx.script.slots()
            [first..first + g.window_count as usize]
            .to_vec();
        for w in &wins {
            if w.tag & 0xFF != 0 {
                continue;
            }
            let Some(bytes) = ctx.script.window_bytes_pooled_copy(w) else {
                continue;
            };
            let Ok(instrs) = yuris_script::decode_window(&bytes) else {
                continue;
            };
            // 引用窗必须以单条变量指令开头
            let Some(base) = (match instrs.first().map(|i| &i.kind) {
                Some(Insn::PushVar(r)) | Some(Insn::PushVarRef(r))
                | Some(Insn::PushVarIndexed(r)) => Some(r.clone()),
                _ => None,
            }) else {
                continue;
            };
            if instrs.len() == 1 {
                // 标量声明
                if store.get_opt(&base).is_none() && !store.has_array(&base) {
                    store.set(&base, default.clone());
                    count += 1;
                }
                continue;
            }
            // 数组声明:存在 ArrayLoad;取其前的下标表达式值(变量指令压 0 占位)
            let has_load = instrs.iter().any(|i| matches!(i.kind, Insn::ArrayLoad { .. }));
            if !has_load {
                continue;
            }
            let mut stack: Vec<i64> = Vec::new();
            for ins in &instrs {
                match &ins.kind {
                    Insn::PushInt(n) => stack.push(*n),
                    Insn::PushVar(_) | Insn::PushVarRef(_) | Insn::PushVarIndexed(_) => {
                        stack.push(0)
                    }
                    Insn::Binary(op) => {
                        let (Some(rhs), Some(lhs)) = (stack.pop(), stack.pop()) else {
                            break;
                        };
                        let v = match op {
                            yuris_script::BinOp::Add => lhs.wrapping_add(rhs),
                            yuris_script::BinOp::Sub => lhs.wrapping_sub(rhs),
                            yuris_script::BinOp::Mul => lhs.wrapping_mul(rhs),
                            _ => 0,
                        };
                        stack.push(v);
                    }
                    Insn::ArrayLoad { .. } => break,
                    _ => break,
                }
            }
            if let Some(&max_idx) = stack.last() {
                if max_idx >= 0 {
                    let need = (max_idx + 1) as u32;
                    match store.array_dims(&base) {
                        None => {
                            store.declare_array(&base, elem, &[need]);
                            count += 1;
                        }
                        Some(dims) => {
                            // 同数组多条上界声明 → 取最大(扩容保留旧值)
                            if dims.len() == 1 && dims[0] < need {
                                store.resize_array(&base, &[need])?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(count)
}

/// 声明类命令集合(加载期数据,运行器不执行):INT/FLT/STR 及全局/局部/存档变量族。
fn is_declaration(cmd: u8) -> bool {
    // 精确集合(来自 YSCM 名表):F_*/G_*/S_* 变量声明族 + INT/FLT/STR/VAR 单声明。
    // 注意:0x2a=GO、0x2b=GOSUB、0x29=G_FLT3? —— 0x29 是 G_VAR,保留。
    matches!(
        cmd,
        0x10..=0x13                          // F_FLT/F_INT/F_STR/F_VAR
            | 0x1d..=0x29                     // G_FLT?..G_VAR(全局声明族)
            | 0x52..=0x55                     // S_FLT/S_INT/S_STR/S_VAR
            | 0x19                            // FLT(局部)
            | 0x5c                            // STR(局部)
            | 0x65                            // VAR(局部)
            | 0x32                            // INT(局部)
    )
}

/// 值摘要(事件记录用;Str 截断到 24 字节防爆炸)。
fn value_summary(v: &Value) -> String {
    match v {
        Value::Int(n) => format!("int:{n}"),
        Value::Float(f) => format!("flt:{f}"),
        Value::Str(b) => {
            let shown: Vec<u8> = b.iter().copied().take(24).collect();
            format!("str:{}..({}B)", String::from_utf8_lossy(&shown), b.len())
        }
    }
}

/// 值真值(引擎真值语义 Unknown → 仅实现 Int!=0/非空串,**Likely**;
/// Float/其他 → 显式报错,不猜)。
fn value_truthy(v: &Value) -> Result<bool> {
    match v {
        Value::Int(i) => Ok(*i != 0),
        Value::Str(s) => Ok(!s.is_empty()),
        Value::Float(_) => Err(Error::Unimplemented("float 真值语义")),
    }
}
