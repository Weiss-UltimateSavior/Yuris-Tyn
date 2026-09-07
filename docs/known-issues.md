# 已知问题详单 —— VM VARACT 停摆 / 系统 UI 素材解析失败

> 记录日期:2026-09-08。样本游戏:AnimalTrailGirlishSquare 2(引擎 v555 / YPF v500)。
> 证据来源:33 分钟实机运行会话日志(约 12.2 万行) + `scripts` 探针对封包的全量
> 清点(鲁棒 `YpfIndex::from_path`,见文末「复现与工具」)。结论分级遵循
> PROGRESS.md 证据等级纪律,与 `CONTEXT.md` 术语表用语一致。

***

## 问题 1:VARACT 槽 3 越界 → VM 每帧重试同一组,系统脚本链永久停摆

> **✅ 已结案(2026-09-08,成果 73)**:根因 = 本实现 VARINFO
> LENGTH-on-STR 返回字节数,引擎返回**字符数**(汇编终证 0x4551dc)。
> 修复后 VARACT 零错误,VM 越过 s190 pc=36,系统 UI 链整体复活。
> 以下为破案过程存档,1.6 为结案报告。

**优先级:最高**(VM 侧一切后续进展被此点卡死)

### 1.1 现象与量化

运行日志中反复出现同一族错误,33 分钟会话累计 **121,889 条 ≈ 每帧 1 条(60fps)**:

| 错误变体 | 次数 | 占比 |
| --- | --- | --- |
| `槽 3 POS=37 越界(0x1d4ca 同族)` | 108,948 | 89% |
| `槽 3 POS=38 越界` | 3,353 | 3% |
| `槽 3 POS=33 越界` | 2,720 | 2% |
| `槽 3 POS=32 越界` | 870 | <1% |
| `槽 3 LENGTH=5 越界(0x1d4d4 同族; s190 pc=36 串字节=35 POS=31 ops=[(3,0,Int(1)), (4,0,Int(31)), (5,0,Int(5))])` | 854 | <1% |
| `槽 3 POS=34 / POS=31 越界` | 810 / 334 | <1% |

关键观察:

- **POS 值随会话状态漂移**(31~38),但单一值长期占主导 → 失败点的 POS
  操作数来自**变量**(非脚本常量),变量状态在长时间内稳定。
- 唯一带上下文的样本(LENGTH 变体,日志自行打印)把失败点钉在
  **s190(yst00190.ybn) pc=36**,对象串 **35 字节**,POS=31、LENGTH=5。
- 错误每小时 ~21.8 万条的速率意味着 **VM 从未越过首个失败组**。

**步骤 1 补上下文后的决定性数据(2026-09-08 补跑,90 秒 / 10,124 条错误)**:

> `s190 pc=36 串字节=37 ops=[(3,0,Int(1)), (4,0,Int(33)), (5,0,Int(5))]`
> —— 全部错误同属 **s190 pc=36 一处**;与旧会话 LENGTH 样本
> (同 pc=36,串字节=35,POS=31)合并,呈现精确算术关系:
> **POS = 串字节 − 4,LENGTH = 5**(两会话长度不同均自洽)。
> 即「拷贝末 5 字节」惯用法 —— **字节偏移语义(H2)在算术上闭合**:
> 区间 [POS−1, POS−1+5) 恰好是串尾 5 字节,永不越界;字符序数解读
> (37 字节 ≈ 19 字符 < POS+5)则必然越界。串内容/长度随运行状态变化
> (35↔37 字节,疑含时刻文本),POS 由脚本按字节长度派生。

**hex 取证补记(2026-09-08 报错补 hex 转储后)**:串内容 =
`config/sound_3/btn_01` + SJIS「ブルードラゴ」 + `_bt4`
(37 字节 = 21 ASCII + 6 双字节 + 4;**31 字符**)—— 即问题 2 的
UI 按钮请求路径;pc=36 在提取资源名尾缀做命名派生。上段「字节偏移
闭合」实为**幸存者偏差**:算式里的「串字节」是我们 LENGTH 查询的
返回值,它本身就被实现错了(字节数 37;引擎=字符数 31 →
POS = 31−5+1 = 27,字符语义下永不越界)。

### 1.2 代码链路(失败机制已闭环 —— Confirmed)

[yuris-vm/src/lib.rs](file:///Users/weiss/Desktop/yuris/Yuris-Tyn/crates/yuris-vm/src/lib.rs) 的 VARACT 处理器:

1. [lib.rs:1931](file:///Users/weiss/Desktop/yuris/Yuris-Tyn/crates/yuris-vm/src/lib.rs#L1931)
   命令分发 → 求值各槽;CUT(槽2)/COPY(槽3) 分支在
   [lib.rs:2058-2107](file:///Users/weiss/Desktop/yuris/Yuris-Tyn/crates/yuris-vm/src/lib.rs#L2058-2107):
   POS 取槽 4、LENGTH 取槽 5,`varact_char_to_byte`([lib.rs:4198](file:///Users/weiss/Desktop/yuris/Yuris-Tyn/crates/yuris-vm/src/lib.rs#L4198))
   按 **SJIS 字符步进**(0x81-9F/0xE0-EF 算 2 字节)换算字节偏移;
   起点越界 → `0x1d4ca` 报错,终点越界 → `0x1d4d4` 报错。
2. 报错经 `?` 上抛,**而 `self.pc += 1` 位于该命令臂末尾**([lib.rs:2217](file:///Users/weiss/Desktop/yuris/Yuris-Tyn/crates/yuris-vm/src/lib.rs#L2217))
   → **报错时 pc 不推进**。
3. [drive_vm](file:///Users/weiss/Desktop/yuris/Yuris-Tyn/crates/yuris-player-core/src/lib.rs#L121)
   收到 `Err` 仅打日志即 break → 下一帧重新 `run()` → **从同一组再次执行**
   → 同一变量状态下产出同一 POS → 同一错误。循环闭合,VM 永久停摆。

### 1.3 已知事实(Confirmed)与假设(分级)

**Confirmed:**

- s190 = yst00190.ybn,系统/UI 脚本;历史上 pc=333(成果 50)、pc=472(成果 61)
  两处卡点均已收口,当前停点是 s190 的第三个卡点(pc=36 一处已钉实,POS 变体
  点尚未定位——见 1.5 排查第 1 步)。
- 语料中 POS 是**运行期表达式**:`正向搜索 POS=@6347+@48`(s190 g317,
  成果 50)——@48 = 内层 LOOP 迭代计数(1 基)。
- 引擎侧 `0x1d4ca/0x1d4d4` 是 CMDH_00453178 CUT/COPY 分支的步进守卫地址;
  守卫含「POS∈{0,1} 短路」「空串不报错」特例(varact_varinfo.md)。
- 实现内部把 0x1d4ca 家族边界标注为「钳制到字符数的**近似**,真实语料该路径
  未观测到,等级 Likely」([lib.rs:2062-2067](file:///Users/weiss/Desktop/yuris/Yuris-Tyn/crates/yuris-vm/src/lib.rs#L2062-2067))——现在真实运行命中了它,
  近似被证伪,须重新定性。

**假设(最终裁决 2026-09-08,均经反编译/汇编终证):**

- **H1 上游状态分歧 → 证伪(其具体形态)**:变量内容与引擎一致
  (hex 取证 = 真实请求路径,非脏数据);真正的「分歧」是 LENGTH
  查询语义 —— 我们返回字节数,引擎返回字符数(见 H2' 终证)。
- **H2 字节偏移语义 → 证伪(汇编)**:CMDH_00453178 COPY 守卫循环
  `off += DAT_0059b0c0[ch]+1` 按**字符宽度表**走 POS−1 次 →
  POS=1 基字符序数、LENGTH=字符数。「POS=串字节−4」的算术闭合
  纯系纯 ASCII 串字节≈字符的巧合(幸存者偏差,见 1.1 补记)。
- **H2'(替代,Confirmed)**:引擎 VARINFO LENGTH-on-STR 返回**字符数**
  —— 汇编逐指令终证 0x4551dc-0x455208:strlen 只作循环边界,入栈
  结果 = 宽度表步进的 EAX 计数器(每字符 +1)。
- **H3 引擎可恢复性 → 证伪(反汇编)**:`FUN_0046bea4(0x1d4ca,1)` →
  `FUN_0046befc` 末尾 `DAT_008725dc=1`(与 WM_CLOSE 处理器
  FUN_00410de4 写同一标志)→ 帧驱动 FUN_00404164 返回 1 → 主循环
  FUN_00403bec(L88-91)调 FUN_00410de4 → WM_CLOSE 退出。**该守卫在
  真引擎是致命错误** → 真引擎从不命中 → 侧证 LENGTH 语义必然使
  POS 合法(H2')。

### 1.4 影响面

- VM 在首个失败组之后**全部不可达**:s190 及其后续的系统 UI 链(设置界面、
  存读档 UI、WINDOWINFO/FONTINFO 输出等)全部冻结。
- 游戏主流程(对话/立绘/voice)由 scenario 播放器并行驱动,不受影响 ——
  这正是「游戏能玩但系统界面全坏」的单点根因。

### 1.5 排查计划(已全部完成)

1. ~~**补上下文**~~ **✅ 已完成(2026-09-08)**:POS 变体补打
   `s{sid} pc={pc} 串字节 ops` 后,一次运行钉实**全部失败同属 s190 pc=36
   一处**,并暴露 POS=串字节−4 的字节偏移算术(见 1.1 步骤 1 数据)。
2. ~~**dump 失败组**~~ **✅ 已完成**:probe_group_windows.py dump s190
   g24-38 → g36 VARACT 五窗解码:`POS = @6293−@6292+1、LENGTH = @6292`;
   @6293 ← g31 VARINFO `LENGTH($55[1])`(槽 13);@6292 = @53[2] = 5。
   @6347/@48 为旧会话另一 POS 变体点,非本卡点。
3. ~~**守卫汇编复核**~~ **✅ 已完成**:见 1.3 裁决 —— H2 证伪
   (字符序数),H3 证伪(致命退出),H2' 终证(LENGTH=字符数)。
4. ~~**语义落地**~~ **✅ 已完成(成果 73)**:`exec_varinfo_query` 槽 13 与
   fallback 改 `sjis_char_len()`(与引擎 0x4551dc 循环同构);报错补
   hex/目标取证字段。
5. ~~**可恢复性对齐**~~ **✅ 结案(无需改)**:H3 证伪 —— 引擎该错误即
   退出,本实现的报错中止语义与引擎等价;修复根因后错误不再发生,
   无需「记录+跳过」。

### 1.6 结案报告(2026-09-08,成果 73)

**根因链(全部 Confirmed)**:

```
g31 VARINFO LENGTH($55[1])          ← 请求路径串(37B/31 字符)
  本实现: sjis_byte_len = 37        ← 错:字节
  引擎:   0x4551dc 步进计数 = 31    ← 对:字符
→ @6293 = 37(应为 31)
g36 VARACT COPY: POS = 37−5+1 = 33(应为 27)
→ varact_char_to_byte(obj, 32) 走 32 字符 > 31 字符
→ 0x1d4ca 同族报错 → pc 不推进 → 每帧重试 → 永久停摆
```

**修复**:`yuris-vm` 新增 `sjis_char_len()`(宽度表步进计数),替换
LENGTH 槽 13 及 fallback 的字节长实现;删除 `sjis_byte_len`。

**验证(可复现,100 秒窗口)**:

- VARACT 错误 **0 条**(修复前同窗口 10,124 条);全日志 0 错误。
- VM 越过 s190 pc=36,系统 UI 链复活:设置页/音量/标签页等全部控件
  请求序列出现(此前不可达)。
- 游戏推进到标题画面,主流程无回归。

***

## 问题 2:cgsys/config/* 系统 UI 素材解析失败(210 次/会话)

**优先级:高**(对应 README「UI 控件定位问题」的素材面)

### 2.1 现象与清单

启动与运行期间,VM 事件流请求的系统 UI 图片全部解析失败(共 **210 次**,
去重后 ~30 个路径):

| 请求路径(前缀 cgsys/config/) | 次数 |
| --- | --- |
| sound_2/tip_cgauge | 32 |
| other/btn_show_bt4 | 18 |
| sound_2/btn_votest_bt4 / btn_sysvoice_bt4 / btn_cslider_bt3 / btn_cmute_r_bt4 / btn_all_mask | 各 16 |
| other/btn_check_bt4 | 16 |
| sound/btn_all_mask | 3 |
| text/tip_mes_preview_1/2/3、btn_tab_system_1/2/3_bt3、btn_tab_sound_1/2/3、btn_tab_other_on、back_sound_3 | 各 2 |
| sound_3/btn_01~05+〈SJIS 日文名〉_bt4(按钮名含游戏场景名) | 各 2 |

### 2.2 封包实测对照(核心反证)

用鲁棒解析器全量清点:

- **cg.ypf**(5,655 条目):0 条含 "config" → UI 素材不在主 CG 包。
- **cgsys_ec.ypf**(4,708 条目):239+ 条 `cgsys\config\*`,全部
  `flag=0xCB`(stored PNG),与请求逐条对照:

| VM 请求 | 包内实存 | 判定 |
| --- | --- | --- |
| `sound_2/tip_cgauge` | `cgsys\config\sound\tip_cgauge.png`(1,750B, off 0xDA069F) | **目录变体**:无 `sound_2`,只有 `sound\` |
| `sound_2/btn_votest_bt4` | `cgsys\config\sound\btn_votest_bt4.png`(4,977B) | 同上 |
| `sound_2/btn_sysvoice_bt4` | `cgsys\config\sound\btn_sysvoice_bt4.png`(1,660B) | 同上 |
| `text/tip_mes_preview_1..3` | 仅见 `…tip_mes_preview_4.png` | 序号派生待查 |
| `btn_tab_system_1_bt3` | `cgsys\config\btn_tab_system_on.png`(根级、`_on` 形态) | **命名派生规则缺失** |
| `other/btn_show_bt4`、`btn_all_mask`、`btn_check_bt4` | 两包均未检索到 | 真缺失/他包/动态生成,待查 |

**结论:失败不是「包里没有」,而是 VM 请求路径与包内真实布局不匹配** ——
缺的是引擎「UI 控件逻辑名 → 实际文件」的**索引层**逆向(疑为 CGSYS/YGA
类系统资源表,尚未立案)。

### 2.3 三类子因

- **A. 目录变体**:VM 请求 `sound_2/`、`sound_3/` 子目录,包内只有
  `sound\`。`_2/_3` 疑为引擎运行期按皮肤/尺寸变体派生的查找路径,包内
  不存在对应目录;真实引擎要么有回退逻辑,要么请求形态本身就不同。
- **B. 命名派生**:VM 把标签页按钮推导为 `btn_tab_system_1_bt3`,包内实存
  `btn_tab_system_on.png`(根级、状态后缀 `_on`)——控件名/序号/按钮形态
  (`bt3/bt4`/`_on`/`_onover`)的推导规则缺失。
- **C. 真缺失**:`btn_show_bt4`、`btn_all_mask`、`btn_check_bt4` 及
  `sound_3/btn_01ブルードラゴン_bt4` 类 SJIS 名文件,在两包中均未检索到
  (SJIS 名按 lossy UTF-8 grep 有漏检可能,需字节级检索复核)。

### 2.4 旁证发现(单独立案候选)

- **前导杂字节条目名**:cgsys_ec.ypf 存在 `)cgsys\config\…`、
  `0cgsys_c\config\…`、`(cgsys_c\…`、`*cgsys_c\…`、`+cgsys_c\…` 形态的
  名字。疑为 YPF 虚拟根前缀族的新成员(已知 `$`/`%`/`9` 之外)或名字边界
  解析残余漂移 —— 会破坏精确名查找,须核实。
- **别名根**:`cgsys\…` 与 `cgsys_c\…` 大量条目指向同一 offset
  (如 tip_cgauge_b 同为 0xDA0D79)→ `cgsys_c` 是运行期别名根。
- **工具链缺口**:`yuris-tools` 的 `ypf list` 用严格 `YpfArchive::from_bytes`,
  对 cg.ypf / cgsys_ec.ypf 直接失败(`entry #0: name is not ASCII`);
  运行时用的鲁棒 `YpfIndex::from_path`(名字边界结构化校验,成果 70)才能
  解析。**建议 tools 切换到鲁棒解析器**,否则离线排查只能靠临时探针。
- flag 门控(`read_image_bytes` 要求 0xCB)不是本次失败原因(实测全 0xCB)。

### 2.5 影响面与排查计划

影响:设置/存档/文本窗等系统 UI 按钮无图(定位与点击由 VM 逻辑承担,
仅素材缺失);玩家可见为系统界面大面积空按钮。

> **✅ 部分结案(2026-09-08,成果 74)**:2.3 的 A 类(目录变体)与
> B 类(命名派生)归因为同一规则 —— 引擎 VFS 未命中时「从右向左逐层
> 剥离 `_单数字` token」回退(**Likely**,脚本↔封包对拍 6+ 例全中
> 0 反例;引擎侧代码未取证)。已在 `resolve_entry` 落地,CG 解析失败
> **208 → 151 条**,可解族(tip_cgauge×32、btn_votest/sysvoice/
> cslider/cmute×16 族、back_sound_2/3、btn_tab _1/_2/_3 全系)全部
> 命中加载。C 类经 **12 包字节级 XOR-0xC9 检索终证为包内不存在**
> (btn_all_mask、btn_c01~c16_bt4、other/*、back_other、
> btn_tab_other_on、sound_3/btn_01~65〈日文〉_bt4、
> text/tip_mes_preview_1..3 —— 包内仅 `_4`)→ 引擎同样命中失败,
> 按钮空图为引擎原生表现,剩余 151 条不再是分歧点,不再修。
> 2.4 前导杂字节定性更新:原始索引区 dump 证实为**盘上名字自带**
> 首字节(0x10~0x3B,语义 Unknown),非解析漂移;查找由 YpfIndex
> 剥根双索引(name[1..])免疫,不阻断。
> 以下 2.1~2.4 为破案过程存档。

1. ~~**逆向引擎 UI 资源索引层**~~ → 引擎解析代码不在反编译子集;
   以脚本(sid 253/254 es.BT.CG.SET 字面量)↔封包对拍代替,
   `_N` 剥离规则 Likely 定级(汇编取证后可升 Confirmed)。
2. ~~**对拍后修正 read_cg_bytes 候选推导/回退(如 sound_2 → sound)**~~
   → `resolve_entry` 落地 `_N` 变体剥离,实测 208→151。
3. ~~**字节级检索复核 C 类「真缺失」**~~ → 终证不存在(上表)。
4. ~~**核实前导杂字节名字**~~ → 盘上自带,非漂移,查找免疫(上表)。

### 2.6 遗留(降级为观察项)

- `text/tip_mes_preview_1..3` 剥 `_N` 后仍缺(包内仅 `_4`)——
  若后续取到引擎 trace,可核实引擎是否还有更低层回退(如任意同前缀
  兜底);当前与引擎同表现,不影响推进。
- cgsys_ec.ypf 名字首字节 0x10~0x3B 的语义(疑似与引擎哈希索引或
  分包路由相关)—— 无查找阻断,按需另立。

***

## 复现与工具

```bash
# 构建与运行(素材失败与 VARACT 洪水立即可见)
cargo build --release -p yuris-cli
target/release/yuris-cli run "/Users/weiss/Desktop/yuris/AnimalTrailGirlishSquare 2" 2>run.log

# 统计
grep -c "VARACT 槽 3" run.log
grep "CG 图像解析失败:cgsys/config" run.log | sort | uniq -c | sort -rn | head
```

封包清点探针(鲁棒解析器,本清单数据的采集方式):

```bash
cargo run -p yuris-format --example tmp_probe_cgsys -- <ypf路径> <子串> [上限]
# 例: cg.ypf 中 cgsys 条目 → 0 命中;cgsys_ec.ypf 中 config → 239+ 命中
```

> 注:探针文件 `crates/yuris-format/examples/tmp_probe_cgsys.rs` 为临时取证
> 工具,问题 2 闭环后可删除或转正为 `ypf grep` 子命令。
