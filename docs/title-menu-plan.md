# 标题界面按钮问题分析与计划

> 状态:根因已定位并修复主体(成果 76);悬停/SE/功能面存在遗留差距。
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
| G1 | 当前 Start 直接 `wait=None`,依赖兜底 `\GO(SCENARIO_MAIN)` 进开场;未写 G1=1(非原生路径;且 OUTLINE 无法落地) | Confirmed(代码) |
| G2 | 按钮默认绘制 `_on` 高亮态(start/load/lastload/extra)+ END 用 `_off`,原生默认应全 `_off` | Confirmed(图像对比) |
| G3 | 无悬停 `_on` / 按下 `_over` 切换,无 `_na` 灰化 | Confirmed |
| G4 | 按钮集仅 5 个,缺 `arasuji`(剧本已支持 G1==2)/`config`/`manual`/`web` | Confirmed |
| G5 | 悬停/点击无 SE;sse01~06 语义映射未验证 | Confirmed + Unknown |
| G6 | `title_load()`/`title_extra()` 为桩:LOAD/EXTRA 画面未实装 | Confirmed |
| G7 | END 疑似有确认对话框(`btn_confirm_title_bt4`)未实现 | Hypothesis |

## 4. 行动计划

### P1 保真度核心(按钮机制对齐原生)

- [ ] **P1-1 按钮三态绘制**:默认 `_off`;`cursor_logical` 命中时切 `_on`;按下切 `_over`。
      实现:每帧对 5 个按钮层按命中状态换资源(`load_scenario_image` 缓存三态 ResourceId)。
      验证:实机目测悬停高亮、按下闪烁。
- [ ] **P1-2 写 G1 走原生分支**:Start→`G1=1` 后 `wait=None`(经 `\GO.G.IF` 落 SCENARIO_MAIN,删除对兜底路径的依赖)。
      验证:日志确认 `\GO.G.IF` 命中 SCENARIO_MAIN。
- [ ] **P1-3 补 OUTLINE 按钮**(`btn_arasuji_*`,G1=2 → `\GO.G.IF` 落 ARA,前情回顾直接可播)。
      验证:点击 OUTLINE 进入 ara.txt 播放。
- [ ] **P1-4 `_na` 态**:无存档时 LOAD/CONTINUE 用 `btn_*_na` 且点击无效(依赖存档系统状态,可先用"存档为空"近似)。

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
