# 已知问题详单 —— VM VARACT 停摆 / 系统 UI 素材解析失败

> 记录日期:2026-09-08。样本游戏:AnimalTrailGirlishSquare 2(引擎 v555 / YPF v500)。
> 证据来源:33 分钟实机运行会话日志(约 12.2 万行) + `scripts` 探针对封包的全量
> 清点(鲁棒 `YpfIndex::from_path`,见文末「复现与工具」)。结论分级遵循
> PROGRESS.md 证据等级纪律,与 `CONTEXT.md` 术语表用语一致。

***

## 问题 1:VARACT 槽 3 越界 → VM 每帧重试同一组,系统脚本链永久停摆

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

**假设(待验证):**

- **H1(Likely)上游状态分歧**:POS=@6347+@48,若 @6347 基值或 @48 推进与
  引擎有分歧,POS 就落到越界值。旁证:35 字节 SJIS 串 ≈ 17 字符,POS=37
  远超 off-by-one 的量级;且 POS 值随会话状态(标题/对话)漂移。
- **H2(算术上强支持,待汇编终证)**:POS/LENGTH 是**字节偏移**(SJIS 步进
  只用于边界对齐)。POS=串字节−4、LENGTH=5 的「取串尾 5 字节」惯用法在
  两个不同串长上均精确自洽(见 1.1 步骤 1 数据);终证仍需
  FUN_00453178 CUT/COPY 分支守卫汇编(1.5 第 3 步)。
- **H3(Hypothesis)引擎可恢复性**:0x1d4ca 家族报错在引擎中可能是命令级
  可恢复错误(跳过/钳制后继续),不会停摆整条 VM;若是,本实现的
  「报错即中止整帧」策略把一次可恢复错误放大成永久卡死。

### 1.4 影响面

- VM 在首个失败组之后**全部不可达**:s190 及其后续的系统 UI 链(设置界面、
  存读档 UI、WINDOWINFO/FONTINFO 输出等)全部冻结。
- 游戏主流程(对话/立绘/voice)由 scenario 播放器并行驱动,不受影响 ——
  这正是「游戏能玩但系统界面全坏」的单点根因。

### 1.5 排查计划(按序执行)

1. ~~**补上下文**~~ **✅ 已完成(2026-09-08)**:POS 变体补打
   `s{sid} pc={pc} 串字节 ops` 后,一次运行钉实**全部失败同属 s190 pc=36
   一处**,并暴露 POS=串字节−4 的字节偏移算术(见 1.1 步骤 1 数据)。
2. **dump 失败组**:提取 s190 各失败组的窗口字节与运行时 @6347/@48 值,
   与引擎 trace 同组对拍,判定 H1(状态分歧)是否成立。
3. **守卫汇编复核**:重读 FUN_00453178 CUT/COPY 分支的边界检查(字节 vs
   字符),裁决 H2。
4. **语义落地**:按裁决结果改 `varact_char_to_byte` 路径(字节偏移)或修
   上游状态源(H1)。
5. **可恢复性对齐**(H3 证实后):0x1d4ca 家族改为「记录 + 跳过该组」,
   与引擎行为一致;同时消除每帧重试的日志洪水。

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

1. **逆向引擎 UI 资源索引层**:从引擎或 trace 中找到「控件逻辑名 → 文件名」
   的映射表(trace 的 LoadCG 请求序列即真值路径形态,含 `_2/_3` 变体与
   回退顺序)。
2. 对拍后修正 `read_cg_bytes` 候选推导/回退(如 `sound_2 → sound`)。
3. 字节级检索复核 C 类「真缺失」与 SJIS 名按钮的实存性。
4. 核实前导杂字节名字(虚拟根前缀 vs 解析漂移),决定是否修正 YpfIndex。

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
