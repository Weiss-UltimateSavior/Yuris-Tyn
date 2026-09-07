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
下标 1 元素的字符串**字符数**(汇编终证 2026-09-08,见 §5)。

## 3b. script190 g31→g36 实链(LENGTH 消费点,2026-09-08 破案链)

- g31 VARINFO:`LENGTH($55[1])` → `@6293`($55[1] = 系统按钮请求路径串,
  如 `config/sound_3/btn_01`+SJIS名+`_bt4`,37B/31 字符)。
- g36 VARACT COPY:`POS = @6293−@6292+1`、`LENGTH = @6292`(=@53[2]=5)
  → **「取串尾 LENGTH 个字符」惯用法**;尾缀用于按钮命名派生。
- 本实现曾把 LENGTH 实现为字节数(37)→ POS=33 > 31 字符 → 每帧
  0x1d4ca 同族报错,系统脚本链停摆(known-issues 问题 1,成果 73 结案)。

## 4. 落地边界(铁律)

- **只读槽可执行**:TYPE/STRTYPE/DIMNUM/DIMSIZE*/LENGTH/SEARCH/STRFIRST/SJISCODE —
  求值记录 `VmEvent::VarQuery`,不写游戏状态。
- **写回槽走 Unsupported**:SET/LET/CUT/COPY/UPPER/LOWER/…/PUSH/POP/INIT/G_* —
  未逆向写回目标,不猜。
- `0x67` 的 a2/a3… 分支内对 INT/FLT 槽的"调试写入"
  (`DAT_005ca920/DAT_005ca8a0`)是临时调试信息,非变量存储 —— 不实现。

## 5. LENGTH 语义终证(2026-09-08 汇编,推翻旧「Likely 字节数」)

**CMDH_004550a0 LENGTH-on-STR 分支(0x4551dc-0x455208)逐指令:**

```asm
4551dc: xorl  %eax,%eax          ; count = 0 ← 结果寄存器
4551de: xorl  %esi,%esi          ; off = 0(仅步进用)
4551e0: movzbl (%esi,%edi),%ecx  ; ch = str[off]
4551e4: movzbl 0x59b0c0(%ecx),%ecx
4551eb: incl  %ecx               ; step = 宽度表+1
4551ef: cmp $2,%ecx / jne 4551f5
4551f4: incl %esi                ; 双字节:额外跳 1 字节
4551f5: incl %eax                ; count++(每字符恰 +1)
4551f6: incl %esi
4551f7: cmpl %edx,%esi / jl 4551e0   ; off < strlen(仅边界)
4551fb: cltd; mov %eax,0x8(%esp)     ; 结果 = count(64 位)
455208: calll 0x45baf8               ; push 字符数
```

- strlen(edx) 只作**循环边界**;真正入栈的是 EAX **字符计数器**。
- 即 **LENGTH = 字符数**(SJIS 双字节按 1 计),非字节数。
- 连带定性:VARACT 守卫 0x1d4ca/0x1d4d4 在真引擎为**致命错误**
  (`FUN_0046bea4(x,1)` → `DAT_008725dc=1` → 主循环调 FUN_00410de4 →
  WM_CLOSE 退出),非可恢复跳过 → 真引擎从不命中该守卫。

## 6. 未解(Unknown)

- 各写回槽的引擎写回目标(变量存储区 vs 临时缓冲)。
- `es._strlen` 等 GOSUB 封装的调用约定细节。
