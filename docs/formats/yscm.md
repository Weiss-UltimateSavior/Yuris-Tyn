# YSCM 命令字典格式规格(`ysc.ybn`)

| 项 | 值 |
|---|---|
| 状态 | **Confirmed**(结构 + 样本总量,可复现验证) |
| 样本 | `bn.ypf` 内 `%ysbin\ysc.ybn`(解压后 11,102 字节) |
| 引擎版本 | 555 |
| 验证 | `python3 scripts/probe_yscm.py <ysc.ybn>` + `cargo test --test sample yscm` |

> YSCM 是 YU-RIS **脚本语言的命令表**:每条命令的名字 + 参数名 + 参数类型码。
> `[YU-RIS] SDK编译器调用` 原文佐证:"那些字符串其实是按顺序的 Opcode 的名称和参数的名称"。
> 它是理解脚本命令语义的字典,但**不等于** VM 字节码的 opcode 表(见
> `docs/opcode/opcode-table.md` 的交叉比对结论)。

---

## 1. Header(0x10 字节,不加密 —— YSCM 在 YPF 中以 zlib 存储,解压即明文)

| Offset | Size | 样本值 | 字段 | 说明 |
|---|---|---|---|---|
| 0x00 | 4 | `YSCM` | `magic` | |
| 0x04 | 4 | `555` | `version` | 引擎版本 |
| 0x08 | 4 | `121` (0x79) | `command_count` | 命令条数 |
| 0x0C | 4 | `0` | `unknown` | 样本恒 0,语义 **Unknown** |

```rust
pub struct YscmHeader {
    pub magic: [u8; 4],      // b"YSCM"
    pub version: u32,
    pub command_count: u32,
    pub unknown: u32,
}
```

---

## 2. Body:command_count 条命令记录,变长连续排列

```
每条命令:
    name          C 字符串(明文,UTF-8/ASCII,0x00 结尾)
    param_count   u8
    param_count × {
        param_name    C 字符串(可为空串)
        param_type    u16 LE
    }
```

```rust
pub struct YscmCommand {
    pub name: String,
    pub params: Vec<YscmParam>,   // { name: String, ty: u16 }
    pub offset: usize,            // 调试用:条目在 YSCM 内的起始偏移
}
```

**样本实测(逐条断言通过,`scripts/probe_yscm.py`)**:

| 断言 | 结果 |
|---|---|
| 命令条数 == 121 | ✓ |
| 参数总数 == 1113 | ✓ |
| 命令名非空、可打印 ASCII、唯一 | ✓ |
| 参数名可打印(含空串,如 `COMPILEMODE` 的 1 个空名参数) | ✓ |
| body 在 `0x2749` 干净结束,无越界 | ✓ |

命令表开头:`ALIAS(0参数), CG(58), CGACT(71), CGEND(2), CGINFO(36), CLIPACT(2),
CLIPINFO(2), CSV(2), ...`
命令表结尾:`... PRODUCT(32), SYSTEMMODE(33), COMPILEMODE(1), RELEASEMODE(1),
PROJECTFOLDER(1)`
下标 0x75 = `SYSTEMMODE`,其参数含 `FILEPRIORITYDEVELOP/DEBUG/RELEASE`
(印证免封包文章)与 `BMP PNG JPG GIF AVI PSB WEBP WAV OGG`(媒体格式清单)。

---

## 3. 参数类型码(u16)—— 语义 Unknown,分布已实测

样本 1113 个参数的类型码直方图(精确值,`probe_yscm.py` 输出):

```
0x0000 ×366   0x0001 ×133   0x0002 ×37    0x0003 ×7     (低字节 0-3)
0x0100 ×36    0x0101 ×1     0x0200 ×20    0x0300 ×438   0x0301 ×1
0x0400 ×2     0x0500 ×10    0x0600 ×1     0x0700 ×7     0x0800 ×1
0x0900 ×2     0x0a00 ×1     0x0b00 ×1     0x0c00 ×23    0x0d00 ×3
0x0e00 ×3     0x1000 ×1     0x1400 ×2     0x1500 ×2     0x1600 ×3
0x1700 ×3     0x1800 ×1     0x1900 ×3     0x1a00 ×1     0x1b00 ×1
0x1c00 ×3                                     (合计 1113 ✓)
```

**可观察的规律(非结论)**:低字节 ∈ {0,1,2,3},高字节 ∈ {0x00..0x1c} ——
疑似「(类别, 子类型)」位组合标记,但与 YSTB slot tag(见 ystb.md §4)的
对应关系未确认。**不要猜**。

---

## 4. Tail(0x2749 起,1045 字节)—— 结构 Unknown

内容为二进制混杂(含 SJIS 消息片段与疑似分词器字符表
`ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789...`、`!"$#`、`%'&(-./` 等)。
原样保留,不做猜测性解析。

---

## 5. 未确认项

| 项 | 状态 |
|---|---|
| header.unknown(0x0C) | **Unknown**(样本 0) |
| 参数类型码语义 | **Unknown**(分布已实测) |
| tail 1045 字节结构 | **Unknown**(原样保留) |
| YSCM 命令 ↔ YSTB slot tag 的映射 | **Unknown**(已证伪"下标直配"假说,见 opcode-table.md §5) |
| 其他引擎版本的 YSCM 布局 | **Unknown** |
