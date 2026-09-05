//! 指令语义。所有已证实映射来自官方编译器逆向(`docs/reverse/yscom-compiler-notes.md`),
//! 与 v555 全语料统计(`docs/opcode/opcode-table.md`)一致。

// 注意: VarRef / VarSpace 定义在 `yuris-value`(变量模型归值层),此处 re-export。
pub use yuris_value::{VarRef, VarSpace};

/// 二元运算符(编译器映射,含源码记号)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `+` (0x2b)。字符串拼接同样走此指令(多态,手册 `$S="ABC"+$F`)。
    Add,
    /// `-` (0x2d)。
    Sub,
    /// `*` (0x2a)。
    Mul,
    /// `/` (0x2f)。
    Div,
    /// `%` (0x25)。
    Mod,
    /// `==` (0x3d)。
    Eq,
    /// `!=` (0x21)。
    Ne,
    /// `>=` (0x5a)。
    Ge,
    /// `<=` (0x53)。
    Le,
    /// `>` (0x3e)。
    Gt,
    /// `<` (0x3c)。
    Lt,
    /// `&` 单字符 (0x41) —— 按位与。
    BitAnd,
    /// `|` 单字符 (0x4f) —— 按位或。
    BitOr,
    /// `&&` (0x26)。
    LogAnd,
    /// `||` (0x7c)。
    LogOr,
    /// `^` (0x5e)。
    Xor,
}

/// 一元运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `-x`(一元负号,0x52;token 0x14 无累积值时发射)。
    Neg,
}

/// 显式类型转换(手册 §数値型／文字列型を変換する)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cast {
    /// `$()` (0x73) 转字符串。
    ToStr,
    /// `@()` (0x69) 转整数。
    ToInt,
}

/// 指令语义。已证实与未证实的边界就在这个枚举里。
#[derive(Debug, Clone, PartialEq)]
pub enum Insn {
    /// 整数字面量(0x42=1B / 0x57=2B / 0x49=4B / 0x4c=8B,LE;INT 64 位)。
    PushInt(i64),
    /// 浮点字面量(0x46,8B,IEEE f64;FLT 64 位)。
    PushFloat(f64),
    /// 字符串(0x4d,载荷原样保存 —— 含引号的 SJIS 字节,编码转换另做)。
    PushStr(Vec<u8>),
    /// 取变量**值**(0x48)。
    PushVar(VarRef),
    /// 取变量**引用**(0x56;赋值目标/引用形式)。
    PushVarRef(VarRef),
    /// 下标/成员形式(0x76;编译器 fixup 改写,细节 Likely)。
    PushVarIndexed(VarRef),
    /// 数组元素读取(0x29,`@A(2,3)`;操作数字节语料恒 0,原样保留)。
    ArrayLoad {
        /// 原始操作数字节。
        raw: u8,
    },
    /// 二元运算。
    Binary(BinOp),
    /// 一元运算。
    Unary(UnOp),
    /// 显式类型转换。
    Cast(Cast),
    /// 表达式组分隔/收尾(0x2c)。
    GroupSep,
    /// **未证实 opcode**(0x00/0x01/0x08 等)。保留原始码与操作数,不猜。
    Unknown {
        /// 原始 opcode 字节。
        raw_op: u8,
        /// 原始操作数字节。
        operand: Vec<u8>,
    },
}

/// 一条解码后的指令。
#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    /// 指令头在窗口内的字节偏移。
    pub offset: usize,
    /// 原始 opcode 字节(即使已解析也保留)。
    pub raw_op: u8,
    /// 指令总长(3 + 操作数字节数)。
    pub size: usize,
    /// 语义。
    pub kind: Insn,
}

impl Insn {
    /// 助记符(反汇编/trace 显示用)。
    pub fn mnemonic(&self) -> &'static str {
        match self {
            Insn::PushInt(_) => "pushint",
            Insn::PushFloat(_) => "pushfloat",
            Insn::PushStr(_) => "pushstr",
            Insn::PushVar(_) => "pushvar",
            Insn::PushVarRef(_) => "pushvarref",
            Insn::PushVarIndexed(_) => "pushvaridx",
            Insn::ArrayLoad { .. } => "aload",
            Insn::Binary(b) => match b {
                BinOp::Add => "add",
                BinOp::Sub => "sub",
                BinOp::Mul => "mul",
                BinOp::Div => "div",
                BinOp::Mod => "mod",
                BinOp::Eq => "eq",
                BinOp::Ne => "ne",
                BinOp::Ge => "ge",
                BinOp::Le => "le",
                BinOp::Gt => "gt",
                BinOp::Lt => "lt",
                BinOp::BitAnd => "bitand",
                BinOp::BitOr => "bitor",
                BinOp::LogAnd => "and",
                BinOp::LogOr => "or",
                BinOp::Xor => "xor",
            },
            Insn::Unary(_) => "neg",
            Insn::Cast(Cast::ToStr) => "tostr",
            Insn::Cast(Cast::ToInt) => "toint",
            Insn::GroupSep => "groupsep",
            Insn::Unknown { .. } => "unknown",
        }
    }
}

pub(crate) fn resolve(op: u8, operand: &[u8]) -> Insn {
    // 有操作数宽度要求的 opcode:宽度不符 → Unknown(诚实,不猜)
    match op {
        0x42 if operand.len() == 1 => {
            return Insn::PushInt(i8::from_le_bytes([operand[0]]) as i64);
        }
        0x57 if operand.len() == 2 => {
            return Insn::PushInt(i16::from_le_bytes([operand[0], operand[1]]) as i64);
        }
        0x49 if operand.len() == 4 => {
            let b = [operand[0], operand[1], operand[2], operand[3]];
            return Insn::PushInt(i32::from_le_bytes(b) as i64);
        }
        0x4c if operand.len() == 8 => {
            let mut b = [0u8; 8];
            b.copy_from_slice(operand);
            return Insn::PushInt(i64::from_le_bytes(b));
        }
        0x46 if operand.len() == 8 => {
            let mut b = [0u8; 8];
            b.copy_from_slice(operand);
            return Insn::PushFloat(f64::from_le_bytes(b));
        }
        0x4d => {
            return Insn::PushStr(operand.to_vec());
        }
        0x48 | 0x56 | 0x76 if operand.len() == 3 => {
            let var = VarRef {
                space: VarSpace::from_prefix(operand[0]),
                id: u16::from_le_bytes([operand[1], operand[2]]),
            };
            return match op {
                0x48 => Insn::PushVar(var),
                0x56 => Insn::PushVarRef(var),
                _ => Insn::PushVarIndexed(var),
            };
        }
        0x29 if operand.len() == 1 => {
            return Insn::ArrayLoad { raw: operand[0] };
        }
        _ => {}
    }

    // 无操作数运算符(宽度必须为 0)
    if operand.is_empty() {
        let bin = match op {
            0x2b => Some(BinOp::Add),
            0x2d => Some(BinOp::Sub),
            0x2a => Some(BinOp::Mul),
            0x2f => Some(BinOp::Div),
            0x25 => Some(BinOp::Mod),
            0x3d => Some(BinOp::Eq),
            0x21 => Some(BinOp::Ne),
            0x5a => Some(BinOp::Ge),
            0x53 => Some(BinOp::Le),
            0x3e => Some(BinOp::Gt),
            0x3c => Some(BinOp::Lt),
            0x41 => Some(BinOp::BitAnd),
            0x4f => Some(BinOp::BitOr),
            0x26 => Some(BinOp::LogAnd),
            0x7c => Some(BinOp::LogOr),
            0x5e => Some(BinOp::Xor),
            _ => None,
        };
        if let Some(b) = bin {
            return Insn::Binary(b);
        }
        match op {
            0x52 => return Insn::Unary(UnOp::Neg),
            0x73 => return Insn::Cast(Cast::ToStr),
            0x69 => return Insn::Cast(Cast::ToInt),
            0x2c => return Insn::GroupSep,
            _ => {}
        }
    }

    // 已知 opcode 但操作数宽度意外 / 完全未知 → Unknown,原样保留
    Insn::Unknown {
        raw_op: op,
        operand: operand.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_window;

    fn one(window: &[u8]) -> Insn {
        let mut v = decode_window(window).unwrap();
        assert_eq!(v.len(), 1);
        v.remove(0).kind
    }

    #[test]
    fn literals_by_width() {
        assert_eq!(one(&[0x42, 1, 0, 0x05]), Insn::PushInt(5));
        assert_eq!(one(&[0x42, 1, 0, 0xff]), Insn::PushInt(-1)); // i8
        assert_eq!(one(&[0x57, 2, 0, 0xff, 0xff]), Insn::PushInt(-1)); // i16
        assert_eq!(one(&[0x49, 4, 0, 0x00, 0x00, 0x00, 0x80]), Insn::PushInt(-2147483648));
        let mut w64 = vec![0x4c, 8, 0];
        w64.extend_from_slice(&(-1i64).to_le_bytes());
        assert_eq!(one(&w64), Insn::PushInt(-1));
    }

    #[test]
    fn float_and_string() {
        let mut w = vec![0x46, 8, 0];
        w.extend_from_slice(&10.0f64.to_le_bytes());
        assert_eq!(one(&w), Insn::PushFloat(10.0)); // 语料实测值
        // 类型 4:引号被编译器算进载荷
        assert_eq!(
            one(&[0x4d, 4, 0, b'"', b'A', b'B', b'"']),
            Insn::PushStr(b"\"AB\"".to_vec())
        );
    }

    #[test]
    fn variable_refs() {
        assert_eq!(
            one(&[0x48, 3, 0, 0x40, 0x19, 0x09]),
            Insn::PushVar(VarRef { space: VarSpace::At, id: 0x0919 })
        );
        assert_eq!(
            one(&[0x56, 3, 0, 0x24, 0x04, 0x00]),
            Insn::PushVarRef(VarRef { space: VarSpace::Dollar, id: 4 })
        );
        assert_eq!(
            one(&[0x76, 3, 0, 0x23, 0x00, 0x00]),
            Insn::PushVarIndexed(VarRef { space: VarSpace::Hash, id: 0 })
        );
    }

    #[test]
    fn operators_and_unknowns() {
        assert_eq!(one(&[0x2b, 0, 0]), Insn::Binary(BinOp::Add));
        assert_eq!(one(&[0x3d, 0, 0]), Insn::Binary(BinOp::Eq));
        assert_eq!(one(&[0x7c, 0, 0]), Insn::Binary(BinOp::LogOr));
        assert_eq!(one(&[0x52, 0, 0]), Insn::Unary(UnOp::Neg));
        assert_eq!(one(&[0x73, 0, 0]), Insn::Cast(Cast::ToStr));
        assert_eq!(one(&[0x69, 0, 0]), Insn::Cast(Cast::ToInt));
        assert_eq!(one(&[0x2c, 0, 0]), Insn::GroupSep);
        assert_eq!(one(&[0x29, 1, 0, 0x00]), Insn::ArrayLoad { raw: 0 });
        // 未证实 opcode:Unknown + 原样保留
        assert_eq!(
            one(&[0x00, 2, 0, 0xde, 0xad]),
            Insn::Unknown { raw_op: 0x00, operand: vec![0xde, 0xad] }
        );
        // 已知 op + 意外宽度 → Unknown
        assert!(matches!(
            one(&[0x42, 2, 0, 0x01, 0x02]),
            Insn::Unknown { raw_op: 0x42, .. }
        ));
    }

    #[test]
    fn multi_instruction_window_with_offsets() {
        // 与语料 yst00000 首窗口同构:6+5+4+3+4 = 22
        let w = [
            0x56, 3, 0, 0x24, 0xca, 0x04, // PushVar($, 0x04ca)
            0x57, 2, 0, 0x90, 0x01, // PushInt(0x0190)
            0x42, 1, 0, 0x01, // PushInt(1)
            0x2b, 0, 0, // Add
            0x29, 1, 0, 0x00, // ArrayLoad
        ];
        let v = decode_window(&w).unwrap();
        assert_eq!(v.len(), 5);
        assert_eq!(v[0].offset, 0);
        assert_eq!(v[1].offset, 6);
        assert_eq!(v[2].offset, 11);
        assert_eq!(v[3].offset, 15);
        assert_eq!(v[4].offset, 18);
        assert!(v.iter().all(|i| matches!(i.kind, Insn::PushVarRef(_)
            | Insn::PushInt(_) | Insn::Binary(_) | Insn::ArrayLoad { .. })));
    }

    #[test]
    fn truncation_is_error() {
        assert!(decode_window(&[0x42, 1, 0]).is_err()); // 操作数截断
        assert!(decode_window(&[0x42]).is_err()); // 头截断
    }
}
