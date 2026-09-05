# YU-RIS 引擎命令层与执行循环（kemonomichi2.exe 逆向笔记）

| 项 | 值 |
|---|---|
| 目标 | `AnimalTrailGirlishSquare 2/kemonomichi2.exe`（2,114,560 B，PE32 x86，引擎 v555 / YPF 500） |
| 工具 | Ghidra 11.3.2 headless（3502 函数全量反编译 `~/ghidra_all/kemonomichi2.exe/`；命令处理器补建 85 函数 `extra/`） |
| 日期 | 2026-09-03 |
| 探针 | `scripts/probe_part1_groups.py`（全语料逐条断言）、`scripts/probe_tag_semantics.py`、`scripts/probe_engine_interp_scan.py`、`scripts/probe_engine_va.py`、`scripts/probe_yscm_index.py` |

> **结论概述**：YU-RIS v555 的「VM」是**命令级线程化解释器**。
> YSTB 的 part1 区是**命令实例表**；commands 区是**参数窗口表**；content(+part4) 是
> 共享表达式字节码池。流程控制（GO/GOSUB/IF/LOOP/RETURN）全部在**命令层**，
> 表达式层（30 个 opcode）不含任何流程控制指令 —— U2 就此解除。

---

## 1. YSTB 四区的新语义（全部 Confirmed：引擎加载器 FUN_00450dfd + 全语料 302/302）

```
header[8] = unknown1 = 命令组数 G
part1     = G × u32          ← 命令实例表
commands  = Σcount_i × 12B   ← 参数窗口表（每个窗口一条 12B 记录）
content   = ctlen 字节        ← 表达式字节码/文本 池
part4     = p4len 字节        ← content 的逻辑延续（部分窗口读入 part4）

part1[i]（u32 LE）:
  byte0 = 命令类型 = YSCM 命令下标（0..120）
  byte1 = 本组窗口数 count_i
  [2:4] = u16 参数 gparam（声明类命令 = 数组维数位图，见 §7）

窗口记录（12B）: tag(u32) / len(u32) / offset(u32，相对 content 起点)
```

**逐条断言（铁律 2）**：302/302 脚本满足 `part1_len == 4G` 且
`Σ(count_i×12) == command_len` 零失败；yst_list.ybn 是 YSTL 非 YSTB（预期外）。

已知现象更正：744 个窗口（149 文件，全部 tag0）满足 `off+len > ctlen`，
**读取越过 content 边界伸入 part4** —— content 池逻辑上延续进 part4，
`p4len == 4G` 恒等式的另一含义待解（§未解）。

## 2. 加载器 FUN_00450dfd（Confirmed）

```
obj = malloc(0x1c)（每脚本一份，按 yst%05d 序号惰性加载）
obj+0x00 ← header.unknown1（G）
四个区各自独立 XOR key[i&3]（引擎侧证实编译器的分区计数模型）
obj+0x0c ← malloc(G)   u8  数组：各组 count（来自 part1[i].byte1）
obj+0x08 ← malloc(G*2) u16 数组：各组 gparam
obj+0x10 ← malloc(G*4) 记录指针数组：ptr[0]=commands 基址，ptr[i+1]=ptr[i]+count_i*12
obj+0x14 ← content 基址；obj+0x18 ← part4 基址
特例（载入期解析，仅两类）:
  type 0x2a (GO):    解码窗口文本 → FUN_0045124c(哈希查标签) → 槽位 len = id|0x10000000
  type 0x2b (GOSUB): 遍历组内 tag==0 记录，同样解码回写 tag[2:4] = id (u16)
FUN_00451348() ← 加载后处理（标签表/声明组处理，待逆向）
```

运行期对象是更大的结构 DAT_00872404（= loader obj − 0x20，字段 +0x1c 起对齐），
**obj+0x00（unk1）在加载后被复用为运行时 PC**。

## 3. 主执行循环 FUN_0040449c（Confirmed）

```c
// 每个脚本任务一次
DAT_00872404 = 脚本运行时对象;
while (!退出标志 && !让出标志) {
    pc = obj->pc;               // obj+0x20 = 当前组下标
    obj->pc = pc + 1;           // 先取后增 → 跳转写 pc = 目标-1（GO 实际直接写目标后靠 +1 执行）
    让出标志 = A[sel][pc]();    // 处理器返回非零 → 挂起（WAIT/文本等待/错误）
}
// 挂起恢复：obj[+0x18] 倒计时、obj[+0x1c] timeGetTime 定时；任务队列轮转 FUN_0043be6c
// sel = DAT_0059b73c | (模式标志==0)：双处理器数组
//   X[0] = 数组0（加载后全部 = 0x45c4ec 消息泵/等待 stub）
//   X[1] = 数组1（= 命令处理器表 DAT_0078b020[type]）
```

- `DAT_0078b020` 在 **BSS**，由 FUN_0046305c 初始化：先全部填默认处理器
  `0x0045c4d4`（报错+让出），再逐个赋值 121 项中实际实现的命令
- `0x0045c4ec`（数组0 占位）= 等待模式泵消息（PeekMessage/Sleep(10)，返回 1 让出）
- **双数组的精确选择语义**（skip 模式 vs 执行模式的全局标志）= Likely，待定

## 4. 命令处理器（关键者，全部 Confirmed 反编译）

### GO（type 0x2a，恒 1 窗口）CMDH_0044272c
```
len 域高位 = 0 → 现场解码标签名 → FUN_0045124c → 标签 id（失败报错）
已解析     → id = len & 0x0fffffff
label = 标签表[id]  // {name,hash, 目标组PC:+8, 脚本号:+0xC(u16), 字符状态:+0xE/+0xF}
obj[0x3e/0x244] ← 字符状态（+ 调用深度偏移）
if 当前脚本号 != label.脚本号:
    惰性加载目标脚本（再调 FUN_00450dfd）并重绑 X/参数/计数/记录/内容/part4 全部指针
PC = label.目标组PC
```

### GOSUB（type 0x2b，1-12 窗口）CMDH_004428c0
```
1. FUN_00442d01() 求值条件窗口（w0）→ 假则直接返回（不跳）
2. gparam 解码本帧局部数组维数: int=(u16&0xff)>>3, flt=(u16&7)*4+(u16>>14), str=(u16>>9)&0x1f
3. 压帧: frame[ret_pc=pc+1, 脚本号(u16), 记录指针, 字符状态×2]；depth++
   （帧 0x328B：INT 局部 +8(8B/个) FLT +0x90 STR +0x120；
     返回值区 INT +0x160 FLT +0x1e8 STR +0x278；计数 +700/+0x2cd/+0x2de）
4. 窗口实参求值结果写入新帧局部
5. 标签解析: w 的 B2/B3 非 0 → 内联 id = B3*0x100+B2；否则取求值字符串 → 哈希
6. PC = label.目标组PC（跨脚本则重绑，同 GO）
```

### RETURN（type 0x4f，0-10 窗口）CMDH_0044b418
```
求值窗口 → 返回值写入调用者帧（INT +0x160 / FLT +0x1e8 / STR +0x278）
depth--；frame = frames[depth]
PC = frame.ret_pc；脚本号 = frame.脚本号（跨脚本重绑）
depth==0：脚本尾处理 FUN_00451724()，返回 -1（脚本执行完毕）
```

### IF（type 0x2c，恒 3 窗口）CMDH_004431ec
```
嵌套栈 obj[0x40 + level*4]（level = obj[0x3e]，<0x40）：push {字符状态, 本组PC, w2.len}
条件 = int求值表[w0.B2](&content[w0.off], w0.len)
条件假 → PC = (w1.len != 0) ? w1.len : w2.len
        ★ w1/w2 的 len 域存编译期算好的「目标组下标」（非运行期解析）
```
ELSE(0x2d, 0 或 3 窗口)/IFEND(0x30, 0 窗口) 同理配合；LOOP(0x37, 2 窗口) /
LOOPEND(0x3a, 0) / LOOPBREAK(0x38) / LOOPCONTINUE(0x39) 为循环块对
（语料统计：IF↔IFEND 各 8649、LOOP↔LOOPEND 各 946、ELSE↔IFBLEND 各 1517）。

### LET（type 0x35，恒 2 窗口）FUN_00443808
```
w0 = 左值（变量引用），w1 = 右值表达式
左值 kind 由变量描述符 DAT_0087240c[id] 的 byte+1 决定: 1=INT 2=FLT 3=STR
复合赋值码 = w0.B3: 0=赋值 1=+= 2=-= 3=*= 4=/= 5=%= 6=&= 7=|= 8=^=
变量空间（LHS id）:
  0x32=INT 命令声明的数组 / 0x33=FLT / 0x34=STR / 0x35=LET 局部（当前帧）
  0x75..0x78 / 0x90..0x92 / 0x119..0x19e → 系统变量表 DAT_00665c40
  0x118 → 位数组（index>>5）
元素 = 8 字节（i64/double），下标越界按声明计数报错
字符串连接走 0x1000 缓冲
```

### LABEL（type 0x33）= 运行期 no-op（FUN_00443668 返回 0）
标签在**加载期**注册（GO/GOSUB 解码时查表已存在）。

## 5. 参数求值器 FUN_004253cd（Confirmed）

MOVIE/RETURN 等处理器开头统一调用；对当前组逐窗口求值：

```
count = counts[pc]; records = recptrs[pc];
for i in 1..=count:
    B0 = records[i].tag & 0xff     ← YSCM 参数下标（param index）
    DAT_006624a0[B0] = i           ← 参数槽映射（处理器按 DAT_006624a0[slot] 取值）
    kind  = YSCM 参数表 low [命令id][B0]   （DAT_00872428）
    attr2 = YSCM 参数表 high[命令id][B0]   （DAT_0087242c = 校验规则）
    kind 0: i64 ← int求值表[B2](&content[off], len)
    kind 1: 字符串 ← str求值表[B2](→0x1000 缓冲)
    kind 2: 仅存原始指针（延迟求值）
    attr2 校验: 1/2=下限, 3..0x17=[min,max] 表(DAT_0059b5c0/620), 0x18:<0x1000001,
               0x19:>=0x1000000, 0x1a/0x1b:<=count-1, 0x1c:0<=v<4
```

求值表（FUN_00468160 初始化）：
`int[1]=00420bdc int[2]=00420c0c int[3]=00420bdc int[4]=00420c0c`
`flt[1]=00420c60 flt[2]=00420c8c …` `str[B2=16..18]=00420a90 str[19]=00420ba8`

## 5b. 表达式字节码解释器主循环（U2b 已关闭，Confirmed）

**thunk（00420bdc/00420c0c/00420c60/00420c8c/00420a90/00420ba8）**：
设模式标志（1=INT / 2=FLT / 3=STR）→ 调 **FUN_00420acc** → 从固定全局取 i64/f64/串结果。

**FUN_00420acc 主循环**（与我们的编码逐字段一致）：

```c
i = 0;
do {
    pc = base + i;
    i += *(u16*)(base + i + 1) + 3;          // [op:u8][len:u16][operand] ✓
    err = (*(code *)(&DAT_007e1660)[*pc])(); // 256 项处理器表，按 opcode 索引 ✓
    if (err) return -1;
} while (i < len);
// 串模式：循环后把结果串拷入调用方缓冲
```

**处理器表 FUN_0046a21c**（默认 = LAB_00423084，256 项）：全部已证实 opcode
的引擎处理器地址逐项对齐编译器语义：

| op | 处理器 | op | 处理器 |
|---|---|---|---|
| 0x2a mul | 00422cac | 0x42 pushint8 | 00421e44 |
| 0x2b add | 004225b0 | 0x46 pushfloat | 00422008 |
| 0x2c groupsep | **FUN_00423080(=ALIAS no-op!)** | 0x48 pushvar | 00420ec4† / 00421a3c‡ |
| 0x2d sub | 00422c48 | 0x49 pushint32 | 00421f34 |
| 0x25 mod | 00422dd8 | 0x4c pushint64 | 00421f9c |
| 0x2f div | 00422d24 | 0x4d pushstr | 00420cb8 |
| 0x3c lt | 00422a7c | 0x52 neg | 00422090 |
| 0x3e gt | 004229e0 | 0x53 le | 00422bac |
| 0x3d equal | 00422700 | 0x56 pushvarref | 004218b0† / 00421a3c‡ |
| 0x21 ne | 00422870 | 0x57 pushint16 | 00421ebc |
| 0x26 logand | 00422ee0 | 0x5a ge | 00422b14 |
| 0x29 arrayload | 00421a4c | 0x5e xor | 00423038 |
| 0x41 bitand | 00422e50 | 0x69 cast-int | 0042220c |
| 0x4f bitor | 00422e98 | 0x73 cast-str | 004220ec |
| 0x76 pushvaridx | 00421994† / 00421a3c‡ | 0x7c logor | 00422f8c |

† 运行期（FUN_0046a21c(1)）　‡ 载入期（FUN_0046a21c(0)，三个变量类共用占位
——加载器解码 GO/GOSUB 标签窗口期间变量不求值）
未列 opcode（0x00/0x01/0x08）= 默认处理器 LAB_00423084（报错）——
**引擎侧证实它们不是有效指令**，支持「tag0 截断伪影」假说（U2c Likely→加强）。

## 6. tag 位段语义（Confirmed(结构)/Likely(个别位)）

`tag = [B0][B1][B2][B3]`（LE 字节序）：

| 位段 | 语义 | 证据 |
|---|---|---|
| B0 (低字节) | **YSCM 参数下标**（参数槽选择；GOSUB/RETURN 稀疏填参 0x00/0x10/0x20…） | 求值器 DAT_006624a0[B0]=槽；YSCM 参数表按 B0 索引（Confirmed） |
| B1 | 未使用（全语料恒 0） | 语料统计 |
| B2 | **值类型**：0=通用/无 1=@INT 2=FLT 3=$STR 4=其他（F_INT→1、F_FLT→2、F_STR→3 逐项吻合） | 求值表按 B2 选择（Confirmed） |
| B3 | 次级编号（0-7）：复合赋值码（LET 左值）/ 标签内联 id 高位（GOSUB）/ 变量族 | LET/GOSUB 处理器（Confirmed(用法)/Likely(全表)） |
| tag==0 | IF/ELSE 的分支目标窗口（len 域存编译期组号）；GOSUB 的标签名窗口 | IF/GOSUB 处理器（Confirmed） |

旧「槽位级绑定模型 / tag>>16=实例编号」假设**作废**：观测到的 tag>>16 分布
实为 (B3<<8|B2) 的类型组合，非实例编号。

## 7. 命令类型 ↔ YSCM 下标全表（Confirmed，样本见 probe_part1_groups.py 输出）

- `0x0d`=END(431 组，基本 0 窗口=脚本尾)、`0x11`=F_INT(704)、`0x12`=F_STR(119)、
  `0x19`=FLT(295)、`0x2a`=GO(103，恒 1 窗)、`0x2b`=GOSUB(27065！最高频)、
  `0x2c`=IF(8649，恒 3 窗)、`0x35`=LET(18315，恒 2 窗)、`0x32`=INT(3093)、
  `0x5c`=STR(860)、`0x53`=S_INT(983)、`0x4f`=RETURN(3798)
- **声明类 F_*/G_*/S_* 的处理器 = 默认错误 stub** → 它们不由运行器执行，
  是**加载期数据**（变量声明：name/维数等），由 FUN_00451348 消费（待逆向）
- ALIAS(0)/DEBUGLIST(9)/PREP(77)/RETURNBREAK(80)/S_FLT…等 no-op 类与
  编译期命令一致（ALIAS=宏、RETURNBREAK=编译期标记）

## 8. YSCM tail（Confirmed，勘误）

tail(1045B) = **37 个 C 串（CRT 错误消息，日文）+ 256 字节映射表**，789+256=1045，
尾部无剩余。引擎 do-while `i<0x91, i+=4` **先解析后判步** → 恰 37 次；
按「35 次」朴素读法会巧合闭合（785+256+4）但把末两条空格串并进了表
——又一次「总和对得上 ≠ 逐条对得上」（已录勘误）。
此前「tail=系统变量表/配置键」猜测**证伪**（配置键是 SYSTEMMODE 的参数名，
在 body 内）。

## 9. 变量系统与 YSVR（Confirmed）

**运行期变量没有名字**：变量 = 纯 id（描述符表 `DAT_0087240c[id]`，
字段 {+0 类别, +1 类型(1=INT/2=FLT/3=STR), +2 维数, +4.. 各维边界}）。
LET 的 LHS id 是**声明命令 id**（0x32=INT/0x33=FLT/0x34=STR/0x35=LET 局部/
0x75+ 系统变量族）——名字只在编译期（ERIS 源码）存在。

**变量定义表 = `%ysbin\ysv.ybn`（YSVR）**，引擎 FUN_0046b7a0 载入
`DAT_0087280c`、FUN_00451348 消费（探针 `probe_vartab.py`，
**3362/3362 条目逐字节精确闭合**）：

```
YSVR: magic b'YSVR' + version(555)@+4 + u16 条目数@+8 + 条目流@+10
条目:
  +0 u8  kind        1=全局初值(1226) 2=按脚本初值(1226) 3=?(910)
  +1 u8  类别(desc[0])
  +2 u16 脚本号      (kind==2 的匹配键; FUN_00454bb0 存档过滤用 kind/类别×位掩码)
  +4 u16 变量 id
  +6 u8  类型        1=INT(2357) 2=FLT(127) 3=STR(426) 0=仅声明无初值(452)
  +7 u8  维数
  +8 维数×u32 各维边界
  然后: INT=i64 / FLT=f64 / STR=u16 len+bytes；type 0 无初值段
样本: id 范围 0..7570
```

**启动链 FUN_0046b63c（Confirmed）**：FUN_0046305c(YSCM+命令表) →
变量描述符分配 → FUN_0046b7a0(载 YSVR) → FUN_00451348(应用初值) →
FUN_00463c7c → FUN_0045124c(哈希查**入口标签**) → 建任务 →
FUN_00450dfd(加载脚本) → 设 PC → 运行。

其他容器 magic 实测：`yst.ybn` = **YSTD**(16B)、`ysl.ybn` = YSLB(139KB，疑标签表)。

## 9b. YSLB 标签表（Confirmed，2026-09-03 深夜）

**标签表 = `%ysbin\ysl.ybn`（YSLB）**，引擎 `FUN_00463c7c` 加载
`DAT_00872814`（标签数组）+ `DAT_007d1c40`（256 桶头）。样本
**4153/4153 条目、`murmur2(name)==存储哈希` 零失败、139051/139051 精确闭合**：

```
YSLB: magic b"YSLB" + version u32 + u32 标签数 + 256×u32 桶头(hash>>24 分桶)
      + 标签 × {u8 len, name, u32 hash=murmur2(name), u32 target_pc,
                u16 script_id, u8 flag_a(+0xE), u8 flag_b(+0xF)}
```

- **引擎查找哈希 = murmur2(seed=0)**（乘数 0x5BD1E995），4153 条全量验证
- 样本首条：`es.BT.W.GET` / 0x0000de4a / pc 24 / script 4
- 引擎兼容性怪癖：stride 按 len+13 读取，`flag_b` 实际吃到**下一条的 len 字节**
  （文件每条尾部实为 12B：len+name+hash4+pc4+script2+flag_a）
- **语料交叉验证（Rust 测试）**：GO 103 窗中 99 为干净 M-串、98 命中 YSLB
  （4 未命中 + 4 非 M-串 = 池化截断伪影，U2c 同族）；GOSUB tag0 M-串
  **27021 条、26942 命中（99.7%）**——未命中者是 GOSUB 的其他字符串参数
  （引擎查表失败静默跳过，不回写）
- 由此 **GO/GOSUB 跳转在 Rust VM 中可完整实现**（YSLB + 组号 + 脚本号）

## 9c. 小 id(≤999)系统变量读路径与 @48 定性（Confirmed，2026-09-04）

**读即计算、无变量存储**：pushvar(`00420ec4`)对带下标访问(或 DIM 标志)调
`0042158e`，其中 id<1000 分支按 id 二次派发到
`00447ebc`(INT)/`00447d4c`(FLT)/STR 路径 —— **switch(in_EAX) 的 case = 变量 id**。
标量访问(`00420fb2`)不进该 switch（id<1000 标量 = 帧/暂存区，见成果 50）。
已证实 case 表（`p5_sysvar/00447ebc_sysvar_read_int.c`）：

| case(id) | 变量 | 语义 | 依据 |
|---|---|---|---|
| 0x00-0x2f 内 0x00 | @0 | GetLocalTime 分行取值(u16 索引跳表) | case 0 |
| 0x01 | @1 | timeGetTime 派生计时(4 子槽) | case 1 |
| 0x05-0x0f | @5.. | GlobalMemoryStatus / 系统常量 | 各 case |
| 0x30 | **@48** | **内层 LOOP 迭代计数(1 基;无循环 = 0)** | 见下 |
| 0x31 | @49 | 恒 999 | case 0x31 |
| 0x32 | @50 | `@50[x]` = 变量 x 当前值(间接访问,经 desc 派发) | case 0x32 |
| 0x35 | @53 | **INT 帧局部数组**(当前帧 +0x10+idx×8,计数 +700) | 成果 50 |
| 0x38/0x39/0x3a/0x3b | @56-59 | 局部槽有效性(INT/FLT/STR 区计数 700/0x2cd/0x2de) | 各 case |
| 0x3c | @60 | **INT 返回值数组**(当前帧 +0x168+idx×8,计数 +0x2ef) | 成果 50 |
| 0x3f-0x42 | @63-66 | 返回值槽有效性(计数 0x2ef/0x300/0x311) | 各 case |
| 0x43 | @67 | 当前脚本对象 +0x348 数组(Idx param_2) | case 0x43 |
| 0x46 | @70 | `@70[x]` = 变量 x 的类型字节(desc+1) | case 0x46 |
| 0x6c..0x223 | @108.. | 引擎全局状态(窗口/时钟/配置等,多为只读) | 各 case |

**@48 定性证据链（成果 51）**：

1. case 0x30（00447ebc）:读 `obj+0x244` 嵌套栈计数,栈顶记录 **+0x10(+0x14)** 8 字节
   = 返回值;栈空 → 0。
2. LOOP 处理器 `00445a00`（p5_sysvar/00445a00_CMD_LOOP_0x37.c）:压记录时
   `rec+0x10 = 1; rec+0x14 = 0` —— **迭代计数从 1 起**。
3. LOOPEND `00445ce8`:`counter(+0x10) < count(+0x18)`(有符号 64 位) → counter++ 且
   PC = rec+4(循环体顶);否则弹栈出循环。
4. LOOPBREAK `00445b9c`/LOOPCONTINUE `00445c54`:LV 参数 = 回退层数(默认 1);
   BREAK 置栈顶 counter=0/count=0 后 PC = rec+8(配对 LOOPEND,再经 LOOPEND 弹栈)。
5. **GOSUB `004428c0` 不压该栈** —— 只把当前栈深字节存入帧 +1,RETURN 恢复
   (子程序里的循环栈增长随 RETURN 整体丢弃);故子程序内无循环时 @48 = 调用者
   当前迭代(跨帧可见)。
6. 语料验证（s190 yst00190.ybn,924 组）:@48 只读不写,出现在
   - g316/g317 正向搜索 `POS = @6347 + @48`(LOOP 内逐位推进)
   - g332/g333 反向搜索 `POS = @6350 - @48 - @6351 + 2`(LOOP 内 @48 递增 → POS 从
     串尾向串头扫描;**@48=0 时 POS=9 越界 8 字节串 —— Rust VM 曾因此报
     0x1d4d4 越界,正是缺此定性**)
   - g113-g133 文本折行 `@6312*( @48-1)+1`(行号 = 迭代计数)
7. 引擎同族处理:LOOPBREAK 后、配对 LOOPEND 前无 @48 读取路径(语料 0 例),
   VM 以 truncate 建模 BREAK,与引擎「清零记录 + 经 LOOPEND 弹栈」在 @48
   可观测面上等价。

VM 落地:`Evaluator::loop_counter` 注入(`yuris-script/src/eval.rs` load_auto/aload)
+ `GroupVm::read_var_value` @48 直读 `loops.last().counter`
(`yuris-vm/src/lib.rs`);回归测试 `sysvar_at48_is_loop_iteration`
(`crates/yuris-vm/tests/runtime_loop.rs`)。

## 10. 未解清单（更新）

| # | 项 | 状态 |
|---|---|---|
| U2 | ~~流程控制~~ | **已解除**（命令层；表达式层无流程指令） |
| U2b | 表达式层最终解释器（00420bdc/00420c0c/00420a90 thunk 的目标） | 待展开（预期与编译器 opcode 表一致） |
| U2c | 语料 0x00/0x01/0x08 表达式 op | Likely = tag0 前缀截断窗口的解析伪影，待复核 |
| U3 | tag B1 / B3 全表语义 | B1 Unknown（恒 0）；B3 已知两种用法 |
| U4 | v555 种子串 | 引擎初始化拷贝 "revsiruy"（"yu-ris" 反写）到 DAT_006603e0，但该缓冲随后被复用；真正 CRC 调用点未定位 |
| U5 | ~~part4 == 4G 恒等式的含义~~ | **已解除(Confirmed)**：content 与 part4 是逻辑上一大段，
`content ‖ part4` 为完整表达式/文本池,窗口 offset 相对该拼接段;194,634 窗在拼接后
全部无越界(`window_bytes_pooled_copy` 已按此实现)。part4 长度恰 4G 只是分配巧合,
非"每组 u32 表" |
| U9 | ~~FUN_00451348~~ | **已解除** = YSVR 变量初值应用器（§9）；标签注册仍在（疑 FUN_00463c7c 或 FUN_004637d8） |
| U10 | 双处理器数组选择标志的精确语义（skip 模式） | Likely |
| U11 | YSTD（yst.ybn 16B）/ YSLB（ysl.ybn 139KB）/ YSER 结构 | Unknown（新发现） |
