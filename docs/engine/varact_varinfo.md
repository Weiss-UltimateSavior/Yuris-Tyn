# VARACT(0x66) / VARINFO(0x67) 语义笔记

| 项 | 值 |
|---|---|
| 证据 | `CMDH_00453178`(VARACT,5290B)+`CMDH_004550a0`(VARINFO,2775B)反编译;YSCM 参数名;真实调用(script190 g4) |
| 处理器 | VARACT→`00453178`;VARINFO→`004550a0` |

## 1. 结构(Confirmed)

- 两命令都先 `FUN_004253cd()` 参数求值,再按 **B0=参数下标** 槽分发
  (`DAT_006624aN` 槽标志,N=参数下标)。
- 变量描述符 `DAT_0087240c[id]` 按 byte+1 类型分支:1=INT/2=FLT/3=STR。
- 字符串读 `DAT_00662300`(槽求值区),写入 `malloc` 临时缓冲后
  `FUN_0045b9f8()` 显示查询结果;返回 0。

## 2. 参数表(YSCM)

- **VARACT(29 参)**:SET(0)/LET(1)/CUT(2)/COPY(3)/POS(4)/LENGTH(5)/TYPE(6)/
  UPPER(7)/UPPER2(8)/LOWER(9)/LOWER2(10)/HANTOZEN(11)/ZENTOHAN(12)/
  DIMSIZE(13)/PUSH(14)/POP(15)/INIT(16)/G_INT..G_STR4(17-28)
- **VARINFO(22 参)**:SET(0)/LET(1)/TYPE(2)/STRTYPE(3)/DIMNUM(4)/
  DIMSIZE..DIMSIZE8(5-12)/LENGTH(13)/SEARCH(14)/STRFIRST(15)/SJISCODE(16)/
  INT(17)/FLT(18)/STR(19)/NO(20)/NO2(21)

## 3. 真实调用(script190 g4,VARIABLE)

3 窗:`pushint8 1`(B0=0x0D → LENGTH=13)/引用 `$0x37[1]`(STR 数组元素)/
`pushvar @0x188e`。即 **LENGTH 查询**:`$0x37`(STR 数组,YSVR 17 元素)取
下标 1 元素的字符串字节长度(Likely:字节数;引擎用 `DAT_0059b0c0` 逐字符
步进,SJIS 双字节按 2 计 → **字节长度,非字符数**)。

## 4. 落地边界(铁律)

- **只读槽可执行**:TYPE/STRTYPE/DIMNUM/DIMSIZE*/LENGTH/SEARCH/STRFIRST/SJISCODE —
  求值记录 `VmEvent::VarQuery`,不写游戏状态。
- **写回槽走 Unsupported**:SET/LET/CUT/COPY/UPPER/LOWER/…/PUSH/POP/INIT/G_* —
  未逆向写回目标,不猜。
- `0x67` 的 a2/a3… 分支内对 INT/FLT 槽的"调试写入"
  (`DAT_005ca920/DAT_005ca8a0`)是临时调试信息,非变量存储 —— 不实现。

## 5. 未解(Unknown)

- 各写回槽的引擎写回目标(变量存储区 vs 临时缓冲)。
- `es._strlen` 等 GOSUB 封装的调用约定细节。
- LENGTH 精确语义:字节数 vs 字符数(Likely 字节,按步进表)。
