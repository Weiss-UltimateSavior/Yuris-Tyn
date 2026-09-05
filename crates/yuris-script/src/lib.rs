//! yuris-script
//!
//! L2：变长 VM 字节码 → `Instruction` IR。
//!
//! 指令编码与语义见 `docs/opcode/opcode-table.md`(第二版,**Confirmed**,
//! 由官方编译器 YSCom.exe 逆向 + v555 全语料统计双侧验证):
//!
//! ```text
//! instruction = op(u8) + operand_len(u16 LE) + operand(operand_len 字节)
//! ```
//!
//! 已证实语义:字面量族(0x42/57/49/4c/46/4d)、变量引用(0x48/56/76,
//! 操作数 `[前缀字符][id:u16]`)、算术/比较/逻辑/位运算、类型转换(0x73/0x69)、
//! 数组读取(0x29)、组分隔(0x2c)。
//!
//! **未证实**(0x00/0x01/0x08 等)一律输出 [`Insn::Unknown`],**绝不猜测**。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod eval;
pub mod insn;
pub use eval::{Evaluator, LValueRef, Slot};
pub use insn::{BinOp, Cast, Insn, Instruction, UnOp};
pub use yuris_value::{VarRef, VarSpace};
pub use yuris_core::Result;

use yuris_core::Error;

/// 解码一个自描述指令窗口(不跨窗口,不处理槽位分帧)。
///
/// 分帧由 `yuris-format::ystb::segment_window` 或调用方完成;
/// 本函数按 `[op][len:u16][operand]` 线性切分并解析语义。
/// 任何不闭合(截断/越界)都会报错 —— 窗口必须完整。
pub fn decode_window(window: &[u8]) -> Result<Vec<Instruction>> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < window.len() {
        if p + 3 > window.len() {
            return Err(Error::format(format!(
                "instruction header truncated at window offset {p}"
            )));
        }
        let op = window[p];
        let operand_len = u16::from_le_bytes([window[p + 1], window[p + 2]]) as usize;
        let operand_start = p + 3;
        let end = operand_start + operand_len;
        if end > window.len() {
            return Err(Error::format(format!(
                "operand overruns window: op={op:#04x} len={operand_len} end={end} > {}",
                window.len()
            )));
        }
        let operand = &window[operand_start..end];
        out.push(Instruction {
            offset: p,
            raw_op: op,
            size: 3 + operand_len,
            kind: insn::resolve(op, operand),
        });
        p = end;
    }
    Ok(out)
}
