# 标题界面按钮问题分析与计划

> 状态:P1 保真度核心 + P2 反馈层 + P3 功能画面均已完成(成果 78/79/80/81)。
> 遗留:config 按钮待原生配置画面实装(G4);原生 .sd 存档装载链未实现(LOAD 用本实现 JSON 存档)。
> 方法:证据等级驱动逆向(Confirmed / Likely / Hypothesis / Unknown)。
> 取证日期:2026-09-08。

## 1. 问题现象

1. 标题界面 5 个按钮(START/LOAD/CONTINUE/EXTRA/END)点击均无对应功能跳转,**无论点哪个按钮都进入开场**。
2. 按钮无悬停视觉反馈、无悬停/点击音效。

## 2. 取证证据

### 2.1 剧本侧:按钮选择经全局变量 G1 传递(Confirmed)

`sc.ypf → scenario\scenario_start.txt` 的 `#SCENARIO_TITLE` 段原文(节选):

```
#SCENARIO_TITLE

\TITLE

//
\GO.G.IF(1, "==",  1, SCENARIO_MAIN)
\GO.G.IF(1, "==",  2, ARA)

//
\GO(SCENARIO_MAIN)

\END
```

- `\TITLE` 为引擎内置标题画面,按钮选择结果写入**全局变量 1(G1)**;
- `SCENARIO_MAIN`(同文件)→ `\GO(maho2_01)` → 第一章开场 = **START 落点**;
- `ARA`(`scenario\ara.txt #ARA`,前情回顾 "Previously on Animal Trail ☆ Girlish Square!")= **OUTLINE(あらすじ)按钮落点**(G1==2);
- **兜底 `\GO(SCENARIO_MAIN)`:任何未写 G1 的路径都会落到开场** —— 这是"点哪个都进游戏"的直接机制。

### 2.2 素材侧:按钮三态 + 不可用态(Confirmed,cgsys_ec.ypf 实存并已提取查看)

| 后缀 | 语义(图像实证) | 例 |
|---|---|---|
| `_off` | 常态:粉底紫字 | `btn_start_off` |
| `_on` | 高亮:金字 + 星光装饰(悬停/聚焦) | `btn_start_on` |
| `_over` | 按下态(与 `_on` 近似,字节不同) | `btn_start_over` |
| `_na` | 不可用:灰暗色(CONTINUE) | `btn_lastload_na`、`btn_extra_na` |

按钮全集 9 个:`start`、`load`、`lastload`(CONTINUE)、`arasuji`(OUTLINE)、`extra`、`end`、`config`、`manual`、`web`。

另:`cgsys\config\system\btn_confirm_title_bt4.png` 存在,提示原生 END 可能有确认对话框(Hypothesis)。

### 2.3 音效侧(绑定面 Confirmed / 语义角色 Likely)

- `sysse.ypf` 实存 6 个系统音效:`sysse\sse01.ogg` ~ `sse06.ogg`(44.1kHz 单声道 OGG);
- **es.BT.SE.SET 双参绑定**(yst 系统脚本槽位级实证,成果 80):按钮宏以
  **槽 B0=0x21 = 悬停音、槽 B0=0x22 = 决定/取消音** 绑定两个音效路径。

**映射表(成果 80,槽位级全语料统计 + 时长佐证)**:

| 音效 | 时长 | 唯一参数窗口 | 语义 | 等级 |
|---|---|---|---|---|
| **sse02** | **0.086s** | 1075(29 文件) | **悬停音**(槽 0x21 恒定;极短促为悬停音特征) | Likely |
| **sse03** | 0.335s | 1031(28 文件) | **决定音**(槽 0x22,普通按钮) | Likely |
| **sse06** | 0.387s | 70(19 文件) | **取消/戻る音**(槽 0x22,BTN.BACK 型按钮) | Likely |
| sse01 | 0.303s | 4(仅 yst00013) | 动态派发(见下) | Unknown |
| sse04 | 0.390s | 6(仅 yst00013) | 动态派发(见下) | Unknown |
| sse05 | 0.356s | 6(仅 yst00013) | 动态派发(见下) | Unknown |

- 绑定面 Confirmed:全语料 `es.BT.SE.SET` 槽 0x21 恒 sse02,槽 0x22 ∈ {sse03, sse06};
- **sse01/04/05 无静态按钮绑定**:仅出现在 yst00013 的 IF 条件表达式
  `$1306[@1761] == "sysse/sse0X" && @1762 == 1 → es.SSE77.*` —— 经字符串数组
  $1306 的运行期队列动态派发,写入方未逆向,语义不猜;
- sc.ypf 剧本对 sse 无真实引用(命中均为 pressed/crosses 等英文子串);
- 等级说明:「槽 0x21=悬停 / 槽 0x22=决定」的角色指派为 **Likely**
  (参数序推断 + 时长佐证;引擎侧播放时机未做 watch 取证)。

**全量解码(2026-09-08,成果 79,工具 `/tmp/dump_ystb_groups.py`)**:
YSTB 静态解码得 **SCENE1 原生布局/绑定真值**(Confirmed):

| 按钮(★=NAME.SET Shift-JIS) | XY(es.BT.XY.SET) | CG | 绑定(es.BT.SET) |
|---|---|---|---|
| ★あらすじ(OUTLINE) | (1393,384) | btn_arasuji(312×29) | **BTN.START,参数 2** |
| ★スタート | (1393,439) | btn_start(317×76) | BTN.START,参数 1 |
| ★ロード | (1393,533) | btn_load | BTN.LOAD |
| ★前回からの続き | (1393,627) | btn_lastload_{off,over,on,na} 显式五参 | BTN.LLOAD |
| (无存档分支) | (1393,627) | btn_lastload_na | **BTN.LLOAD.NA** |
| ★おまけ | (1314,745) | btn_extra | BTN.CGMODE(+VOMODE 同位) |
| ★コンフィグ | (1477,745) | btn_config | BTN.CONFIG |
| ★終了 | (1640,745) | btn_end(146×47) | BTN.END |

- **BTN.START 参数 = G1 写入值**(arasuji=2/start=1),与 2.1 剧本侧
  `\GO.G.IF` 闭环;
- CONTINUE 由 ct=44 条件组按存档存在性二选一注册(双按钮同位);
- 素材勘误:`btn_load` **无** `_na`(LOAD 恒可用);`btn_extra_na` 存在但
  SCENE1 未绑定;manual/web 仅 off/over;
- 坐标系:LOGICAL 1920×1080 一比一,PNG 可见 bbox 无透明边距,
  XY = 图层左上角(旧目测布局 5~38px 非常数偏差已全列改正);
- BTDEF.SCENE1/2/3:SCENE2/3(通关后)无 start/arasuji,换
  btn_true/btn_scenejump(+na)(记录不实装)。

### 2.4 旧实现根因链(Confirmed,已修复)

1. `\TITLE` 命令旧实现用 `Wait::Line`:任意点击即推进,按钮只是装饰精灵,无命中检测;
2. 推进后不写 G1 → 2.1 的两个 `\GO.G.IF` 均不命中 → 兜底 `\GO(SCENARIO_MAIN)` → 必进开场;
3. 与 2.1 机制叠加,表现为"无论点哪个按钮都进游戏"。

**已修复(成果 76)**:`Wait::TitleMenu` 状态 + `poll_title_menu()` 按钮命中区检测 + `TitleMenuAction` 路由
(Start→推进执行循环 / Load·LastLoad→`title_load()` 桩 / Extra→`title_extra()` 桩 / End→退出)。
实机验证:各按钮不再误入开场,End 退出,未实现动作留在标题。

## 3. 遗留差距清单(对照原生)

| # | 差距 | 证据等级 |
|---|---|---|
| G1 | ~~Start 直接 `wait=None` 依赖兜底~~ 已修(成果 78:Start/Outline 写 G1 走 `\GO.G.IF`) | 已关闭 |
| G2 | ~~按钮默认绘 `_on` 高亮~~ 已修(成果 78:默认 `_off`) | 已关闭 |
| G3 | ~~无悬停/按下/`_na` 切换~~ 已修(成果 78 三态 + 成果 79 `_na`:仅 lastload,`btn_load` 无 `_na` 变体——**原"LOAD/CONTINUE 用 `btn_*_na`"表述证伪**) | Confirmed |
| G4 | 按钮集 6 个(arasuji 已补,成果 79):`manual`/`web` **明确裁剪**(成果 81 —— 站外跳转/说明书类,内核无对应功能);`config` 待原生配置画面(s250~254)实装后注册,不设占位钮 | Confirmed(取舍记录) |
| G5 | ~~悬停/点击无 SE;sse01~06 语义映射未验证~~ 已修(成果 80:映射表建立 + 悬停/决定音接入;映射见 2.3。sse01/04/05 动态派发语义仍 Unknown,不影响标题) | 已关闭 |
| G6 | ~~`title_load()`/`title_extra()` 为桩~~ 已修(成果 81:LOAD 存档列表 + EXTRA 落地菜单/CG 分页鉴赏/BGM 点播) | 已关闭 |
| G7 | ~~END 确认对话框未实现~~ 已修(成果 81:`confirm/dialog_end.png` + 通用 `btn_yes/btn_no` 三态钮;`btn_confirm_title_bt4` 为 config 系变体,未采用) | 已关闭 |

## 4. 行动计划

### P1 保真度核心(按钮机制对齐原生)

- [x] **P1-1 按钮三态绘制**:默认 `_off`;`cursor_logical` 命中时切 `_on`;按下帧切 `_over`。
      实现(成果 78):`TitleButton{rect,id,rids[3],shown}`;`title_screen()` 预载
      15 张三态素材,默认绘 `_off`;`update_title_buttons()` 每帧按命中态换层资源
      (Player::tick 在 scenario.tick 后调用)。验证:单测通过;**实机已确认
      (用户反馈按钮特效变化,成果 79 先导确认)**。
- [x] **P1-2 写 G1 走原生分支**:Start→`host.set_global(1,1)` 后 `wait=None`(经
      `\GO.G.IF` 落 SCENARIO_MAIN)。`ScenarioHost` 新增 `set_global` 默认方法。
      **勘误(成果 78)**:原 `@50[n]` 映射被运行时证伪(@50 dims=[1],idx=1 越界,
      "写 G1=1 失败…bound=1"),改为 PlayerCore 内部 `globals` 表(写入/读取
      自洽;qsave/quick_load 同步改挂,JSON 形状不变)。引擎真值存储 Unknown:
      全语料仅 scenario_start.txt 两处 `\GO.G.IF` 且均槽 1;引擎反编译无明文
      命令串,G 实体待 es.BT.* 宏链逆向(B5)。
      验证:回归单测 `title_start_writes_g1_and_branches` 通过 + 实机日志
      `写 G1=1` / `GO.G.IF G[1]=1 == 1 → SCENARIO_MAIN`。
- [x] **P1-3 补 OUTLINE 按钮**(成果 79):`btn_arasuji`(312×29)入列,层 id
      `0x5C_7000_000A`,原生位 (1393,384)(START 上方);`TitleMenuAction::Outline`
      → `set_global(1,2)` → `\GO.G.IF(1,"==",2,ARA)` 落前情回顾。
      **取证升级**:`es.BT.SET("BTN.START",2)` 绑定实锤(BTN.START 参数 = G1
      写入值);全列按钮坐标改正为 es.BT.XY.SET 原生值(旧目测 5~38px 偏差)。
      验证:单测 `title_outline_writes_g2_and_branches` 通过;实机待确认。
- [x] **P1-4 `_na` 态**(成果 79,**表述修订**:仅 CONTINUE 有 `_na`,
      `btn_load` 无 `_na` 变体,LOAD 恒可用 —— 原计划"LOAD/CONTINUE 用
      `btn_*_na`"证伪):无快存文件时 lastload 载 `btn_lastload_na` 单素材,
      `TitleButton.active=false`(不切三态、点击无效;原生 BTN.LLOAD.NA
      同位条件注册复现)。引擎原判据 save/*.sd,本实现以快存文件为判据。

### P2 反馈层(成果 80 完成)

- [x] **P2-1 sse 语义验证**(成果 80):工具
      `crates/yuris-vm/examples/tmp_sse_scan.rs`(bn.ypf 全语料 YSTB 解密 →
      池内 sse 扫描 → 命令组归属转储)+ `tmp_sse_count.rs`(槽位级唯一窗口
      精确计数)+ `crates/yuris-resource/examples/tmp_se_duration.rs`
      (symphonia 时长)。结论:es.BT.SE.SET 槽 0x21 恒 `sysse/sse02`
      (0.086s,悬停音)、槽 0x22 ∈ {sse03(决定,0.335s)、sse06(戻る,
      0.387s)};sse01/04/05 仅 yst00013 经 $1306 队列动态派发,语义
      Unknown。映射表入 2.3(等级 Likely:参数序推断+时长佐证)。
- [x] **P2-2 标题按钮接入 SE**(成果 80):`TitleButton.hovered` 边沿检测,
      `update_title_buttons()` 悬停**进入**命中区播 `TITLE_SE_HOVER`
      (sse02,保持不重复);`poll_title_menu()` 命中播
      `TITLE_SE_DECIDE`(sse03);新增 `play_sysse()`(空前缀 resolve,
      未命中记录决策不出声);`Audio::play_se` 带 name + `se_log`
      决策留痕(上限 32)。`_na` 灰钮沿用 active 门控:不响不响应。
      验证:`cargo test -p yuris-player-core` 8 测(新增 3:悬停边沿/
      点击决定音/灰钮静默,最小 GroupVm 夹具);workspace 全绿;
      release 构建通过;**实机目测待用户确认**。

### P3 功能补全(成果 81 完成)

- [x] **P3-1 LOAD 画面实装**(成果 81):`open_load()` —— `saveload/back_load.png`
      整屏 + 存档行(扫描 `save/yskernel_*.json`,mtime 排序,`fmt_unix_time`
      civil 算法显示时间,最多 8 行)+ 戻る(`saveload/btn_back` 三态)。
      槽位点击 → `request_load_path` → `Player::tick` 走 `restore_from(path)`
      (quick_load 泛化共用;读档完成 `start()` 重置)。空列表 =
      「セーブデータがありません」。**取舍**:原生 save/*.sd(YSVM 任务态)
      装载链未实现,列表为本实现 JSON 快存(与 F5/F9 同源)。
- [x] **P3-2 EXTRA 画面实装**(成果 81,勘误 2 重做):**EXTRA 直达 CG 鉴赏**
      (yst00257 原生流程,无落地菜单;初版自造落地菜单被证伪 —— 标题层透出
      致「UI 重复错位/透明黑」)。`cgmode/back`(3×3 白格画格底)+ ev 缩略图
      9 格/页(`load_thumb` 解码降采样直传)+ 全图查看 + 前·後页;
      **BGM 鉴赏**(顶部标签切换)= `extra/back`(EXTRAS 双列列表底)
      22 曲/页点播,戻る停止。标签行:原生 y=10 起,活动 `_on` 单图 /
      非活动 bt3n 图集 0 段裁剪(`load_atlas_segment`);返回钮 =
      `btn_back_bt3` 三态图集裁剪(蓝/淡/橙)。标签行 4 钮:CG/BGM 可切,
      RP(SCENE)/MV 以 `_na` 暗段禁用展示(st/wp/sv 素材缺失不布)。
      **取舍**:CG 无锁全量展示;画格锚点按白格实测非引擎真值(Likely)。
- [x] **P3-3 END 确认对话框**(成果 81):`request_quit()` → `open_confirm_end()`:
      压暗 + `confirm/dialog_end.png`(531×168)+ 通用 `confirm/btn_yes/btn_no`
      三态钮(146×45)。はい → `request_quit` 真退出;いいえ → 返回标题。
      `btn_confirm_title_bt4`(config 系变体)未采用。
- [x] **P3-4 config/manual/web 按钮取舍**(成果 81 裁剪记录):`manual`/`web`
      = 站外跳转/说明书类,内核无对应功能 → **明确裁剪**;`config` 需原生
      配置画面(s250~s254 系统剧本)实装后注册,不设占位钮。

## 5. 取证附件

- 提取工具:`crates/yuris-vm/examples/tmp_extract.rs`(播放器同路径 `read_image_bytes`);
- SE 映射工具(成果 80):`crates/yuris-vm/examples/tmp_sse_scan.rs`(全语料
  sse 扫描+组归属)、`tmp_sse_count.rs`(槽位级计数)、
  `crates/yuris-resource/examples/tmp_se_duration.rs`(symphonia 时长);
- P3 素材取证(成果 81):`crates/yuris-vm/examples/tmp_p3_probe.rs`
  (YpfReader 鲁棒解析:saveload/confirm/extra 计数 + PNG 尺寸 +
  thumb_cg↔ev 直映验证)、`tmp_yst_dump.rs`(yst 组转储)、
  `tmp_p3_probe2.rs`(extra 根素材/tab 变体)、`tmp_extract2.rs`
  (素材提取目验:cgmode/back 3×3 画格、bt3 三态图集、tab 4 态图集);
- 三态样张:`/tmp/title_btn/cgsys_title_btn_start_{off,on,over}.png`、`btn_lastload_na.png`、`btn_arasuji_off.png`;
- 剧本转储:`scenario_start.txt`(889 B)/ `start.txt`(54 B)/ `ara.txt`(18061 B),工具 `/tmp/dump_sc.py`。
