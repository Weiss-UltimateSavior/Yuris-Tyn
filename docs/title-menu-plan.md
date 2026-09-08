# 标题界面按钮问题分析与计划

> 状态:P1 保真度核心已完成(成果 78/79);悬停/SE/功能面存在遗留差距(P2/P3)。
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

### 2.3 音效侧(Confirmed 缺失面 / Unknown 映射)

- `sysse.ypf` 实存 6 个系统音效:`sysse\sse01.ogg` ~ `sse06.ogg`;
- 播放器标题流程(`title_screen`/`poll_title_menu`)**无任何 `play_se` 调用**(代码检索实证);
- sse01~06 与 悬停音/决定音/取消音 的具体映射 **Unknown**(需逐个试听或从引擎/脚本引用验证)。

**补充取证(2026-09-08,bn.ypf yst00259)**:标题菜单由 **YSTB 脚本驱动**
—— `yst00259.ybn` 引用 `title/btn_start`(es.BT.CG.SET)+ `BTN.START`
(es.BT.NAME.SET)+ **`es.BT.SE.SET(sysse/sse02, sysse/sse03)`** —— 按钮
宏绑定了两个系统音效(疑悬停/决定,试听后可升级映射为 Likely)。
es.BT.* 宏族 = s9 系统宏库按钮工具箱(CG/XY/Z/SE/SET/NAME/GROUP.SET)。

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
| G4 | 按钮集 6 个(arasuji 已补,成果 79),缺 `config`(1477,745)/`manual`/`web`(后两者仅 off/over 双态;SCENE2/3 的 true/scenejump 不实装) | Confirmed |
| G5 | 悬停/点击无 SE;sse01~06 语义映射未验证 | Confirmed + Unknown |
| G6 | `title_load()`/`title_extra()` 为桩:LOAD/EXTRA 画面未实装 | Confirmed |
| G7 | END 疑似有确认对话框(`btn_confirm_title_bt4`)未实现 | Hypothesis |

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

### P2 反馈层

- [ ] **P2-1 sse 语义验证**:提取 `sse01~06.ogg` 试听/查时长,结合引擎 SE 引用(如有)建立映射表,写入 ypf.md 或 CONTEXT.md 术语。
- [ ] **P2-2 标题按钮接入 SE**:悬停进命中区播悬停音、点击播决定音(经 `play_se`)。

### P3 功能补全

- [ ] **P3-1 LOAD 画面实装**(`title_load()`):存档列表 UI + 读档 → 跳对应剧本。
- [ ] **P3-2 EXTRA 画面实装**(`title_extra()`):CG/BGM 鉴赏模式(素材已确认存在 `cgsys\extra\*`)。
- [ ] **P3-3 END 确认对话框**:验证 `btn_confirm_title_bt4` 用途后决定是否实现。
- [ ] **P3-4 config/manual/web 按钮取舍**:manual/web 为站外/说明书类,建议明确裁剪并记录。

## 5. 取证附件

- 提取工具:`crates/yuris-vm/examples/tmp_extract.rs`(播放器同路径 `read_image_bytes`);
- 三态样张:`/tmp/title_btn/cgsys_title_btn_start_{off,on,over}.png`、`btn_lastload_na.png`、`btn_arasuji_off.png`;
- 剧本转储:`scenario_start.txt`(889 B)/ `start.txt`(54 B)/ `ara.txt`(18061 B),工具 `/tmp/dump_sc.py`。
