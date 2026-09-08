# 游戏内 UI 缺失 —— 问题分析与计划

> 状态:分析完成,未实装。
> 方法:证据等级驱动逆向(Confirmed / Likely / Hypothesis / Unknown)。
> 取证日期:2026-09-08。前置:成果 81(标题子画面)之后、实机进入正篇验证时发现。

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

按风险递增分四步,每步可独立实机验收:

- [ ] **第 1 步 过滤规则**(B3 正解):
  - `file` 路径含 `cgsys\debug\` → 跳过(引擎 debug 覆盖层);
  - `Wait::TitleMenu` 存续期间全部跳过(内置标题接管视觉,避免双份按钮);
  - 台词窗部件(tip_meswindow 族)与 `show_window_frame` 二选一(建议保留
    VM 出窗体、scenario 只出文字,对齐原生;过渡期可先共存观察)。
- [ ] **第 2 步 接桥**:`consume_events` 对未过滤的 `Cg` 事件调
  `bridge.scene_mut().upsert_layer(...)`(id=fnv(id)、x/y/z=position、
  resource=fnv(id)、z 取 60 段起步),`CgEnd` → `hide_layer`;纹理已在
  `loaded`。风险点:`file` 为空/None 的 CG(成果 62:无 FILE 槽的注册件
  —— 如 BT.OVER 族)无纹理可挂,先记录跳过。
- [ ] **第 3 步 z 序与消息窗合流**:截图对拍定 VM 层 z(初值 60:高于
  立绘 10、低于台词窗 80);台词窗改由 VM 部件 + scenario 文字合成。
- [ ] **第 4 步 按钮功能验证**:接入后实机点 backlog/auto/skip/config,
  观察是否经既有输入注入(@133/@138 + $55[1])直接生效(2.4#4);
  生效则 P9.2 大幅减负,不生效再逆向 es.BT 命中链。

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
