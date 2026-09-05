# YSER (yse.ybn) 与 YSTD (yst.ybn) 格式规格

| 项 | 值 |
|---|---|
| 状态 | header **Confirmed**;池结构 **Confirmed(终点封闭)**;个别字段 **Unknown** |
| 样本 | `bn.ypf` 内 `%ysbin\yse.ybn`(YSER,6645B)/`%ysbin\yst.ybn`(YSTD,16B) |
| 引擎版本 | 555 |
| 验证 | `python3 scripts/probe_yser_ystd.py <bn.ypf>` |

---

## 1. YSTD (yst.ybn) —— 16 字节静态

```
+0  magic     b"YSTD"
+4  version   u32 LE = 555
+8  f8        u32 LE = 0x1dba (7610)   ← 语义 Unknown(值接近 YSVR 变量 id 上限 7570,未精确吻合)
+12 f12       u32 LE = 0
```

- **恒 16 字节**(探针断言 `len==16`,YSTD 名即 Static/定长)
- 引擎反编译语料**无 YSTD 直接引用** → 本引擎版本可能未使用该容器(存在性 + 结构 Confirmed)
- `f8` / `f12` 语义 **Unknown**,不猜

## 2. YSER (yse.ybn) —— 错误消息池

```
+0  magic      b"YSER"
+4  version    u32 LE = 555
+8  count      u32 LE = 0x7b (123)
+0x0c         (4 字节) 语义 Unknown(样本 = 00 00 00 00)
+0x10~0x13    (4 字节) 语义 Unknown(样本 = aa 86 01 00)
+0x14          连续 C 字符串池,直至文件尾
```

### 2.1 已确认(逐字节断言)

- header: `YSER` + ver 555 + count 123 —— 与 YSCM 的 121 命令数相近但独立
- **字符串池终点精确封闭**:从 0x14 起按 `0x00` 分隔连续解析,终点 == 文件长度
  (探针: 245 条,0x19f5 == 0x19f5,**零残留 / 零越界**)
- 内容 = **日文错误消息**(SJIS),例如:
  `メモリ不足です。`、`対応していないファイルです。`、`BMPファイルではないようです。`、
  `[内部エラー]\n\nパックファイル[%s]が\nオープンできませんでした。`、`%s` 等
- count=123(header) vs 池中 245 条 —— **不 1:1 对应**(池含全部含 `[%s]` 展开模板),
  实际「错误码 → 消息模板」映射关系 **Unknown**

### 2.2 Unknown(不猜)

- `+0x0c` / `+0x10` 两段 4 字节字段语义
- 245 条中 65 条含非 SJIS 字节 —— **池内可能嵌有 u32 长度前缀/分段标记**,
  个别 `0x00` 出现在 SJIS 文本中间导致的切分错位;精确的"记录板"结构待引擎消费点反编译确认
- count(123) 与池条目数(245)的对应规则

## 3. 与其他容器的关系

- 与 YSCM tail(37 条 CRT 错误消息)同类 = 错误/消息文本集合,但 YSER 是
  独立容器,引擎运行期通过错误码(C 代码的错误号)索引 —— 机制推断 Likely
- 与 YSCM 的 `ERROR`/`ERRORINFO` 命令(错误输出)推测相关(Likely)

## 4. 未确认项

| 项 | 状态 |
|---|---|
| YSTD f8/f12 | **Unknown** |
| YSTD 是否被本引擎使用 | **Confirmed(存在)/用途 Unknown** |
| YSER +0x0c/+0x10 | **Unknown** |
| YSER 记录板精确结构 | **Unknown**(池封闭已确认;无长度前缀的 C 串切分在个别处错位) |
| YSER 错误码 → 消息映射 | **Unknown** |
