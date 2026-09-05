//! yuris-value
//!
//! Value 类型与变量存储。YU-RIS 的类型转换语义在此实现,不使用 `serde_json::Value`。
//!
//! ## 证据等级(铁律:每个语义行为都标注等级)
//!
//! - **Confirmed**(官方手册 `yu-ris_sdk_0495/マニュアル/YU-RIS/html`):
//!   变量三类型 INT(64 位)/FLT(64 位)/STR;`@`数值/`$`字符串前缀;
//!   字符串 `==` 比较;`+` 字符串拼接;`$()`/`@()` 显式转换;
//!   `@("123"+"45") == 12345`;`INT[@A = 0xFFFFFFFFFFFFFFFF]` 得 -1(i64 环绕字面量)
//! - **Likely**(C 实现惯例,待 Golden Test 对照):
//!   混合 int/float 运算提升为 float;有符号溢出环绕;比较结果为 1/0;
//!   float→int 截断
//! - **Unknown → 显式报错**:字符串排序比较、字符串真值、float→str 格式化、
//!   非数字字符串转 int 的精确规则(手册称 β 版"不確定")

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;

use yuris_core::{Error, Result};

/// crate 版本(与 workspace 同步)
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 变量空间。对应源码前缀字符(编译器证实,见 `docs/reverse/yscom-compiler-notes.md` §4.3)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VarSpace {
    /// `@` (0x40) —— 数值变量(INT/FLT)。
    At,
    /// `$` (0x24) —— 字符串变量。
    Dollar,
    /// `#` (0x23) —— 标签/宏引用(`GO[#=es.ERIS]`)。
    Hash,
    /// `` ` `` (0x60) —— 罕见,语义 Unknown。
    Backtick,
    /// 其他前缀(本语料未观测;原样保留,不猜)。
    Unknown(u8),
}

impl VarSpace {
    /// 由操作数首字节解析。
    pub fn from_prefix(b: u8) -> Self {
        match b {
            0x40 => VarSpace::At,
            0x24 => VarSpace::Dollar,
            0x23 => VarSpace::Hash,
            0x60 => VarSpace::Backtick,
            other => VarSpace::Unknown(other),
        }
    }

    /// 原始前缀字节(存储键的一部分)。
    pub fn prefix(self) -> u8 {
        match self {
            VarSpace::At => 0x40,
            VarSpace::Dollar => 0x24,
            VarSpace::Hash => 0x23,
            VarSpace::Backtick => 0x60,
            VarSpace::Unknown(b) => b,
        }
    }
}

/// 变量引用:`[前缀字符][id:u16]`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarRef {
    /// 变量空间(源码前缀)。
    pub space: VarSpace,
    /// 变量 id(u16 LE;id ↔ 名字的解析关系 Unknown —— 编译期已消解)。
    pub id: u16,
}

impl VarRef {
    /// 显示形式(`@1041` / `$F` / `#label` 字符化)。
    pub fn display(&self) -> String {
        let ch = match self.space {
            VarSpace::At => '@',
            VarSpace::Dollar => '$',
            VarSpace::Hash => '#',
            VarSpace::Backtick => '`',
            VarSpace::Unknown(b) => b as char,
        };
        format!("{ch}{}", self.id)
    }
}

/// YU-RIS 值。变量三类型:INT(64 位)/FLT(64 位)/STR(手册 §変数,**Confirmed**)。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 整数(INT,64 位)。
    Int(i64),
    /// 实数(FLT,IEEE f64)。
    Float(f64),
    /// 字符串(STR;**SJIS 字节原样保存**,编码转换在显示层)。
    Str(Vec<u8>),
}

impl Value {
    /// `+`。Int 环绕加(Likely);Str+Str = 字节拼接(手册 **Confirmed**);
    /// 混合数值提升 float(Likely)。
    pub fn add(&self, other: &Value) -> Result<Value> {
        if let (Value::Str(a), Value::Str(b)) = (self, other) {
            let mut v = a.clone();
            v.extend_from_slice(b);
            return Ok(Value::Str(v));
        }
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(*b))),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            _ => Err(Error::Unimplemented(
                "字符串与数值的混合 + 语义 Unknown(手册要求显式 $()/@() 转换)",
            )),
        }
    }

    /// `-`。
    pub fn sub(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(*b))),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            _ => Err(Error::Unimplemented("字符串 - 语义 Unknown")),
        }
    }

    /// `*`。
    pub fn mul(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(*b))),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            _ => Err(Error::Unimplemented("字符串 * 语义 Unknown")),
        }
    }

    /// `/`。Int 除零 → 显式报错(引擎行为 Unknown);Float 按 IEEE。
    pub fn div(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(Error::format("integer division by zero (引擎语义 Unknown)"))
                } else {
                    Ok(Value::Int(a.wrapping_div(*b)))
                }
            }
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / *b as f64)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            _ => Err(Error::Unimplemented("字符串 / 语义 Unknown")),
        }
    }

    /// `%`。Int 除零 → 显式报错。
    pub fn modulo(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(Error::format("integer modulo by zero (引擎语义 Unknown)"))
                } else {
                    Ok(Value::Int(a.wrapping_rem(*b)))
                }
            }
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a % b)),
            _ => Err(Error::Unimplemented("float/int 混合或字符串的 % 语义 Unknown")),
        }
    }

    /// `==`(数值或字符串字节相等;字符串 **Confirmed**)。
    pub fn eq(&self, other: &Value) -> Result<bool> {
        Ok(match (self, other) {
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Float(a), Value::Int(b)) | (Value::Int(b), Value::Float(a)) => {
                *a == *b as f64
            }
            (Value::Str(_), _) | (_, Value::Str(_)) => false,
        })
    }

    /// 排序比较前的数值检查。
    fn require_numeric(&self, other: &Value) -> Result<()> {
        if matches!(self, Value::Str(_)) || matches!(other, Value::Str(_)) {
            return Err(Error::Unimplemented(
                "字符串排序比较语义 Unknown(仅 ==/!= 有手册佐证)",
            ));
        }
        Ok(())
    }

    /// `!=`。
    pub fn ne(&self, other: &Value) -> Result<bool> {
        Ok(!self.eq(other)?)
    }

    /// `>=`。字符串排序 → Unknown 报错。
    pub fn ge(&self, other: &Value) -> Result<bool> {
        self.require_numeric(other)?;
        Ok(self.cmp_num(other) >= 0)
    }

    /// `<=`。
    pub fn le(&self, other: &Value) -> Result<bool> {
        self.require_numeric(other)?;
        Ok(self.cmp_num(other) <= 0)
    }

    /// `>`。
    pub fn gt(&self, other: &Value) -> Result<bool> {
        self.require_numeric(other)?;
        Ok(self.cmp_num(other) > 0)
    }

    /// `<`。
    pub fn lt(&self, other: &Value) -> Result<bool> {
        self.require_numeric(other)?;
        Ok(self.cmp_num(other) < 0)
    }

    fn cmp_num(&self, other: &Value) -> i64 {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => match a.cmp(b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            },
            _ => {
                let a = self.as_f64();
                let b = other.as_f64();
                match a.partial_cmp(&b) {
                    Some(std::cmp::Ordering::Less) => -1,
                    Some(std::cmp::Ordering::Equal) => 0,
                    _ => 1,
                }
            }
        }
    }

    fn as_f64(&self) -> f64 {
        match self {
            Value::Int(v) => *v as f64,
            Value::Float(v) => *v,
            Value::Str(_) => 0.0,
        }
    }

    /// 按位与(仅 Int;**Likely**,单字符 `&` 的精确语义待引擎对照)。
    pub fn bitand(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
            _ => Err(Error::Unimplemented("bitand 仅对 Int 实现(Float/Str Unknown)")),
        }
    }

    /// 按位或。
    pub fn bitor(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
            _ => Err(Error::Unimplemented("bitor 仅对 Int 实现")),
        }
    }

    /// 按位异或。
    pub fn xor(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
            _ => Err(Error::Unimplemented("xor 仅对 Int 实现")),
        }
    }

    /// 逻辑与(真值合并;比较结果在栈上无短路 —— 编译器把两侧先行求值)。
    pub fn logand(&self, other: &Value) -> Result<Value> {
        Ok(Value::Int((self.is_truthy()? && other.is_truthy()?) as i64))
    }

    /// 逻辑或。
    pub fn logor(&self, other: &Value) -> Result<Value> {
        Ok(Value::Int((self.is_truthy()? || other.is_truthy()?) as i64))
    }

    /// 真值。Str 真值 → Unknown 报错。
    pub fn is_truthy(&self) -> Result<bool> {
        match self {
            Value::Int(v) => Ok(*v != 0),
            Value::Float(v) => Ok(*v != 0.0),
            Value::Str(_) => Err(Error::Unimplemented("字符串真值语义 Unknown")),
        }
    }

    /// 一元负号。
    pub fn neg(&self) -> Result<Value> {
        match self {
            Value::Int(v) => Ok(Value::Int(v.wrapping_neg())),
            Value::Float(v) => Ok(Value::Float(-v)),
            Value::Str(_) => Err(Error::Unimplemented("字符串取负 语义 Unknown")),
        }
    }

    /// `$()` 转字符串。Int → 十进制(手册 **Confirmed**);
    /// Float → 格式化规则 Unknown → 报错;Str → 原样。
    pub fn cast_to_str(&self) -> Result<Value> {
        match self {
            Value::Str(_) => Ok(self.clone()),
            Value::Int(v) => Ok(Value::Str(v.to_string().into_bytes())),
            Value::Float(_) => Err(Error::Unimplemented(
                "float→string 格式化规则 Unknown(引擎可能是 %f 系)",
            )),
        }
    }

    /// 取 Int(仅 i64;其他类型 → None,不猜)。
    pub fn as_int_opt(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// 字符串按 SJIS 解码的 String(lossy;用于事件字段展示)。
    pub fn str_as_string(&self) -> String {
        match self {
            Value::Str(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => String::new(),
        }
    }

    /// `@()` 转整数。字符串全为十进制数字(可带负号)→ 数值
    /// (手册 `@("123"+"45") == 12345` **Confirmed**);
    /// 含非数字字符 → 手册称 β 版"不確定" → 显式报错;
    /// Float → 截断(**Likely**,C 惯例)。
    pub fn cast_to_int(&self) -> Result<Value> {
        match self {
            Value::Int(_) => Ok(self.clone()),
            Value::Float(v) => Ok(Value::Int(*v as i64)),
            Value::Str(bytes) => {
                let s = String::from_utf8_lossy(bytes);
                let t = s.trim();
                match t.parse::<i64>() {
                    Ok(v) => Ok(Value::Int(v)),
                    Err(_) => Err(Error::Unimplemented(
                        "非数字字符串转 int 的引擎规则 Unknown(手册:β 版不確定)",
                    )),
                }
            }
        }
    }
}

/// 数组元素类型(变量描述符 byte+1:1=INT 2=FLT 3=STR;引擎 Confirmed)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElemType {
    /// 1 = INT(i64,8 字节/元素)。
    Int,
    /// 2 = FLT(f64,8 字节/元素)。
    Float,
    /// 3 = STR(变长字节串)。
    Str,
}

/// 数组存储:类型 + 各维边界 + 行主序元素(引擎 0x29 处理器 Confirmed:
/// 线性化 = Σ idx[k]·Π bounds[j<k],**最后一维最快**;越界 → 引擎报错 0x1a202)。
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayStorage {
    /// 元素类型。
    pub elem: ElemType,
    /// 各维边界(元素个数;来自 YSVR 维数/边界或 GOSUB gparam)。
    pub bounds: Vec<u32>,
    /// 行主序元素。长度 = bounds.product()(STR 数组每元素为字符串)。
    pub data: Vec<Value>,
}

impl ArrayStorage {
    /// 按边界构造并清零/置空。
    pub fn new(elem: ElemType, bounds: &[u32]) -> Self {
        let n: usize = bounds.iter().map(|&b| b as usize).product();
        let data = match elem {
            ElemType::Int => vec![Value::Int(0); n],
            ElemType::Float => vec![Value::Float(0.0); n],
            ElemType::Str => vec![Value::Str(Vec::new()); n],
        };
        Self { elem, bounds: bounds.to_vec(), data }
    }

    /// 行主序线性偏移(**Confirmed**:引擎 0x29,最后一维最快)。
    /// `indices.len() != bounds.len()` → 维数不匹配(引擎 0x1a20c);
    /// `idx[k] >= bounds[k]` → 越界(引擎 0x1a202)。
    pub fn linear_offset(bounds: &[u32], indices: &[i64]) -> Result<usize> {
        if indices.len() != bounds.len() {
            return Err(Error::format(format!(
                "array dimension mismatch: got {} indices, declared {}",
                indices.len(),
                bounds.len()
            )));
        }
        let mut offset: i64 = 0;
        let mut stride: i64 = 1;
        for k in (0..bounds.len()).rev() {
            let idx = indices[k];
            let b = bounds[k] as i64;
            if !(0..b).contains(&idx) {
                return Err(Error::format(format!(
                    "array index out of bounds: idx[{k}]={idx}, bound={b}"
                )));
            }
            offset += stride * idx;
            stride *= b;
        }
        Ok(offset as usize)
    }

    /// 读元素。
    pub fn get(&self, indices: &[i64]) -> Result<&Value> {
        let off = Self::linear_offset(&self.bounds, indices)?;
        self.data.get(off).ok_or_else(|| Error::format("linear offset out of range (内部错误)"))
    }

    /// 写元素。
    pub fn set(&mut self, indices: &[i64], v: Value) -> Result<()> {
        if !self.elem_matches(&v) {
            return Err(Error::format(format!(
                "element type mismatch: storing {:?} into {:?} array",
                v, self.elem
            )));
        }
        let off = Self::linear_offset(&self.bounds, indices)?;
        self.data[off] = v;
        Ok(())
    }

    fn elem_matches(&self, v: &Value) -> bool {
        matches!(
            (self.elem, v),
            (ElemType::Int, Value::Int(_))
                | (ElemType::Float, Value::Float(_))
                | (ElemType::Str, Value::Str(_))
        )
    }
}

/// LET 复合赋值码(w0.B3,引擎 FUN_00443808 **Confirmed**):
/// 0=`=` 1=`+=` 2=`-=` 3=`*=` 4=`/=` 5=`%=` 6=`&=` 7=`|=` 8=`^=`。
pub fn compound_assign(code: u8, lhs: &Value, rhs: &Value) -> Result<Value> {
    match code {
        0 => Ok(rhs.clone()),
        1 => lhs.add(rhs),
        2 => lhs.sub(rhs),
        3 => lhs.mul(rhs),
        4 => lhs.div(rhs),
        5 => lhs.modulo(rhs),
        6 => lhs.bitand(rhs),
        7 => lhs.bitor(rhs),
        8 => lhs.xor(rhs),
        _ => Err(Error::format(format!("unknown compound assignment code {code}"))),
    }
}

#[derive(Debug, Clone)]
pub struct VariableStore {
    vars: HashMap<(u8, u16), Value>,
    arrays: HashMap<(u8, u16), ArrayStorage>,
}

impl Default for VariableStore {
    fn default() -> Self {
        Self {
            vars: HashMap::new(),
            arrays: HashMap::new(),
        }
    }
}

impl VariableStore {
    /// 空存储。
    pub fn new() -> Self {
        Self::default()
    }

    /// 读取;未定义 → 显式报错(引擎默认值行为 Unknown,不猜)。
    pub fn get(&self, r: &VarRef) -> Result<&Value> {
        self.vars
            .get(&(r.space.prefix(), r.id))
            .ok_or_else(|| Error::format(format!("undefined variable {}", r.display())))
    }

    /// 读取(可选)。
    pub fn get_opt(&self, r: &VarRef) -> Option<&Value> {
        self.vars.get(&(r.space.prefix(), r.id))
    }

    /// 写入(LET / 命令参数求值的结果)。
    pub fn set(&mut self, r: &VarRef, v: Value) {
        self.vars.insert((r.space.prefix(), r.id), v);
    }

    /// 声明/重置数组(YSVR 消费或 GOSUB 帧局部)。
    /// 幂等:重复声明同型数组不重置(引擎:声明组只在加载期消费一次)。
    pub fn declare_array(&mut self, r: &VarRef, elem: ElemType, bounds: &[u32]) {
        let key = (r.space.prefix(), r.id);
        if let Some(a) = self.arrays.get(&key) {
            if a.elem == elem && a.bounds == bounds {
                return;
            }
        }
        self.arrays.insert(key, ArrayStorage::new(elem, bounds));
    }

    /// 读数组元素(标量数组 = 单下标;未声明 → 报错,同引擎 0x1a5ea)。
    pub fn get_elem(&self, r: &VarRef, indices: &[i64]) -> Result<&Value> {
        let a = self
            .arrays
            .get(&(r.space.prefix(), r.id))
            .ok_or_else(|| {
                Error::format(format!(
                    "undefined array variable {}{} (引擎 0x1a5ea 同族)",
                    r.space.prefix(),
                    r.id
                ))
            })?;
        a.get(indices).map_err(|e| {
            Error::format(format!("{} (array {}{})", e, r.space.prefix(), r.id))
        })
    }

    /// 写数组元素。
    pub fn set_elem(&mut self, r: &VarRef, indices: &[i64], v: Value) -> Result<()> {
        match self.arrays.get_mut(&(r.space.prefix(), r.id)) {
            Some(a) => {
                // 引擎写入按**描述符类型**收敛(变量无独立类型,desc 定型):
                // FLT 数组收 Int → 转 f64;INT 数组收 Float → 四舍五入成 i64
                // (引擎 00443808 FLT/INT 描述符分支各自转存;成果 59g)。
                let cv = match (&a.elem, v) {
                    (ElemType::Float, Value::Int(i)) => Value::Float(i as f64),
                    (ElemType::Int, Value::Float(f)) => Value::Int(f.round() as i64),
                    (_, other) => other,
                };
                a.set(indices, cv).map_err(|e| {
                    Error::format(format!("{} (array {}{})", e, r.space.prefix(), r.id))
                })
            }
            None => Err(Error::format(format!(
                "undefined array variable {}{}",
                r.space.prefix(),
                r.id
            ))),
        }
    }

    /// 数组是否存在(描述符检查;引擎:id>999 需描述符非零,否则 0x1a5ea)。
    pub fn has_array(&self, r: &VarRef) -> bool {
        self.arrays.contains_key(&(r.space.prefix(), r.id))
    }

    /// 数组各维边界(未声明 → None)。
    pub fn array_dims(&self, r: &VarRef) -> Option<Vec<u32>> {
        self.arrays.get(&(r.space.prefix(), r.id)).map(|a| a.bounds.clone())
    }

    /// 数组元素类型(未声明 → None)。
    pub fn array_elem_type(&self, r: &VarRef) -> Option<ElemType> {
        self.arrays.get(&(r.space.prefix(), r.id)).map(|a| a.elem)
    }

    /// 调整数组为 `new_bounds`(保留原有元素,新增元素清零;
    /// 引擎 FUN_00454710 realloc 的保留行为 **Likely**)。
    /// 维数必须与原数组一致(引擎 VARACT DIMSIZE 要求维数==1,0x18cb4 族)。
    pub fn resize_array(&mut self, r: &VarRef, new_bounds: &[u32]) -> Result<()> {
        let key = (r.space.prefix(), r.id);
        let Some(old) = self.arrays.get(&key) else {
            return Err(Error::format(format!(
                "resize_array: undefined array {}{}",
                r.space.prefix(),
                r.id
            )));
        };
        if old.bounds.len() != new_bounds.len() {
            return Err(Error::format(format!(
                "resize_array: dim mismatch (array has {}, got {})",
                old.bounds.len(),
                new_bounds.len()
            )));
        }
        let mut next = ArrayStorage::new(old.elem, new_bounds);
        // 行主序前缀保留(两数组前缀布局一致)
        let n = old.data.len().min(next.data.len());
        next.data[..n].clone_from_slice(&old.data[..n]);
        self.arrays.insert(key, next);
        Ok(())
    }

    /// YSSD/LOAD 整块写回(引擎 FUN_0044564d 等价:memcpy 到描述符数据区)。
    ///
    /// `values` 元素数 ≤ 声明容量(引擎 memcpy 无界,VM 显式防护);少于
    /// 容量 = 前缀覆盖、其余保留(引擎部分拷贝语义)。类型收敛同
    /// [`Self::set_elem`]。
    pub fn load_array_data(&mut self, r: &VarRef, values: Vec<Value>) -> Result<()> {
        let key = (r.space.prefix(), r.id);
        let Some(a) = self.arrays.get_mut(&key) else {
            return Err(Error::format(format!(
                "load_array_data: undefined array {}{}",
                r.space.prefix(),
                r.id
            )));
        };
        if values.len() > a.data.len() {
            return Err(Error::format(format!(
                "load_array_data: {} values exceed capacity {} ({}{})",
                values.len(),
                a.data.len(),
                r.space.prefix(),
                r.id
            )));
        }
        for (i, v) in values.into_iter().enumerate() {
            let cv = match (&a.elem, v) {
                (ElemType::Float, Value::Int(i)) => Value::Float(i as f64),
                (ElemType::Int, Value::Float(f)) => Value::Int(f.round() as i64),
                (_, other) => other,
            };
            a.data[i] = cv;
        }
        Ok(())
    }

    /// 已定义变量数。
    pub fn len(&self) -> usize {
        self.vars.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(b: &[u8]) -> Value {
        Value::Str(b.to_vec())
    }

    #[test]
    fn manual_confirmed_semantics() {
        // 手册: $S = "ABC" + ".BMP"
        assert_eq!(
            s(b"ABC").add(&s(b".BMP")).unwrap(),
            s(b"ABC.BMP")
        );
        // 手册: @("123"+"45") == 12345
        let n = s(b"123").add(&s(b"45")).unwrap();
        assert_eq!(n.cast_to_int().unwrap(), Value::Int(12345));
        // 手册: $() 数值转字符串
        assert_eq!(Value::Int(123).cast_to_str().unwrap(), s(b"123"));
        // 手册: 字符串 == 比较(りんご 的 SJIS 字节)
        let ringo: &[u8] = &[0x82, 0xE8, 0x82, 0xF1, 0x82, 0xB2];
        assert!(s(ringo).eq(&s(ringo)).unwrap());
        // 手册: 64 位字面量 0xFFFFFFFFFFFFFFFF → -1(字面量已在编译期环绕)
        assert_eq!(Value::Int(-1).cast_to_str().unwrap(), s(b"-1"));
    }

    #[test]
    fn likely_semantics_are_marked_and_working() {
        // 混合精度提升(Likely)
        assert_eq!(
            Value::Int(30).mul(&Value::Float(0.5)).unwrap(),
            Value::Float(15.0)
        );
        // 比较结果 1/0(Likely)
        assert_eq!(Value::Int(5).ge(&Value::Int(5)).unwrap(), true);
        assert_eq!(Value::Int(4).lt(&Value::Int(5)).unwrap(), true);
        // 截断(Likely)
        assert_eq!(Value::Float(1.9).cast_to_int().unwrap(), Value::Int(1));
    }

    #[test]
    fn unknown_semantics_error_out() {
        assert!(Value::Str(vec![0x82, 0xA0]).is_truthy().is_err()); // 字符串真值(あ SJIS)
        assert!(Value::Str(b"abc".to_vec()).cast_to_int().is_err()); // 非数字
        assert!(Value::Float(1.0).cast_to_str().is_err()); // float 格式化
        assert!(Value::Int(1).div(&Value::Int(0)).is_err()); // 除零
        assert!(Value::Str(b"a".to_vec()).lt(&Value::Str(b"b".to_vec())).is_err()); // 字符串排序
    }

    #[test]
    fn store_roundtrip() {
        let mut st = VariableStore::new();
        let r = VarRef { space: VarSpace::At, id: 0x0919 };
        assert!(st.get(&r).is_err()); // 未定义 → 报错
        st.set(&r, Value::Int(250));
        assert_eq!(st.get(&r).unwrap(), &Value::Int(250));
        let r2 = VarRef { space: VarSpace::Dollar, id: 4 };
        st.set(&r2, s(b"GAME"));
        assert_eq!(st.get(&r2).unwrap(), &s(b"GAME"));
        assert_eq!(st.len(), 2);
    }
}

#[cfg(test)]
mod array_tests {
    use super::*;

    /// 行主序线性化(引擎 0x29 Confirmed:最后一维最快)。
    #[test]
    fn row_major_linearization() {
        // 2×3 数组:idx=(r,c) → r*3+c
        let bounds = [2u32, 3];
        assert_eq!(ArrayStorage::linear_offset(&bounds, &[0, 0]).unwrap(), 0);
        assert_eq!(ArrayStorage::linear_offset(&bounds, &[0, 2]).unwrap(), 2);
        assert_eq!(ArrayStorage::linear_offset(&bounds, &[1, 0]).unwrap(), 3);
        assert_eq!(ArrayStorage::linear_offset(&bounds, &[1, 2]).unwrap(), 5);
        // 三维:2×3×4,idx=(1,1,1) = 1*(3*4)+1*4+1 = 17
        assert_eq!(
            ArrayStorage::linear_offset(&[2, 3, 4], &[1, 1, 1]).unwrap(),
            17
        );
    }

    #[test]
    fn bounds_and_dimension_errors() {
        // 越界(引擎 0x1a202)
        assert!(ArrayStorage::linear_offset(&[2, 3], &[2, 0]).is_err());
        assert!(ArrayStorage::linear_offset(&[2, 3], &[0, 3]).is_err());
        assert!(ArrayStorage::linear_offset(&[2, 3], &[-1, 0]).is_err());
        // 维数不匹配(引擎 0x1a20c)
        assert!(ArrayStorage::linear_offset(&[2, 3], &[1]).is_err());
        assert!(ArrayStorage::linear_offset(&[2, 3], &[1, 1, 1]).is_err());
    }

    #[test]
    fn array_get_set_roundtrip() {
        let mut a = ArrayStorage::new(ElemType::Int, &[2, 3]);
        a.set(&[1, 2], Value::Int(42)).unwrap();
        assert_eq!(a.get(&[1, 2]).unwrap(), &Value::Int(42));
        assert_eq!(a.get(&[0, 0]).unwrap(), &Value::Int(0)); // 清零初始化
        // 类型不匹配拒绝
        assert!(a.set(&[0, 0], Value::Float(1.0)).is_err());
        // FLT 数组清零
        let f = ArrayStorage::new(ElemType::Float, &[2]);
        assert_eq!(f.get(&[1]).unwrap(), &Value::Float(0.0));
        // STR 数组置空串
        let s = ArrayStorage::new(ElemType::Str, &[2]);
        assert_eq!(s.get(&[1]).unwrap(), &Value::Str(Vec::new()));
    }

    /// LET 复合赋值码(引擎 FUN_00443808 Confirmed:B3 0..8)。
    #[test]
    fn compound_assign_codes() {
        let l = Value::Int(10);
        assert_eq!(compound_assign(0, &l, &Value::Int(3)).unwrap(), Value::Int(3));
        assert_eq!(compound_assign(1, &l, &Value::Int(3)).unwrap(), Value::Int(13));
        assert_eq!(compound_assign(2, &l, &Value::Int(3)).unwrap(), Value::Int(7));
        assert_eq!(compound_assign(3, &l, &Value::Int(3)).unwrap(), Value::Int(30));
        assert_eq!(compound_assign(4, &l, &Value::Int(3)).unwrap(), Value::Int(3));
        assert_eq!(compound_assign(5, &l, &Value::Int(3)).unwrap(), Value::Int(1));
        assert_eq!(compound_assign(6, &l, &Value::Int(3)).unwrap(), Value::Int(2));
        assert_eq!(compound_assign(7, &l, &Value::Int(3)).unwrap(), Value::Int(11));
        assert_eq!(compound_assign(8, &l, &Value::Int(3)).unwrap(), Value::Int(9));
        // 字符串 +=(手册 Confirmed 的拼接语义)
        let s = Value::Str(b"ABC".to_vec());
        assert_eq!(
            compound_assign(1, &s, &Value::Str(b".BMP".to_vec())).unwrap(),
            Value::Str(b"ABC.BMP".to_vec())
        );
        assert!(compound_assign(9, &l, &Value::Int(1)).is_err());
    }

    /// VariableStore 的数组 API(declare 幂等 / get_elem / set_elem / 未声明报错)。
    #[test]
    fn store_array_api() {
        let mut st = VariableStore::new();
        let r = VarRef { space: VarSpace::At, id: 5000 };
        assert!(!st.has_array(&r));
        st.declare_array(&r, ElemType::Int, &[4]);
        assert!(st.has_array(&r));
        st.set_elem(&r, &[2], Value::Int(9)).unwrap();
        assert_eq!(st.get_elem(&r, &[2]).unwrap(), &Value::Int(9));
        // 幂等重声明:数据保留
        st.declare_array(&r, ElemType::Int, &[4]);
        assert_eq!(st.get_elem(&r, &[2]).unwrap(), &Value::Int(9));
        // 未声明 → 报错(引擎 0x1a5ea 同族)
        let r2 = VarRef { space: VarSpace::At, id: 5001 };
        assert!(st.get_elem(&r2, &[0]).is_err());
        // 未定义标量仍报错
        assert!(st.get(&r2).is_err());
    }
}
