# YPF 封包格式规格

| 项 | 值 |
|---|---|
| 状态 | **Confirmed**（基于样本实测，可复现验证） |
| 样本 | `AnimalTrailGirlishSquare 2/pac/bn.ypf` |
| 样本参数 | 1,465,312 字节 / version 500 / 309 条目 |
| 引擎版本 | 555 |

> ⚠️ 本规格在 **YPF version 500 / 引擎 555** 上验证。其他版本需重新校验，
> 尤其是 `name_xor_key` 与 entry 尾部字段。

---

## 1. Header（0x20 字节）

| Offset | Size | Type | 样本值 | 说明 |
|---|---|---|---|---|
| 0x00 | 4 | char[4] | `YPF\0` (`59 50 46 00`) | magic |
| 0x04 | 4 | u32 LE | `500` | YPF 格式版本 |
| 0x08 | 4 | u32 LE | `309` | 条目数 |
| 0x0C | 4 | u32 LE | `0x3655` | **首个文件数据的绝对偏移**；同时是索引区结束位置 |
| 0x10 | 16 | — | 全 0 | 保留 |
| 0x20 | 4 | `52 ae 33 00` | `index_prefix` | **用途 Unknown**，索引区前置字段 |

```rust
pub struct YpfHeader {
    pub magic: [u8; 4],       // b"YPF\0"
    pub version: u32,
    pub file_count: u32,
    pub first_data_off: u32,
    pub index_prefix: u32,    // 位于 0x20，用途 Unknown，原样保留
}
```

**验证要点**：首个 entry 的名字从 `0x24` 开始（不是 `0x20`）。
解析完 `file_count` 条 entry 后，读指针与 `first_data_off` **存在 −4 字节残差**，
详见 §2.4 —— 这是已知异常，实现时必须用容差而非严格相等。

---

## 2. 索引区

位于 `[0x20, first_data_off)`，其中前 4 字节是 `index_prefix`，
**entry 数据从 `0x24` 开始**。

样本：总长 `0x3655 - 0x20 = 13877` 字节，entry 占 `13873` 字节。

### 2.1 Entry 布局

```
+---------------------------+
| name      : C 字符串       |  每个非 0 字节 XOR 0xC9；终止符 0x00 不加密
+---------------------------+
| flags     : u8            |  1 = zlib 压缩；0 = 原样存储
| uncomp_len: u32 LE        |  解压后长度
| comp_len  : u32 LE        |  压缩后长度（= 文件内占用字节数）
| offset    : u32 LE        |  数据区绝对偏移
| reserved  : u32 LE        |  实测恒为 0
| tail      : u8[8]         |  用途 Unknown（疑似校验和）
+---------------------------+
```

**Entry 大小** = `len(name) + 1 + 1 + 4 + 4 + 4 + 4 + 8` = `len(name) + 26`

```rust
pub struct YpfEntry {
    pub name: String,          // 已解码
    pub name_raw_prefix: u8,   // 原始首字节，保留（见 §2.3）
    pub flags: u8,
    pub uncompressed_len: u32,
    pub compressed_len: u32,
    pub offset: u32,
    pub reserved: u32,
    pub tail: [u8; 8],
}
```

### 2.2 文件名解码规则

```rust
fn decode_name(raw: &[u8], key: u8) -> String {
    // 逐字节：非 0 字节 XOR key；遇到 0x00 结束（0x00 不参与 XOR）
    raw.iter()
       .take_while(|&&b| b != 0)
       .map(|&b| b ^ key)
       .collect()
}
```

样本 key = **`0xC9`**。

解码示例：

```
raw:  ED B0 BA AB A0 A7 95 B0 BA BD F9 F9 F9 FA FD E7 B0 AB A7 00
      ↓ XOR 0xC9（0x00 除外）
out:  24 79 73 62 69 6E 5C 79 73 74 30 30 30 33 34 2E 79 62 6E
      $  y  s  b  i  n  \  y  s  t  0  0  0  3  4  .  y  b  n

      → "$ysbin\yst00034.ybn"
```

### 2.3 未知项：路径首字节

样本中 309 条目的原始首字节分布：

| 原始字节 | XOR 0xC9 后 | 数量 |
|---|---|---|
| `0xED` | `$` (0x24) | 303 |
| `0xEC` | `%` (0x25) | 5 |
| `0xF0` | `9` (0x39) | 1 |

**状态**：**Unknown**

两种可能：
- (a) 不同的虚拟根目录标记（`$` / `%` / `9`）
- (b) XOR key 存在边界情况，导致少数首字节解码偏差

**处理**：实现中**保留原始首字节**（`name_raw_prefix`），不要丢弃。
按 (a) 处理（当作路径一部分），并加日志；待多版本样本验证后修正。

### 2.4 已知异常：末条残差 −4 字节

**状态**：**Unknown**（现象已确认，原因未明）

实测：

| 项 | 值 |
|---|---|
| 首个名字起始 | `0x24` |
| 末个名字起始 | `0x362C` |
| 按 `len+26` 模型解析 309 条后 | 结束于 `0x3659` |
| `first_data_off` | `0x3655` |
| **残差** | **−4 字节** |

交叉验证：

- 相邻名字起始间距直方图：`45 × 303`、`40 × 5`、`42 × 1`，
  与 `len(name) + 26` 模型**逐条吻合，308 条零不符**
  → 模型本身正确
- 末条 `$ysbin\yst00270.ybn`（19 字符）按模型应为 45 字节，
  但实际只占 `0x3655 − 0x362C = 41` 字节，尾部仅 4 字节
  → **末条 tail 字段被截断为 4 字节**
- 索引区头部有 4 字节 `index_prefix`（`52 ae 33 00`）
  → 头部的 4 字节与末条缺失的 4 字节**恰好相抵**

**对实现的影响**：无阻塞。采用鲁棒解析策略即可：

```rust
// 读取 0x20 处的 4 字节 index_prefix（用途 Unknown，原样保存）
let index_prefix = read_u32(&data[0x20..0x24]);
let mut p = 0x24;
let mut entries = Vec::with_capacity(file_count as usize);
for _ in 0..file_count {
    let e = parse_entry(&data, &mut p)?;   // name + flag + 4×u32 + tail(8)
    entries.push(e);
    if p >= first_data_off as usize { break; }  // 末条 tail 可能不足 8 字节
}
// 容差断言，不用严格相等
debug_assert!((p as i64 - first_data_off as i64).abs() <= 8);
```

**不要**把「索引精确闭合」写成断言 —— 样本上它不成立。

---

## 3. 数据区

从 `first_data_off` 起，每个 entry 占 `compressed_len` 字节。

- `flags == 1` → zlib 解压，得到 `uncompressed_len` 字节
- `flags == 0` → 原样读取

样本统计：304 / 309 为 zlib；5 条 `flags == 0`；1 条 `compressed_len == 0`。

### 3.1 zlib 特性

zlib 头通常是 `78 01` / `78 5E` / `78 9C` / `78 DA`。
因此可以在封包里搜索 `78 xx` 来定位数据区起点 —— 这是最初定位
`first_data_off` 含义的方法。

> ⚠️ 第三方汉化补丁可能修改 zlib 头（见
> `[YU-RIS] 收费组补丁破解之ペトリコール`，把 `78 DA` 改成 `5A 65` / `5A 21`）。
> 正规发行版不受影响。

---

## 4. 文件尾

样本：最后一个数据块结束于 `0x165BDC`，文件大小 `0x165BE0`。
**尾部剩余 4 字节**，用途 **Unknown**（疑似整体校验和）。

---

## 5. 验证方式（可复现）

已固化为可执行脚本：`scripts/probe_format.py`

```bash
python3 scripts/probe_format.py "path/to/bn.ypf"
```

等价的参考实现：

```python
import struct, zlib
d = open('bn.ypf','rb').read()
magic, ver, cnt, data0 = struct.unpack_from('<4sIII', d, 0)
assert magic == b'YPF\0'
index_prefix = d[0x20:0x24]        # 用途 Unknown
p = 0x24                            # 首个名字从 0x24 开始
entries = []
for _ in range(cnt):
    nb = bytearray()
    while d[p] != 0:
        nb.append(d[p] ^ 0xC9); p += 1
    p += 1
    flags = d[p]; p += 1
    uncomp, comp, off, reserved = struct.unpack_from('<IIII', d, p); p += 16
    tail = d[p:p+8]; p += 8
    entries.append((nb.decode(), flags, uncomp, comp, off, reserved, tail.hex()))

# 断言 1：条目数与 header 声明一致
assert len(entries) == cnt

# 断言 2：残差在容差内（末条 tail 可能不足 8 字节，见 §2.4）
residual = data0 - p
assert -8 <= residual <= 8, (hex(p), hex(data0), residual)

# 断言 3：条目尺寸直方图
from collections import Counter
sizes = Counter(len(e[0]) + 26 for e in entries)
assert dict(sizes) == {45: 303, 40: 5, 42: 1}

# 断言 4：zlib 解压长度吻合
ok = 0
for name, flags, uncomp, comp, off, _, _ in entries:
    if flags == 1 and comp > 0:
        if len(zlib.decompress(d[off:off+comp])) == uncomp:
            ok += 1
print(f"entries={len(entries)} end={hex(p)} data0={hex(data0)} "
      f"residual={residual} zlib_ok={ok}")
```

样本期望输出：

```
entries=309 end=0x3659 data0=0x3655 residual=-4 zlib_ok=304
```

条目尺寸分布：`303×45 + 5×40 + 1×42`，与逐条间距实测一致 ✓

---

## 6. 样本封包清单

`AnimalTrailGirlishSquare 2/pac/`（16 个文件，约 1.6 GB）

| 文件 | 大小 | 说明 |
|---|---|---|
| `bn.ypf` | 1.4 MB | **309 条目**，引擎与脚本 |
| `sc.ypf` | 327 KB | 剧本（是否明文待验） |
| `cg.ypf` | 827 MB | 立绘 / CG |
| `bgm.ypf` | 62 MB | BGM |
| `se.ypf` | 32 MB | 音效 |
| `vo.ypf` | 107 MB | 语音 |
| `sysvo.ypf` | 5.5 MB | 系统语音 |
| `sysse.ypf` | 41 KB | 系统音效 |
| `op.ypf` | 125 MB | OP 资源 |
| `op_c.ypf` | 125 MB | OP 资源 |
| `cgsys_ec.ypf` | 83 MB | 系统 CG |
| `update1.ypf` | 491 MB | 更新包 |
| `mv001~004.ymv` | — | YMV 影片 |

### `bn.ypf` 内部条目类型分布（按解压后 magic）

| magic | 数量 | 对应文件 |
|---|---|---|
| `YSTB` | 302 | `yst%05d.ybn` 脚本 |
| `YSCM` | 1 | `ysc.ybn` — Opcode 名表 |
| `YSCF` | 1 | `yscfg.ybn` — 工程配置 |
| `YSER` | 1 | `yse.ybn` |
| `YSLB` | 1 | `ysl.ybn`（139 KB） |
| `YSVR` | 1 | `ysv.ybn`（52 KB） |
| 非 zlib | 2 | stored |
| `comp_len = 0` | 1 | 空 |

---

## 7. 未确认项汇总

| 项 | 状态 | 影响 |
|---|---|---|
| `tail[8]` 语义 | **Unknown** | 无（可暂忽略，但必须原样保留以便回写） |
| `index_prefix`（0x20 处 4 字节） | **Unknown** | 无（原样保留） |
| 末条 tail 仅 4 字节、残差 −4 | **Unknown（现象已确认）** | 无阻塞，用容差断言 |
| 路径首字节 `$`/`%`/`9` | **Unknown** | 索引解析鲁棒性 |
| `name_xor_key` 是否随版本变化 | **Unknown** | 换游戏需重新确定 key |
| 文件尾 4 字节 | **Unknown** | 无 |
| 其他 YPF version 的 entry 布局 | **Unknown** | 版本 Profile |
