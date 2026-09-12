# 游戏内 UI 缺失 —— 问题分析与计划

> 状态:分析完成,未实装。
> 方法:证据等级驱动逆向(Confirmed / Likely / Hypothesis / Unknown)。
> 取证日期:2026-09-08。前置:成果 81(标题子画面)之后、实机进入正篇验证时发现。
> **后续(2026-09-12)**:立绘档位/锚点、背景相机、UI 可见性与 ADV 主行
> 在 `docs/layout-fix-plan.md` 收口(成果 83);本文件的勘误 7/8 已回记
> PROGRESS 成果 82。

## 1. 问题现象

进入正篇(对话可玩切片)后,**原生的游戏内 UI 全部缺失**:

- 消息窗部件(窗体框、名字板、下一页指示符)——当前只有 `show_window_frame`
  的近似底框(`tip_meswindow_txspace` 拉伸至底部,z=80,成果 68);
- 系统按钮(backlog / auto / skip / config / menu 等常驻 ADV 部件)不上屏、不可点;
- 音量条等部件(tip_cgauge 族)只在设置类画面出现,亦未实装。

当前可见的只有 scenario 层驱动的:背景(SC_BG)、立绘/精灵(z=10)、
台词文本(SC_TEXT)、淡入淡出(SC_FADE)、选择肢(z=94/95)。

## 2. 根因分析

### 2.1 主因:VM CG 通道被整体抑制 —— **Confirmed(代码定位)**

本引擎是双脚本系统,**游戏内 UI 的数据源就是 YSTB 系统剧本经 VM 的 CG 通道**:

- 成果 62:s9 = 游戏 UI/按钮/精灵引擎(`es.GAMEMAIN.TIP.*` 纹理族),由
  GOSUB 帧局部实参驱动 —— 对拍实证 UI 部件全部经 VM CG 事件下发;
- 成果 73:VM 越过 s190 pc=36 后,系统 UI 控件请求序列大量出现
  (tip_cgauge、btn_votest、btn_tab_system 等)—— 请求面已通;
- 成果 75:剩余 151 条解析失败 = 产品未打包的可选素材,引擎原生容错,
  **不是缺口**。

而播放器侧(`crates/yuris-player-core/src/lib.rs` `consume_events`)
明确抑制了这条通道(成果 67 的 P8 集成取舍,注释原文):

```rust
// 注(P8 集成取舍):VM 的 CG 图层(含引擎 debug 覆盖层)不进场景 ——
// 视觉由 scenario 层驱动;VM 仅维护系统状态(变量/流程/文本事件)。
```

即:**纹理已预载进 backend(`loaded` 集合,rid = FNV-1a(id)),但从不建
场景层**。游戏内 UI 缺失是当时刻意的架构取舍,不是逆向缺口。

### 2.2 当年抑制的动因(合理性)—— **Confirmed(成果 67/69 记录)**

1. debug 覆盖层噪声:`cgsys\debug\btn_*` 走同一通道(成果 69 瓶颈 B3);
2. 与 scenario 层视觉冲突:当时先保「背景/立绘/台词」可玩;
3. 标题屏双份 UI 风险:内置标题(成果 76/79)与 VM 标题剧本(yst00259
   经 es.BT.* 注册的同名按钮)会叠加。

### 2.3 已就绪但未接线的能力 —— **Confirmed(代码)**

| 能力 | 位置 | 状态 |
|---|---|---|
| `VmEvent::Cg { pc, script_id, id, position(x,y,z), param_count, file }` | `yuris-vm/src/lib.rs` | 事件流完整(含槽 4/5/6 坐标) |
| `VmEvent::CgEnd { id, .. }` | 同上 | 显示结束通知 |
| **SceneBridge:Cg → upsert Layer(id=fnv(id), x/y/z=position)、CgEnd → hide** | `yuris-vm/src/bridge.rs`(成果 48 映射草案) | **已实现,未被播放器调用** |
| CG 纹理预载(rid = FNV-1a(id),与 bridge 哈希一致) | player-core `consume_events` | 已实现 |

即:接桥只差「开关 + 过滤 + z 序」三件事,不需要新的逆向。

### 2.4 次级缺口(接入后仍需逐项解决)

| # | 缺口 | 等级 |
|---|---|---|
| 1 | CG ~55 个 Unknown 槽(缩放/渐变/效果族;处理器 22942B 未逐段定性) | Unknown(成果 48 表) |
| 2 | position 槽 4/5/6 → x/y/z 映射 = Likely(逐槽未验证) | Likely |
| 3 | 合成 z 序:VM UI 层与 scenario 层(立绘 z=10、台词窗 z=80、选择肢 z=94)的相对顺序未设计 | Unknown(需截图对拍) |
| 4 | 按钮点击命中:**可能免费** —— es.BT.* 按钮的命中逻辑在 VM 剧本内,靠 @133/@138(光标注入,成果 67)+ $55[1](点击注入)自判;接入后需实机验证 | Hypothesis |
| 5 | debug 覆盖层与标题/配置屏的双份 UI 需过滤规则 | Confirmed(必须) |

## 3. 修复路线(P8.2b:VM UI 层接入)

> 状态:第 1/2 步已实装(成果 82,2026-09-08);首验发现位置/生命周期两问题,
> 同日二次修正(见 3.1);第 3/4 步待实机对拍。

### 3.1 首验勘误(2026-09-08,实机截图驱动)

- **问题 A「UI 堆在厂商 LOGO 左上角 (0,0)」**:
  - 位置:es.BT 部件坐标是**注册后**由 `es.BT.XY.SET` 单独设置 —— 宏体转储
    (`tmp_label_lookup.rs` YSLB 查址 s9 pc=64 + `tmp_yst_dump.rs` 组1083)
    实证 XY.SET 的实参转发给 **CGACT(0x02) 槽 B0=0x0c=X / 0x0d=Y**
    (XY.SET(531,10) → CGACT 槽 0x0c=531/0x0d=10),而 VM 原把 CGACT 当
    「不触碰注册表」处理 → 坐标全丢 → 全部 (0,0) 叠加;
  - 时机:厂商期场景门控只挡了标题等待期,挡不住厂商 CG 段。
- **问题 B「进入游戏后没有」**:boot 期部件注册时被过滤(未建层),且
  `reset_title_layers` 会隐藏 VM UI 层 → 进游戏后既无层也不会再发事件。

**二次修正**:
1. **VM**:`CGACT` 槽 0x0c/0x0d → patch `cg_registry.x/y`(命中已注册 CG);
   新增 `GroupVm::cg_registry_snapshot()`(名称字节 + x/y)。
2. **门控升级**:`title_seen`(title_screen 置位)∧ 非标题等待期才放行 ——
   厂商 CG 段完全不建层;
3. **重放**:`rebuild_vm_ui()` —— 放行沿(false→true,即进游戏)按注册表
   重建 VM UI 层(纹理 fnv(名) 命中已预载集才建),boot 期被过滤的部件恢复;
4. **生命周期分层**:`reset_title_layers` 不再隐藏 VM UI 层(系统 UI 跨
   场景持续,原生语义);隐藏移入 `title_screen`(标题接管视觉)。
5. **勘误 5(二次实机:`VM UI 重放 0 层`)**:首版把通道过滤放在了纹理
   预载之前 —— boot 期(门控关)一条纹理都没载,重放按「已预载」判定
   全跳过。修正:**预载与建层解耦**(非 debug 一律预载,建层才受门控)。
6. **勘误 6(三次实机:叠层错乱 + 位置问题)**:
   - **z 全固定 60** → 按钮被台词/底框压住:改 `vm_ui_z_for_path` 分带
     (路径含 `btn_` = 95 台词上;底框/提示 = 70 立绘上台词下;实现选择,
     原生 Z 槽语义 Unknown);
   - **position=None 的 CG.SET(只换纹理/patch 族)把坐标打回 (0,0)**
     (左上角小方块残留根因)→ 改「未指定槽保持现有坐标」(引擎保持
     原值语义,成果 62);
   - **XY.SET 晚于 CG.SET 的时序**:CGACT 落库注册表后,已建层不跟随
     → `consume_events` 处理 `CgAct` 时经新 API `GroupVm::cg_position`
     把权威坐标同步到现有层;
   - **双重底框**:VM 消息窗部件上屏后,scenario 近似底框(SC_WIN)
     撤画(show_text 门控 + 重放后隐藏);
   - **重放泄漏**:重放此前会把标题/配置部件(纹理已预载)一并拉进
     正篇 → 与 consume_events 同一 `vm_ui_layer_allowed` 过滤;
   - `CgState` 增 `file` 字段(最近非空 FILE),快照返回四元组。
   注:截图中画面上下黑边 = letterbox(窗口比例 ≠ 16:9)正常现象。
7. **勘误 7(四次实机:按钮组不全,与参照图对照)**:参照图(引擎原版)
   右下按钮组 = LOG/AUTO/SKIP/SAVE/LOAD/Q.SAVE/Q.LOAD/音量/齿轮,而
   运行图只有 main 系的 AUTO/CONFIG/音量 + 缺失存档组。**根因:过滤
   白名单仅 `cgsys/main/`,而 SAVE/LOAD/QS/QL 素材在 `cgsys\saveload\`**
   (子目录分布各异)。修正:放宽为全部 `cgsys\` 系统 UI(deny 仅 debug
   /title/extra —— 内置标题与 EXTRA 子画面接管),斜杠归一(VM FILE 用
   `/`,成果 66)后判定。剩余单件错位(如 AUTO 落右上)待全组上屏后与
   参照图逐项对拍。
8. **勘误 8(五次实机,重放清单诊断)**:重放清单(158 层)揭示叠层错乱
   真根因 = **同钮按态各注册一个 CG,且各态同坐标**(如
   `ES.GAMEMAIN.BTN.VOICEM.BT.OFF/OVER/ONOV=0=1` 三份同 @(434,1044);
   另有 texticon/icon_01..96 全叠 @(1706,975)、count/icon 叠 @(800,300))。
   引擎按态切换可见性,而我们全显 → 态图互相叠印。修正:**显示层按
   es.BT 基名收敛一层**(名尾 `.BT.<STATE>=..` 剥离为基名;态优先级
   OFF<ON<OVER<ONOV<NA,低者优先;仅 NA 亦显示),texticon/counticon
   族随之坍缩;层 id 改 fnv(base),CgAct/CgEnd 同口径;重放与实时
   consume 同一规则;`vm_ui_prio` 记录基名当前态优先级。诊断清单
   (`[vm-ui]` 行)保留为对拍工具。态切换(悬停 OVER/按下)的引擎机制
   (疑 CGACT)仍未逆向 —— 当前恒显 OFF/NA 态。

- [x] **第 1 步 过滤规则**(成果 82,`vm_ui_layer_allowed`):
  - `file` 路径含 `debug` → 跳过(引擎 debug 覆盖层);
  - `Wait::TitleMenu` 存续期间全部跳过(`ScenarioPlayer::in_title_menu()`
    → `Player::tick` 传 `allow_vm_ui=false`;内置标题接管视觉,避免
    yst00259 双份按钮);
  - 通道白名单:**仅 `cgsys/main/`**(消息窗部件/常驻按钮;title/config/
    extra 屏由内置子画面接管);`file` 空/None 的注册件(成果 62
    BT.OVER 族)跳过。
- [x] **第 2 步 接桥**(成果 82):`consume_events` 对通过过滤的 `Cg`
  事件 `upsert_layer`(id=fnv(id)、x/y=position 槽 4/5、z 固定 60、
  resource=fnv(id));`CgEnd` → `hide_layer(fnv(id))`;纹理沿用既有
  预载;`vm_ui_layers` 登记层 id,标题进入时统一隐藏。
  层位置 = 原生尺寸逻辑坐标(1920×1080),slot z 未用作层序(不猜)。
- [x] **第 2.5 步 位置/生命周期修正**(勘误 4):CGACT 落库 + 注册表重放 +
  title_seen 门控(见 3.1)。
- [ ] **第 3 步 z 序与消息窗合流**(z 分带/底框去重/坐标时序已做,勘误 6;
      **剩余 = 引擎截图对拍**):部件精确锚点(如「03」页码标签的落位)
      与 z 带边界值(95/70)待与引擎同场景截图逐项校准;
- [ ] **第 4 步 按钮功能验证**(待实机):backlog/auto/skip/config 点击
      是否经既有输入注入(@133/@138 + $55[1])直接生效;生效则 P9.2 大幅
      减负,不生效再逆向 es.BT 命中链。

### 实装记录(成果 82)

- `consume_events(allow_vm_ui)` 重写:过滤 → 预载 → `upsert_layer`;
  `CgEnd` 隐藏;`vm_ui_layers` 登记与标题进入时统一清理;
- `ScenarioPlayer::in_title_menu()` 公开标题等待态;
- `GroupVm::cg_registry_snapshot()` + CGACT 槽 0x0c/0x0d 落库;
- `PlayerCore::rebuild_vm_ui()` 注册表重放(门控沿触发);
- 单测 `vm_ui_filter_rules`(debug/标题期/非 main 三条规则)。

### 验收标准

- 正篇对话中:消息窗部件 + 常驻系统按钮上屏,位置/层级与引擎截图对拍
  (B4 同类,需引擎侧同场景截图);
- 无 debug 覆盖层噪声;标题屏无双份 UI;
- backlog/auto 等按钮点击有引擎等价反应(或明确记录未生效原因)。

### 风险登记

| 风险 | 等级 | 缓解 |
|---|---|---|
| VM CG 事件高频重建层(每帧 CG.SET?) | Unknown | 接桥时按 id 去重(已注册仅 patch 槽位,成果 62 CG 注册门控语义) |
| 55 Unknown 槽错误应用导致错位/闪烁 | Likely | 只应用 Confirmed/Likely 槽(id/position/file),其余忽略不猜 |
| 标题/配置屏双份 UI | Confirmed | 第 1 步过滤规则 |
| 事件量大 → 每帧场景重建开销 | Unknown | upsert 幂等,layer 数量有限(注册件) |
