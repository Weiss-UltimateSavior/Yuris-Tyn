# YSCom.exe(官方编译器 v0.495)逆向笔记

| 项 | 值 |
|---|---|
| 目标 | `yu-ris_sdk_0495/yu-ris_0495_031/システム/system/YSCom/YSCom.exe`(194,048 字节,PE32 x86) |
| 工具 | Ghidra 11.3.2 headless(JDK 21,`scripts/ghidra/decompile_all.py`,340 函数全量反编译) |
| 反编译输出 | `~/ghidra_all/YSCom.exe/`(本目录 `decompiled/` 保存了 5 个关键函数) |
| 日期 | 2026-09-03 |

> YSCom.exe 是官方脚本编译器。**编译器写什么字节,引擎就读什么字节** ——
> 它是 opcode 语义的最高等级证据(与引擎交叉验证后为 Confirmed)。

---

## 1. 启动与共享内存(印证《SDK编译器调用》一文)

`yscom_0041900c_filemap_check.c`:

- `OpenFileMappingA("YU-RISCompilerFileMapObject")` → `MapViewOfFile`
- **校验 `*(int*)(base + 0x80) == 0x1ef`(引擎版本 495)**,不符则弹窗退出
- 从 `base + 0x84` 起复制项目路径(对应文章结构体的 `uiEngineVersion` / `ucProjectPath`)
- 文章说 uiEngineVersion 在 uiUnknow1[0x1A] 之后 —— 实测偏移 0x80,与文章吻合

## 2. XOR 密钥推导 —— **Confirmed:CRC32(种子串) 的大端 4 字节**

`yscom_004199c8_key_init.c` + `yscom_00412ebc_crc32.c`:

```c
// 种子:PTR_DAT_004278c4(.rdata 指针)→ .data1 @0x7e7010 = "Yu-ris"
uVar3 = FUN_00412ebc(seed, strlen(seed));   // 标准 CRC32(IEEE 0xEDB88320 查表,表 @0x427240,已验证与 zlib 表逐项一致)
DAT_007e5004 = uVar3 >> 0x18;               // key[0] = crc >> 24
DAT_007e5005 = uVar3 >> 0x10;               // key[1]
DAT_007e5006 = uVar3 >> 8;                  // key[2]
DAT_007e5007 = uVar3;                       // key[3]
```

- **CRC32("Yu-ris") = 0xd36fac96 → 密钥字节 `d3 6f ac 96`**
- 《寻找脚本密钥》/《SDK编译器调用》记录的 SDK 默认密钥 **0x96AC6FD3** =
  把内存 4 字节按 LE u32 读的写法(字节序 d3 6f ac 96)——**两文互证,公式成立**
- 样本游戏密钥 `2b904f93` ≠ 默认值 → 该游戏种子串被改(商业版常见),
  种子串未在 kemonomichi2.exe 的可打印 ASCII 中暴力命中(CRC 目标 0x934F902B)——
  种子可能含非 ASCII 字节或来自其他来源,引擎侧应有同样的 CRC32 例程
  (引擎镜像内已找到标准 CRC32 表 @文件偏移 0x192c80)→ **待办**

## 3. YSTB 写出器 —— **Confirmed + XOR 相位模型修正**

`yscom_004160ac_ystb_writer.c`:

```c
write("YSTB", 4);            // magic 字符串直接引用数据段
write_u32(DAT_007e50a0);     // version (495)
write_u32(DAT_007e5188);     // unknown1
write_u32(part1_len); write_u32(command_len);
write_u32(content_len); write_u32(part4_len);
write_u32(0);                // unknown2
// 4 个区(part1/commands/content/part4)各自独立 XOR:
for (i = 0; i < len; i++) buf[i] ^= key[i & 3];   // key = DAT_007e5004
write(part1); write(commands); write(content); write(part4);
```

- header 8 字段布局与我们规格完全一致
- **XOR 相位模型修正**:每区**独立重新计数** `key[i & 3]`,不是跨区连续!
  我们的"绝对相位"模型之所以在语料上成立,是因为
  `part1_len == 4×unknown1`(恒为 4 的倍数)使两种模型等价。
  其他版本若 part1_len 非 4 倍数,两模型将分歧 —— 解密须按"分区重新计数"实现。

## 4. 表达式发射器 —— **Confirmed:全部字面量/运算符/变量 opcode**

`yscom_00409c6c_expression_emitter.c`(表达式编译主体,含运算符栈 `DAT_0042c41f/420`):

### 4.1 字面量(按值域自动选宽,`[op][len:u16][operand]` 逐字节写出)

| 条件 | 发射 | 总长 |
|---|---|---|
| int8 范围 | `42 01 00 (i8)` | 4 |
| int16 范围 | `57 02 00 (i16)` | 5 |
| int32 范围 | `49 04 00 (i32)` | 7 |
| int64 | `4c 08 00 (i64)` | 11 |
| 浮点字面量 | `46 08 00 (f64)` | 11 |
| 字符串(类型4,自动加引号) | `4d (len+2) 00 22 ...payload... 22` | 3+len+2 |
| 字符串(类型5,原文) | `4d (len) 00 payload` | 3+len |

### 4.2 运算符映射(tokenizer `FUN_0040c658` 定 token 类型 → 发射器压栈)

| 源码 | opcode | 语义 |
|---|---|---|
| `+` | 0x2b | add |
| `-`(二元) | 0x2d | sub |
| `-`(一元) | 0x52 | negate |
| `*` | 0x2a | mul |
| `/` | 0x2f | div |
| `%` | 0x25 | mod |
| `==` | 0x3d | equal |
| `!=` | 0x21 | not-equal |
| `>=` | 0x5a | ge |
| `<=` | 0x53 | le |
| `>` | 0x3e | gt |
| `<` | 0x3c | lt |
| `&`(单) | 0x41 | bit-and |
| `\|`(单) | 0x4f | bit-or |
| `&&` | 0x26 | log-and |
| `\|\|` | 0x7c | log-or |
| `^` | 0x5e | xor |
| `$()` | 0x73 | 转字符串 |
| `@()` | 0x69 | 转整数 |

(与 v555 语料 30 opcode 全部对上;手册 IF 页确认 `== != > >= < <= && ||` 存在,
basic 页确认 `+ - * / %` 存在 —— 三方一致)

### 4.3 变量引用

- 变量按**名字**先发射(0x48 + 名字载荷),随后 fixup 回扫把 `@/$/#/`` ` `` 前缀处
  按形态改写:**0x76**(token 类型 6,下标/成员形式)、**0x56**(token 类型 7,引用形式)
- 最终形态:操作数 3 字节 = `[前缀字符][id:u16]`,`@`=0x40、`$`=0x24、`#`=0x23、`` ` ``=0x60
  —— 与 v555 语料观测的 type_tag 完全一致
- **0x29**(操作数 1 字节,语料恒 0)= **数组元素读取**(`@A(2,3)` 的 `(...)`,
  发射处带维数校验:声明维数不符则编译报错)
- **0x2c**(无操作数)= 表达式组分隔/收尾(弹空运算符栈后发出;`,` 分隔符语义)

## 5. YSCom.ycd(编译器命令字典,v495)—— 结构 Confirmed

- magic `YSCD` + version 495 + command_count 119 + 0
- body:每命令 `{名\0, u8 参数数, 参数×{名\0, u16 a, u16 type}}`(参数记录 4 字节类型区)
- **第二 u16(type)与 v555 YSCM 的类型码 639/639 逐个一致**(跨版本类型系统稳定)
- 第一 u16(a)取值分布 {0:765, 1:137, 2:26, 3:130, 4:4, 5:21, 6:7, 8:3, 256:29},语义 Unknown
- 命令顺序与 v555 一致;495 有 CDDA/EMOTEINFO,555 新增 ALIAS/MOVIE2 族/PRODUCT
- tail(0x3043 起)= 系统标识符表(`_TIME`/`_PAI`/`_INT`/`_FLT`/`_STR`/`_PINT(n)`/`_LC`…,
  与手册"系统变量"页一一对应),细结构未解析

## 6. 其他产出管线情报(字符串证据)

- 编译产物:`%sysc.ybn`(YSCM)、`%sysbin\yst%05d.ybn`、`%sysbin\yst_list.ybn`(YSTL)、
  `%syscfg.ybn`(YSCF)、`%sysl.ybn`/`%sysv.ybn`/`%syst.ybn`/`%syse.ybn`、
  **`%sysi%05d.ybn`(新格式 YSI,用途 Unknown)**
- `global_f.yst` = 全局声明文件;`\macro.%s` = 宏展开机制;输入 `system\YSCom\YSCom.ycd`

## 7. 待办(下一步)

1. 引擎侧(kemonomichi2.exe,3502 函数已全量反编译至 `~/ghidra_all/kemonomichi2.exe/`):
   - 找引擎的 CRC32 密钥初始化调用点 → 提取 v555 商业版种子串(解出 `2b904f93` 的来源)
   - 找表达式求值主循环(注意:0x46e160/0x47e8ec/0x4fb8e2 的 switch 是 CRT memset
     或命令级分发,需按"栈操作特征"过滤;奇数值 0x0b~0x125 的 switch 是另一层分发)
2. `0x00/0x01/0x08` 三个低码 opcode(语料少量出现)—— 疑为命令级/流程控制,引擎侧确认
3. YSCD tail 系统变量表的元数据结构
