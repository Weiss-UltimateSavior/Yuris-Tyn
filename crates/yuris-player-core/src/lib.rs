//! 平台无关播放器核心(P-core)。
//!
#![forbid(unsafe_code)]
#![warn(missing_docs)]
//!
//! 从 yuris-cli 抽出:PlayerCore(ScenarioHost 实现)/ Player(tick/按键/快存快读)/
//! scenario 播放器 / rodio 音频 / letterbox 数学。winit 事件循环、窗口创建与
//! 输入翻译归平台壳(desktop `yuris-cli` / android `neko-android`);本 crate
//! 不依赖窗口创建路径 —— `WgpuBackend` 仅在壳持窗后注入。
//!
//! 依赖 winit 是经由 yuris-render 的 `Arc<winit::window::Window>`;winit 0.30
//! 官方支持 Android(android-activity 后端),故本 crate 可直接复用于移动端。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use yuris_render::{WgpuBackend, LOGICAL_H, LOGICAL_W};
use yuris_runtime::GraphicsBackend;
use yuris_scene::{Layer, ResourceId};
use yuris_value::{Value, VarRef};
use yuris_vm::bridge::{fnv1a, SceneBridge};
use yuris_vm::host::PacFileIndex;
use yuris_vm::{GroupVm, VmSuspend};

pub mod audio;
pub mod scenario;

pub use scenario::{ScenarioHost, ScenarioPlayer, TitleMenuAction};

/// 按平台候选路径加载 CJK 字体(繁中优先,其次简中/日文)。
pub fn load_cjk_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        // Windows
        r"C:\Windows\Fonts\msjh.ttc",
        r"C:\Windows\Fonts\simsun.ttc",
        // macOS
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        // Android(系统 Noto CJK;TC 优先 —— 语料为繁中)
        "/system/fonts/NotoSansTC-Regular.otf",
        "/system/fonts/NotoSansCJK-Regular.ttc",
        "/system/fonts/NotoSansSC-Regular.otf",
        // Linux(Noto CJK 常见路径)
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    CANDIDATES
        .iter()
        .find_map(|p| std::fs::read(p).ok())
}

pub fn letterbox_logical(ww: u32, wh: u32, px: f64, py: f64) -> (i64, i64) {
    if ww == 0 || wh == 0 {
        return (0, 0);
    }
    let sx = LOGICAL_W / ww as f32;
    let sy = LOGICAL_H / wh as f32;
    let s = sx.max(sy);
    ((px as f32 * s) as i64, (py as f32 * s) as i64)
}

/// Unix 秒 → "YYYY-MM-DD HH:MM"(civil 算法,无外部依赖;LOAD 存档行显示)。
fn fmt_unix_time(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, mi) = (rem / 3600, (rem % 3600) / 60);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

/// VM UI 部件 z 分层带(P8.2b 第 3 步初值;实现选择,待引擎截图对拍):
/// 按钮族(路径含 `btn_`)= 95(台词之上,可点击);底框/提示族 = 70
/// (立绘之上、台词之下)。原生 Z 槽语义 Unknown,先按功能分带。
fn vm_ui_z_for_path(path_lower: &str) -> i32 {
    if path_lower.contains("btn_") {
        95
    } else {
        70
    }
}

/// VM UI 层过滤(P8.2b):允许全部 `cgsys\` 系统 UI 素材(游戏内按钮/消息
/// 窗/存档钮等分属 main/saveload/config 等子目录 —— 成果 82 勘误 7:
/// 仅限 main 会把 saveload 的 SAVE/LOAD/QS/QL 组过滤掉,参考图按钮组
/// 不全),deny:debug 覆盖层(成果 69 B3)、title/extra(内置标题与子画面
/// 接管,防双份)。标题等待期由 allow_vm_ui=false 整体关闭。
/// VM FILE 参数用 `/`(成果 66),归一为 `\` 再判。
fn vm_ui_layer_allowed(path_lower: &str, allow_vm_ui: bool) -> bool {
    if !allow_vm_ui {
        return false; // 标题菜单等待期:内置标题接管视觉
    }
    let norm = path_lower.replace('/', "\\");
    if norm.contains(r"\debug\") {
        return false; // 引擎 debug 覆盖层
    }
    if norm.contains(r"cgsys\title\") || norm.contains(r"cgsys\extra\") {
        return false; // 内置标题/EXTRA 子画面接管
    }
    norm.contains(r"cgsys\")
}

/// es.BT CG 名 → (基名, 态优先级)。名字形如
/// `ES.GAMEMAIN.BTN.VOICEM."BT.OFF=0=1` —— 同一按钮各态(OFF/ON/OVER/
/// ONOV/NA)注册为**独立 CG**,引擎按态切换可见性(成果 82 勘误 8)。
/// 显示层必须按基名收敛为一层(低优先级优先;仅 NA 亦显示),否则
/// 3~4 个态图叠同一坐标(叠层错乱根因);texticon/counticon 族各 index
/// 也是同基名同态多份,一并坍缩。
fn vm_ui_base_prio(name: &[u8]) -> (Vec<u8>, u8) {
    let Some(pos) = name.windows(4).rposition(|w| w == b".BT.") else {
        return (name.to_vec(), 0);
    };
    let base = name[..pos].to_vec();
    let tok = &name[pos + 4..];
    let state_end = tok.iter().position(|&b| b == b'=').unwrap_or(tok.len());
    let prio = match &tok[..state_end] {
        b"OFF" => 0u8,
        b"ON" => 1,
        b"OVER" => 2,
        b"ONOV" => 3,
        b"NA" => 4,
        _ => 5,
    };
    (base, prio)
}

fn scene_mut_layer(scene: &mut yuris_scene::Scene, id: u64) -> Option<&mut Layer> {
    scene.layers.iter_mut().find(|l| l.id == id)
}

/// 三态按钮公共体(标题按钮与子画面按钮共用):按光标/按下帧计算每钮的
/// 目标态并切换层资源;返回 (层资源更新, 是否有悬停进入边沿)。
/// `_na` 灰化钮(active=false)不参与。
fn tri_state_pass(
    buttons: &mut [TitleButton],
    cursor: (i64, i64),
    clicked: bool,
) -> (Vec<(u64, ResourceId)>, bool) {
    let (cx, cy) = cursor;
    let mut updates: Vec<(u64, ResourceId)> = Vec::new();
    let mut hover_entered = false;
    for b in buttons {
        if !b.active {
            continue;
        }
        let [x, y, w, h] = b.rect;
        let hover =
            cx >= x as i64 && cx < (x + w) as i64 && cy >= y as i64 && cy < (y + h) as i64;
        if hover && !b.hovered {
            hover_entered = true;
        }
        b.hovered = hover;
        let want = if hover && clicked {
            TITLE_BTN_OVER
        } else if hover {
            TITLE_BTN_ON
        } else {
            TITLE_BTN_OFF
        };
        if want != b.shown {
            if let Some(rid) = b.rids[want] {
                b.shown = want;
                updates.push((b.id, rid));
            }
        }
    }
    (updates, hover_entered)
}

/// scenario 层资源/图层 id 基(与 SceneBridge 的 VM 层错开)。
pub const SC_BG: u64 = 0x5C_0000_0001;
pub const SC_FADE: u64 = 0x5C_0000_0002;
pub const SC_TEXT: u64 = 0x5C_0000_0003;
pub const SC_WIN: u64 = 0x5C_0000_0004;

/// 淡入淡出覆盖层动画状态。
pub struct FadeState {
    pub out: bool,
    pub start: Instant,
    pub ms: u64,
    pub color: [u8; 3],
}

/// 标题按钮三态下标(素材后缀 `_off`/`_on`/`_over`;成果 78 图像实证)。
pub const TITLE_BTN_OFF: usize = 0;
/// 悬停/聚焦高亮态。
pub const TITLE_BTN_ON: usize = 1;
/// 按下态。
pub const TITLE_BTN_OVER: usize = 2;

/// 标题按钮悬停音(sysse/sse02)。es.BT.SE.SET 第一参数全语料 444 组恒定,
/// 时长 0.086s 显著短促(其余 0.30~0.39s);成果 80。
pub const TITLE_SE_HOVER: &str = "sse02";
/// 标题按钮决定音(sysse/sse03)。es.BT.SE.SET 第二参数(普通按钮 316 组);
/// BACK 型取消钮为 sse06(128 组)—— 参数 2 随按钮语境变化,标题六钮均普通型。
pub const TITLE_SE_DECIDE: &str = "sse03";
/// 取消/戻る音(sysse/sse06)。es.BT.SE.SET 第二参数的 BACK 型取值(成果 80)。
pub const TITLE_SE_CANCEL: &str = "sse06";

/// 子画面层 id 基(P3:LOAD / EXTRA / CG 鉴赏 / BGM 鉴赏 / END 确认;
/// 与标题 0x5C_7000_* 与台词窗 SC_* 错开;z 在 50..=61 段)。
pub const UI_LOAD_BASE: u64 = 0x5C_8000_0000;
pub const UI_EXTRA_BASE: u64 = 0x5C_9000_0000;
pub const UI_CG_BASE: u64 = 0x5C_B000_0000;
pub const UI_BGM_BASE: u64 = 0x5C_D000_0000;
pub const UI_CONFIRM_BASE: u64 = 0x5C_E000_0000;

/// 子画面路由钮 id(常量而非算术 —— match 模式需要)。
pub const UI_LOAD_BACK: u64 = UI_LOAD_BASE + 1;
pub const UI_CG_BACK: u64 = UI_CG_BASE + 1;
pub const UI_CG_PREV: u64 = UI_CG_BASE + 0x90;
pub const UI_CG_NEXT: u64 = UI_CG_BASE + 0x91;
pub const UI_CG_TAB_BGM: u64 = UI_CG_BASE + 0xA1;
pub const UI_BGM_BACK: u64 = UI_BGM_BASE + 1;
pub const UI_BGM_PREV: u64 = UI_BGM_BASE + 0x90;
pub const UI_BGM_NEXT: u64 = UI_BGM_BASE + 0x91;
pub const UI_BGM_TAB_CG: u64 = UI_BGM_BASE + 0xA0;
pub const UI_CONFIRM_YES: u64 = UI_CONFIRM_BASE + 2;
pub const UI_CONFIRM_NO: u64 = UI_CONFIRM_BASE + 3;

/// 播放器子画面状态(P3)。均在 `Wait::TitleMenu` 等待下叠加显示,
/// 点击经 `poll_title_menu` 路由,scenario 播放器不感知。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubUi {
    /// 无子画面(标题菜单本体)。
    None,
    /// LOAD 存档列表(back_load 整屏)。
    Load,
    /// EXTRA CG 鉴赏(EXTRA 直达,原生无落地菜单;cgmode/back 3×3 画格)。
    ExtraCg,
    /// EXTRA BGM 鉴赏(extra/back 双列列表 + 曲目点播)。
    ExtraBgm,
    /// END 确认对话框(dialog_end + yes/no)。
    ConfirmEnd,
}

/// 标题按钮:命中区 + 三态资源 + 当前显示态(成果 78;坐标/绑定成果 79)。
pub struct TitleButton {
    /// 命中区 rect = x,y,w,h(逻辑坐标,原生尺寸)。
    pub rect: [f32; 4],
    /// 按钮层 id(0x5C_7000_0005..)。
    pub id: u64,
    /// 三态资源 [off, on, over](title_screen 预载;加载失败 = None)。
    pub rids: [Option<ResourceId>; 3],
    /// 当前显示态(TITLE_BTN_*)。
    pub shown: usize,
    /// 可用性(false = `_na` 不可用态:单素材、无三态切换、点击无效 ——
    /// 原生 es.BT 把无存档 CONTINUE 注册为 BTN.LLOAD.NA 独立按钮,成果 79)。
    pub active: bool,
    /// 上一帧悬停态(进入命中区的边沿触发悬停音,成果 80)。
    pub hovered: bool,
}

/// 播放核心(实现 ScenarioHost;与 scenario 分裂借用)。
pub struct PlayerCore {
    pub index: Arc<PacFileIndex>,
    pub vm: GroupVm,
    pub bridge: SceneBridge,
    pub loaded: HashSet<u64>,
    pub backend: Option<WgpuBackend>,
    /// 已消费的 vm.events() 游标。
    pub events_cursor: usize,
    /// WAIT FRAME 计数(引擎 obj+0x18)。
    pub wait_frames: u64,
    /// WAIT TIME 截止时刻(引擎 obj+0x1c)。
    pub wait_until: Option<Instant>,
    /// 光标逻辑坐标(@133/@138 注入)。
    pub cursor_logical: (i64, i64),
    pub key_pulse: bool,
    pub key_pulse_frames: u32,
    pub font: fontdue::Font,
    pub fade: Option<FadeState>,
    pub clicked: bool,
    /// scenario 已生成的精灵层(id)。
    pub sprites: Vec<u64>,
    /// 精灵淡入/淡出动画:(id, 开始, ms, 淡出?)。
    pub sprite_fades: Vec<(u64, Instant, u64, bool)>,
    /// 音频包索引 + 播放器(P9.1)。
    pub audio_packs: Arc<PacFileIndex>,
    pub audio: crate::audio::Audio,
    /// 活动选择肢(文本 + 命中区)。
    pub choices: Option<Vec<(String, [f32; 4])>>,
    /// 最近 CursorMoved 原始像素(winit 0.30 无 cursor_position 轮询)。
    pub last_cursor_px: Option<(f64, f64)>,
    pub frame_clicked: bool,
    /// 标题按钮(命中区 + 三态资源;成果 78)。
    pub title_buttons: Vec<TitleButton>,
    /// 标题 LOAD/LASTLOAD 请求(tick 内置,Player::tick 消费走快读)。
    pub request_title_load: bool,
    /// 标题 END 请求(主循环消费后退出)。
    pub request_quit: bool,
    /// 全局槽(\GO.G.IF 的 G=n;仿真内部存储 —— 引擎真值映射 Unknown/B5。
    /// 成果 78 勘误:原 `@50[n]` 映射被运行时证伪,@50 dims=[1],idx≥1 越界)。
    pub globals: HashMap<usize, i64>,
    /// 子画面状态(P3;close_subui 复位)。
    pub subui: SubUi,
    /// 子画面三态按钮(戻る/yes/no/标签页;悬停边沿播悬停音)。
    pub ui_buttons: Vec<TitleButton>,
    /// LOAD:存档条目(路径,显示标签;mtime 排序,新在前)。
    pub save_entries: Vec<(std::path::PathBuf, String)>,
    /// LOAD 选中条目(Player::tick 消费走 restore_from)。
    pub request_load_path: Option<std::path::PathBuf>,
    /// EXTRA CG:ev 清单(剥根规范名)与页码、正在查看的全图层。
    pub ev_list: Vec<String>,
    pub cg_page: usize,
    pub cg_view: Option<ResourceId>,
    /// EXTRA BGM:曲目名(去目录/扩展名)与列表页码。
    pub bgm_tracks: Vec<String>,
    pub bgm_page: usize,
    /// VM CG 通道建的场景层 id(P8.2b;标题进入时统一隐藏)。
    /// 层 id = fnv(es.BT 基名)(勘误 8:同钮各态收敛一层)。
    pub vm_ui_layers: Vec<u64>,
    /// es.BT 基名 → 当前显示态优先级(低值优先显示;OFF=0)。
    pub vm_ui_prio: std::collections::HashMap<Vec<u8>, u8>,
    /// 已进入过标题画面(P8.2b 门控:厂商 CG 阶段不建 VM UI 层)。
    pub title_seen: bool,
    /// 上一帧 VM UI 门控值(false→true 沿触发注册表重放)。
    pub vm_ui_allowed_prev: bool,
    #[allow(dead_code)]
    pub game_dir: std::path::PathBuf,
}

impl PlayerCore {
    /// 驱动 VM 一个 WAIT 周期(每帧一次;boot 长段多轮推进)。
    fn drive_vm(&mut self) {
        loop {
            match self.vm.run(4096) {
                Ok(VmSuspend::None) => {
                    if self.vm.executed_groups() % 250_000 == 0 {
                        break; // 单帧预算防爆
                    }
                }
                Ok(VmSuspend::Wait { counter, time_ms }) => {
                    self.wait_frames = counter.unwrap_or(0);
                    self.wait_until = time_ms
                        .map(|ms| Instant::now() + std::time::Duration::from_millis(ms));
                    break;
                }
                Ok(VmSuspend::Complete) => {
                    eprintln!("[player] VM complete @ {} 组", self.vm.executed_groups());
                    break;
                }
                Ok(VmSuspend::Error(m)) => {
                    eprintln!("[player] VM 挂起: {m} @ pc {}", self.vm.pc());
                    break;
                }
                Err(e) => {
                    eprintln!("[player] VM 错误: {e}");
                    break;
                }
            }
        }
    }

    /// 消费 VM 事件流 → 场景;装载新 CG 图像。
    /// P8.2b(成果 82):VM CG 通道接入场景 —— 游戏内 UI(es.BT.* 部件)
    /// 经此上屏;debug 覆盖层与标题等待期过滤(详见 docs/in-game-ui-plan.md)。
    fn consume_events(&mut self, allow_vm_ui: bool) {
        let events = self.vm.events();
        let fresh = &events[self.events_cursor..];
        self.events_cursor = events.len();
        if fresh.is_empty() {
            return;
        }
        for ev in fresh {
            if let yuris_vm::VmEvent::Text { file, .. } = ev {
                let _ = file; // TEXT 事件(P8.3 文本)记录;场景文本走 scenario 层
            }
        }
        let mut to_load: Vec<(u64, Vec<u8>)> = Vec::new();
        for ev in fresh {
            if let yuris_vm::VmEvent::Cg { pc, script_id, id, position, file, .. } = ev {
                let Some(name) = id else { continue };
                let Some(f) = file else { continue };
                if f.is_empty() {
                    continue; // 注册件(成果 62 BT.OVER 族 FILE="")无纹理,不建层
                }
                let path = String::from_utf8_lossy(f).into_owned();
                let lower = path.to_ascii_lowercase();
                if lower.contains("debug") {
                    continue; // 引擎 debug 覆盖层:纹理也不载
                }
                // 预载与建层解耦(P8.2b 勘误 5):boot 期(门控关)也要预载,
                // 否则进游戏时注册表重放找不到纹理 → 永远 0 层。
                let rid = fnv1a(name.as_bytes());
                if !self.loaded.contains(&rid) {
                    match self.index.read_image_bytes(&path) {
                        Some(data) => to_load.push((rid, data)),
                        None => eprintln!(
                            "[player] CG 图像解析失败:{path} (s{script_id} pc={pc})"
                        ),
                    }
                }
                if !vm_ui_layer_allowed(&lower, allow_vm_ui) {
                    continue;
                }
                // 建层(勘误 8):层按 es.BT 基名收敛为一层(id=fnv(base)),
                // 同钮各态(OFF/OVER/ONOV/NA)只显示优先级最低者。
                let (base, prio) = vm_ui_base_prio(name.as_bytes());
                if let Some(&old) = self.vm_ui_prio.get(&base) {
                    if prio > old {
                        continue; // 更差态(如 OVER 3 > OFF 0):不覆盖
                    }
                }
                // position 未指定的 CG.SET(只换纹理/patch 族)= 保持现有
                // 坐标(引擎「未指定槽保持原值」语义,成果 62)。
                let lid = fnv1a(&base);
                let (x, y) = match position {
                    Some((x, y, _z)) => (*x as f32, *y as f32),
                    None => {
                        let scene = self.bridge.scene();
                        match scene.layers.iter().find(|l| l.id == lid) {
                            Some(l) => (l.x, l.y),
                            None => (0.0, 0.0),
                        }
                    }
                };
                // (字段级操作 —— fresh 借 self.vm,不能用整 &mut self 方法)
                if !self.vm_ui_layers.contains(&lid) {
                    self.vm_ui_layers.push(lid);
                }
                let layer = Layer {
                    id: lid,
                    z: vm_ui_z_for_path(&lower),
                    visible: true,
                    x,
                    y,
                    scale_x: 1.0,
                    scale_y: 1.0,
                    alpha: 1.0,
                    rotation: 0.0,
                    resource: Some(ResourceId(rid)),
                };
                self.bridge.scene_mut().upsert_layer(layer);
                self.vm_ui_prio.insert(base, prio);
            }
            if let yuris_vm::VmEvent::CgAct { id, .. } = ev {
                // es.BT.XY.SET 宏体 = CGACT 槽 0x0c/0x0d → 注册表 x/y 已
                // 在 VM 侧落库(成果 82 勘误 4);此处把权威位置同步到
                // 已存在的 UI 层(XY.SET 晚于 CG.SET 的时序修复)。
                let Some(name) = id else { continue };
                let (base, _) = vm_ui_base_prio(name.as_bytes());
                let lid = fnv1a(&base);
                if !self.vm_ui_layers.contains(&lid) {
                    continue;
                }
                if let Some((x, y)) = self.vm.cg_position(name) {
                    let scene = self.bridge.scene_mut();
                    if let Some(l) = scene_mut_layer(scene, lid) {
                        l.x = x as f32;
                        l.y = y as f32;
                    }
                }
            }
            if let yuris_vm::VmEvent::CgEnd { id, .. } = ev {
                if let Some(name) = id {
                    let (base, _) = vm_ui_base_prio(name.as_bytes());
                    let lid = fnv1a(&base);
                    self.bridge.scene_mut().hide_layer(lid);
                    self.vm_ui_prio.remove(&base);
                    self.vm_ui_layers.retain(|&i| i != lid);
                }
            }
        }
        if !to_load.is_empty() {
            let backend = self.backend.as_mut().expect("backend");
            for (rid, data) in to_load {
                match backend.load_image(ResourceId(rid), &data) {
                    Ok(()) => {
                        self.loaded.insert(rid);
                    }
                    Err(e) => eprintln!("[player] 纹理上传失败 rid={rid:#x}: {e}"),
                }
            }
        }
    }

    /// 按 es.BT 基名 upsert VM UI 层(层 id = fnv(base);同钮各态收敛)。
    fn upsert_vm_ui_layer(&mut self, base: &[u8], rid: u64, x: f32, y: f32, path_lower: &str) {
        let lid = fnv1a(base);
        if !self.vm_ui_layers.contains(&lid) {
            self.vm_ui_layers.push(lid);
        }
        let layer = Layer {
            id: lid,
            z: vm_ui_z_for_path(path_lower),
            visible: true,
            x,
            y,
            scale_x: 1.0,
            scale_y: 1.0,
            alpha: 1.0,
            rotation: 0.0,
            resource: Some(ResourceId(rid)),
        };
        self.bridge.scene_mut().upsert_layer(layer);
    }

    /// 注册表重放(P8.2b):进入游戏时按 VM cg_registry 重建 VM UI 层。
    /// 同钮各态(OFF/OVER/ONOV/NA)取最优者一层;诊断清单逐条打印
    /// (谁上屏/坐标/谁被跳,便于对拍)。
    fn rebuild_vm_ui(&mut self) {
        // base → (prio, rid, x, y, path_lower)
        let mut best: HashMap<Vec<u8>, (u8, u64, f32, f32, String)> = HashMap::new();
        let mut skipped = 0usize;
        for (name, x, y, file) in self.vm.cg_registry_snapshot() {
            let Some(f) = file else {
                skipped += 1;
                continue; // 注册件无 FILE(纹理经他路):不进层
            };
            let path = String::from_utf8_lossy(&f).into_owned();
            let lower = path.to_ascii_lowercase();
            if !vm_ui_layer_allowed(&lower, true) {
                continue;
            }
            let rid = fnv1a(&name);
            if !self.loaded.contains(&rid) {
                skipped += 1;
                continue; // 无纹理(预载期解析失败)不建层
            }
            let (base, prio) = vm_ui_base_prio(&name);
            let better = match best.get(&base) {
                Some((old_prio, _, _, _, _)) => prio <= *old_prio,
                None => true,
            };
            if better {
                best.insert(base, (prio, rid, x as f32, y as f32, lower));
            }
        }
        let mut n = 0usize;
        let mut inv: Vec<String> = Vec::new();
        for (base, (prio, rid, x, y, lower)) in best {
            self.upsert_vm_ui_layer(&base, rid, x, y, &lower);
            self.vm_ui_prio.insert(base.clone(), prio);
            let nm = String::from_utf8_lossy(&base);
            let dir = lower
                .trim_start_matches(r"cgsys\")
                .split('\\')
                .next()
                .unwrap_or("?");
            if inv.len() < 60 {
                inv.push(format!("  [{dir}] {nm} @({x:.0},{y:.0})"));
            }
            n += 1;
        }
        if n > 0 {
            // VM 消息窗部件已上屏 → 撤掉 scenario 近似底框(双重叠层修复)
            self.bridge.scene_mut().hide_layer(SC_WIN);
        }
        eprintln!(
            "[player] VM UI 重放 {n} 层(跳 {skipped};注册 {} 条)",
            self.vm.cg_registry_len()
        );
        for l in &inv {
            eprintln!("[vm-ui]{l}");
        }
    }

    /// 读 scenario 资源并上传;返回 ResourceId。
    fn load_scenario_image(&mut self, path_or_name: &str) -> Option<ResourceId> {
        let rid = ResourceId(fnv1a(path_or_name.as_bytes()));
        if self.loaded.contains(&rid.0) {
            return Some(rid);
        }
        let data = self.index.read_cg_bytes(path_or_name)?;
        let b = self.backend.as_mut()?;
        b.load_image(rid, &data).ok()?;
        self.loaded.insert(rid.0);
        Some(rid)
    }

    /// 标题按钮三态切换(每帧;悬停 `_on`、按下帧 `_over`,其余 `_off`;成果 78)。
    /// 悬停**进入**命中区的边沿播悬停音(P2-2,成果 80)。子画面打开时让位。
    fn update_title_buttons(&mut self) {
        if self.subui != SubUi::None || self.title_buttons.is_empty() {
            return;
        }
        let (updates, hover_entered) = tri_state_pass(
            &mut self.title_buttons,
            self.cursor_logical,
            self.frame_clicked,
        );
        if hover_entered {
            self.play_sysse(TITLE_SE_HOVER);
        }
        if updates.is_empty() {
            return;
        }
        let scene = self.bridge.scene_mut();
        for (id, rid) in updates {
            if let Some(layer) = scene_mut_layer(scene, id) {
                layer.resource = Some(rid);
            }
        }
    }

    /// 子画面按钮三态切换(P3;机制与标题按钮同,悬停进入边沿播悬停音)。
    fn update_ui_buttons(&mut self) {
        if self.ui_buttons.is_empty() {
            return;
        }
        let (updates, hover_entered) =
            tri_state_pass(&mut self.ui_buttons, self.cursor_logical, self.frame_clicked);
        if hover_entered {
            self.play_sysse(TITLE_SE_HOVER);
        }
        if updates.is_empty() {
            return;
        }
        let scene = self.bridge.scene_mut();
        for (id, rid) in updates {
            if let Some(layer) = scene_mut_layer(scene, id) {
                layer.resource = Some(rid);
            }
        }
    }

    /// 渲染台词/选择肢到 RGBA 白字(fontdue;按内容定宽,自动换行)。
    /// 上限:200 字符 / 6 行(解码漂移产生的超长串会撑爆 canvas,实测踩坑)。
    fn render_text_layer(&mut self, text: &str) -> Option<ResourceId> {
        let text: String = text.chars().take(200).collect();
        let px = 40.0f32;
        let line_h = 52.0f32;
        let max_w = 1700.0f32;
        let max_rows = 6usize;
        // 打包行:每行 = (metrics, bitmap, xmin, ymin, advance)
        let mut rows: Vec<Vec<(usize, Vec<u8>, i32, i32, i32)>> = vec![Vec::new()];
        let mut row_w: Vec<i32> = vec![0];
        let mut cursor_x = 0i32;
        for ch in text.chars() {
            if ch == ' ' {
                cursor_x += 18;
                *row_w.last_mut().unwrap() = cursor_x;
                continue;
            }
            let (m, bmp) = self.font.rasterize(ch, px);
            if cursor_x + m.advance_width as i32 > max_w as i32 && rows.len() < max_rows {
                rows.push(Vec::new());
                row_w.push(0);
                cursor_x = 0;
            }
            if rows.len() > max_rows {
                break;
            }
            cursor_x += m.advance_width as i32;
            *row_w.last_mut().unwrap() = cursor_x;
            rows.last_mut().unwrap().push((
                m.width,
                bmp,
                m.xmin,
                m.ymin,
                m.advance_width as i32,
            ));
        }
        let width = (*row_w.iter().max().unwrap_or(&0)).clamp(60, max_w as i32) as usize;
        // fontdue Metrics.ymin 语义 = **从行顶(ascent 线)向下**的偏移
        // (实测 STHeiti 40px:'猫' ymin=-4 height=37,即字形顶略越行顶、
        // 底距行顶 33 < ascent 34.4 —— 若按基线解释会整字下坠 34px,
        // 底部全被 canvas 裁掉,即「文字只显示一半」)。因此字形位 =
        // 行顶 + ymin,canvas 高按行盒算,与基线无关。
        let (ascent, descent) = match self.font.horizontal_line_metrics(px) {
            Some(hm) => (hm.ascent, hm.descent.min(0.0)),
            None => (px * 0.9, -px * 0.15),
        };
        let pad_top = 4.0f32;
        let line_box = ((ascent - descent) as usize).max(1); // 行盒高(≈40)
        let height = (pad_top as usize * 2 + (rows.len().max(1) - 1) * line_h as usize + line_box)
            .max(20);
        let mut canvas = vec![0u8; width * height * 4];
        for (ri, row) in rows.iter().enumerate() {
            let mut pen_x = 0i32;
            let row_top = pad_top as i32 + (ri as i32) * line_h as i32;
            for (w, bmp, xmin, ymin, adv) in row {
                for i in 0..bmp.len() {
                    let a = bmp[i];
                    if a < 8 {
                        continue;
                    }
                    let bx = xmin + (i % *w) as i32;
                    let by = ymin + (i / *w) as i32;
                    let cx = (pen_x + bx) as usize;
                    let cy = (row_top + by) as i64;
                    if cx >= width || cy < 0 || cy as usize >= height {
                        continue;
                    }
                    let o = (cy as usize * width + cx) * 4;
                    canvas[o] = 255;
                    canvas[o + 1] = 255;
                    canvas[o + 2] = 255;
                    canvas[o + 3] = a;
                }
                pen_x += *adv;
            }
        }
        let rid = ResourceId(0x5C_1000_0000 | (fnv1a(text.as_bytes()) & 0xFFFF_FFFF));
        let b = self.backend.as_mut()?;
        b.load_image_rgba(rid, &canvas, width as u32, height as u32).ok()?;
        Some(rid)
    }

    /// 精灵淡入/淡出动画推进(\S/\T 的 fade_ms 与 \S.D 淡出)。
    fn update_sprite_fades(&mut self) {
        if self.sprite_fades.is_empty() {
            return;
        }
        let mut done: Vec<(u64, bool)> = Vec::new();
        for (id, start, ms, out) in &self.sprite_fades {
            let t = start.elapsed().as_millis() as f32 / (*ms).max(1) as f32;
            let a = if *out { 1.0 - t.min(1.0) } else { t.min(1.0) };
            let scene = self.bridge.scene_mut();
            if let Some(l) = scene.layers.iter_mut().find(|l| l.id == *id) {
                l.alpha = a;
                l.visible = a > 0.001;
            }
            if t >= 1.0 {
                done.push((*id, *out));
            }
        }
        for (id, out) in done {
            if out {
                self.bridge.scene_mut().hide_layer(id);
            }
            self.sprite_fades.retain(|(fid, _, _, _)| fid != &id);
        }
    }

    /// fade 覆盖层动画推进。
    fn update_fade(&mut self) {
        let Some(fs) = &self.fade else { return };
        let t = fs.start.elapsed().as_millis() as f32 / fs.ms.max(1) as f32;
        let a = if fs.out { t.min(1.0) } else { 1.0 - t.min(1.0) };
        let [r, g, bcol] = fs.color;
        let rid = ResourceId(
            0x5C_2000_0000 | ((r as u64) << 16) | ((g as u64) << 8) | bcol as u64,
        );
        if !self.loaded.contains(&rid.0) {
            let px = vec![r, g, bcol, 255];
            if let Some(b) = self.backend.as_mut() {
                let _ = b.load_image(rid, &px);
                self.loaded.insert(rid.0);
            }
        }
        let layer = Layer {
            id: SC_FADE,
            z: 100,
            visible: a > 0.001,
            x: 0.0,
            y: 0.0,
            scale_x: LOGICAL_W,
            scale_y: LOGICAL_H,
            alpha: a,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
        if t >= 1.0 {
            eprintln!("[scenario] fade 完成(out={})", fs.out);
            self.fade = None;
        }
    }

    /// 音频包内解析音频条目(case-insensitive;voice/bgm/se 命名族)。
    fn resolve_audio(&self, pack: &PacFileIndex, prefix: &str, name: &str) -> Option<Vec<u8>> {
        let want = format!("{}{}.ogg", prefix, name).to_ascii_lowercase();
        for k in pack.keys() {
            let key = String::from_utf8_lossy(k).to_ascii_lowercase();
            if key.ends_with(&want) || key == want {
                let stripped = String::from_utf8_lossy(&k[1..]).into_owned();
                if let Some(d) = pack.read_stored_bytes(&stripped) {
                    return Some(d);
                }
            }
        }
        None
    }

    /// 播系统 UI 音(sysse 包;标题按钮悬停/决定音,P2-2 成果 80)。
    /// 未命中(包缺/条目缺)记录决策不出声,不阻断流程。
    fn play_sysse(&mut self, name: &str) {
        match self.resolve_audio(&self.audio_packs, "", name) {
            Some(data) => self.audio.play_se(name, data),
            None => {
                self.audio.play_se(name, Vec::new());
                eprintln!("[audio] sysse 未命中: {name}");
            }
        }
    }

    // ================= P3 子画面(LOAD / EXTRA / END 确认;成果 81) =================

    /// 关闭子画面:隐藏全部 UI_* 层、清按钮与查看态,回到标题菜单。
    fn close_subui(&mut self) {
        let scene = self.bridge.scene_mut();
        for base in [
            UI_LOAD_BASE,
            UI_EXTRA_BASE,
            UI_CG_BASE,
            UI_BGM_BASE,
            UI_CONFIRM_BASE,
        ] {
            for i in 0..0x100u64 {
                scene.hide_layer(base + i);
            }
        }
        self.ui_buttons.clear();
        self.cg_view = None;
        self.subui = SubUi::None;
    }

    /// 子画面三态图按钮(states = 素材后缀组;命中区 rect 用素材实际尺寸,
    /// 素材未命中时保留 rect 兜底命中区 —— 无图可点,不静默失效)。
    fn ui_button(&mut self, id: u64, rect: [f32; 4], base_path: &str, states: [&str; 3], z: i32) {
        let mut rids: [Option<ResourceId>; 3] = [None, None, None];
        for (i, sfx) in states.iter().enumerate() {
            rids[i] = self.load_scenario_image(&format!("{base_path}{sfx}"));
        }
        self.ui_button_rids(id, rect, rids, z, true);
    }

    /// 子画面按钮(已备三态 rids;命中区 rect 用素材实际尺寸)。
    fn ui_button_rids(
        &mut self,
        id: u64,
        rect: [f32; 4],
        rids: [Option<ResourceId>; 3],
        z: i32,
        active: bool,
    ) {
        let (iw, ih) = rids
            .iter()
            .flatten()
            .next()
            .copied()
            .and_then(|rid0| self.backend.as_ref().and_then(|b| b.image_size(rid0.0)))
            .unwrap_or((rect[2] as u32, rect[3] as u32));
        if rids[TITLE_BTN_OFF].is_none() {
            eprintln!("[scenario] 子画面按钮素材未命中: id={id:#x}(命中区保留)");
        }
        self.ui_buttons.push(TitleButton {
            rect: [rect[0], rect[1], iw as f32, ih as f32],
            id,
            rids,
            shown: TITLE_BTN_OFF,
            hovered: false,
            active,
        });
        let Some(rid0) = rids[TITLE_BTN_OFF] else {
            return;
        };
        let layer = Layer {
            id,
            z,
            visible: true,
            x: rect[0],
            y: rect[1],
            scale_x: 1.0,
            scale_y: 1.0,
            alpha: 1.0,
            rotation: 0.0,
            resource: Some(rid0),
        };
        self.bridge.scene_mut().upsert_layer(layer);
    }

    /// bt3 系图集条裁剪(横排 N 段;btn_back_bt3 = 蓝/淡/橙 3 态,
    /// tab bt3n = 4 态;段宽 = 图宽/段数,rid = fnv(路径) 与段号混合)。
    fn load_atlas_segment(&mut self, path: &str, seg: usize, segs: usize) -> Option<ResourceId> {
        let data = self.index.read_cg_bytes(path)?;
        let img = image::load_from_memory(&data).ok()?;
        let seg_w = img.width() / segs as u32;
        let x0 = (seg as u32 * seg_w).min(img.width());
        let crop = img.crop_imm(x0, 0, seg_w, img.height());
        let rgba = crop.to_rgba8();
        let rid = ResourceId(
            0x5C_A000_0000 ^ (((fnv1a(path.as_bytes()) & 0xFFFF_FFFF) << 2) | seg as u64),
        );
        let b = self.backend.as_mut()?;
        b.load_image_rgba(rid, rgba.as_raw(), rgba.width(), rgba.height()).ok()?;
        self.loaded.insert(rid.0);
        Some(rid)
    }

    /// EXTRA 标签行(原生 yst00257:7 tab @ y=10;本实现布 4 钮 ——
    /// CG/BGM 可切,RP(SCENE)/MV 以 `_na` 暗段展示为禁用(功能未实装),
    /// st/wp/sv 素材包内缺失不布)。等距 247px 防重叠(原生 95px 步距的
    /// 图集裁剪方式未取证,布点为 Likely)。活动标签 = `_on` 单图,
    /// 非活动 = bt3n 图集第 0 段,悬停切 `_on`。
    fn open_tab_bar(&mut self, base: u64, active_cg: bool) {
        let (x0, y, tw, th, pitch) = (531.0, 10.0, 237.0, 53.0, 247.0);
        // CG 标签
        let cg_on = self.load_scenario_image("cgsys/extra/btn_tab_cgmode_on");
        let cg_off = if active_cg {
            cg_on
        } else {
            self.load_atlas_segment("cgsys/extra/btn_tab_cgmode_bt3n", 0, 4)
                .or(cg_on)
        };
        self.ui_button_rids(base + 0xA0, [x0, y, tw, th], [cg_off, cg_on, None], 53, true);
        // RP(SCENE)/MV:禁用展示(_na 暗段;不参与三态/点击)
        let rp = self
            .load_atlas_segment("cgsys/extra/btn_tab_rpmode_bt3n", 3, 4)
            .or_else(|| self.load_atlas_segment("cgsys/extra/btn_tab_rpmode_bt3n", 0, 4));
        self.ui_button_rids(base + 0xA2, [x0 + pitch, y, tw, th], [rp, rp, None], 53, false);
        let mv = self
            .load_atlas_segment("cgsys/extra/btn_tab_mvmode_bt3n", 3, 4)
            .or_else(|| self.load_atlas_segment("cgsys/extra/btn_tab_mvmode_bt3n", 0, 4));
        self.ui_button_rids(base + 0xA3, [x0 + 2.0 * pitch, y, tw, th], [mv, mv, None], 53, false);
        // BGM 标签
        let bgm_on = self.load_scenario_image("cgsys/extra/btn_tab_bgmmode_on");
        let bgm_off = if active_cg {
            self.load_atlas_segment("cgsys/extra/btn_tab_bgmmode_bt3n", 0, 4)
                .or(bgm_on)
        } else {
            bgm_on
        };
        self.ui_button_rids(base + 0xA1, [x0 + 3.0 * pitch, y, tw, th], [bgm_off, bgm_on, None], 53, true);
    }

    /// bt3 三态返回钮(蓝=常态/淡=悬停/橙=按下;图集裁剪)。
    fn open_back_button(&mut self, id: u64, rect: [f32; 4], z: i32) {
        let s0 = self.load_atlas_segment("cgsys/extra/btn_back_bt3", 0, 3);
        let s1 = self.load_atlas_segment("cgsys/extra/btn_back_bt3", 1, 3);
        let s2 = self.load_atlas_segment("cgsys/extra/btn_back_bt3", 2, 3);
        self.ui_button_rids(id, rect, [s0, s1, s2], z, true);
    }

    /// 子画面白字文本(无 backend = 无层,不 panic)。
    fn ui_text(&mut self, id: u64, text: &str, x: f32, y: f32, z: i32) {
        let Some(rid) = self.render_text_layer(text) else {
            return;
        };
        let layer = Layer {
            id,
            z,
            visible: true,
            x,
            y,
            scale_x: 1.0,
            scale_y: 1.0,
            alpha: 1.0,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
    }

    /// 子画面纯色层(压暗/兜底底图;1×1 RGBA 拉伸)。
    fn ui_fill(&mut self, id: u64, color: [u8; 4], z: i32) {
        let rid = ResourceId(
            0x5C_F000_0000
                | ((color[0] as u64) << 16)
                | ((color[1] as u64) << 8)
                | color[2] as u64,
        );
        if !self.loaded.contains(&rid.0) {
            if let Some(b) = self.backend.as_mut() {
                let _ = b.load_image_rgba(rid, &color, 1, 1);
                self.loaded.insert(rid.0);
            }
        }
        let layer = Layer {
            id,
            z,
            visible: true,
            x: 0.0,
            y: 0.0,
            scale_x: LOGICAL_W,
            scale_y: LOGICAL_H,
            alpha: 1.0,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
    }

    /// ev 全图解码 → 缩略图 RGBA 直传(cell 尺寸 cover 裁剪;解码失败跳过)。
    fn load_thumb(&mut self, id: u64, path: &str, x: f32, y: f32, w: f32, h: f32) {
        let Some(data) = self.index.read_cg_bytes(path) else {
            return;
        };
        let Ok(img) = image::load_from_memory(&data) else {
            return;
        };
        let thumb = img.resize_to_fill(w as u32, h as u32, image::imageops::FilterType::Triangle);
        let rgba = thumb.to_rgba8();
        let rid = ResourceId(0x5C_A000_0000 | (fnv1a(path.as_bytes()) & 0xFFFF_FFFF));
        let uploaded = match self.backend.as_mut() {
            Some(b) => b
                .load_image_rgba(rid, rgba.as_raw(), rgba.width(), rgba.height())
                .is_ok(),
            None => false,
        };
        if !uploaded {
            return;
        }
        self.loaded.insert(rid.0);
        let layer = Layer {
            id,
            z: 51,
            visible: true,
            x,
            y,
            scale_x: 1.0,
            scale_y: 1.0,
            alpha: 1.0,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
    }

    /// 扫描 save/yskernel_*.json 存档(mtime 新在前;显示 = mtime + file/label)。
    fn scan_save_entries(&mut self) {
        self.save_entries.clear();
        let dir = self.game_dir.join("save");
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut paths: Vec<std::path::PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .map_or(false, |s| s.starts_with("yskernel_") && s.ends_with(".json"))
            })
            .collect();
        paths.sort_by_key(|p| {
            std::cmp::Reverse(
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            )
        });
        for p in paths {
            let stamp = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| fmt_unix_time(d.as_secs()))
                .unwrap_or_else(|| "----/--/-- --:--".to_string());
            let label = std::fs::read(&p)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .map(|d| {
                    format!(
                        "{}  {} / {}",
                        stamp,
                        d["file"].as_str().unwrap_or("?"),
                        d["label"].as_str().unwrap_or("?")
                    )
                })
                .unwrap_or_else(|| "(损坏的存档)".to_string());
            self.save_entries.push((p, label));
        }
    }

    /// cg\ev\*.png 清单(剥根规范名;含 update 包同名,LIFO 读取时自动覆盖)。
    fn list_ev_paths(&self) -> Vec<String> {
        let mut set: Vec<String> = self
            .index
            .keys()
            .filter_map(|k| {
                let s = String::from_utf8_lossy(&k[1..]);
                (s.contains(r"cg\ev\") && s.ends_with(".png")).then(|| s.into_owned())
            })
            .collect();
        set.sort();
        set.dedup();
        set
    }

    /// BGM 曲目名(去 bgm\ 目录与 .ogg;case 保真排序)。
    fn list_bgm_tracks(&self) -> Vec<String> {
        let mut set: Vec<String> = self
            .audio_packs
            .keys()
            .filter_map(|k| {
                let s = String::from_utf8_lossy(&k[1..]);
                let lower = s.to_ascii_lowercase();
                (lower.contains(r"bgm\") && lower.ends_with(".ogg")).then(|| {
                    s.rsplit('\\')
                        .next()
                        .unwrap_or("")
                        .trim_end_matches(".ogg")
                        .trim_end_matches(".OGG")
                        .to_string()
                })
            })
            .collect();
        set.sort_by_key(|t| t.to_ascii_lowercase());
        set.dedup_by_key(|t| t.to_ascii_lowercase());
        set
    }

    /// P3-1:LOAD 存档列表(back_load 整屏 + 存档行 + 戻る)。
    fn open_load(&mut self) {
        self.close_subui();
        self.subui = SubUi::Load;
        if let Some(rid) = self.load_scenario_image(r"cgsys/saveload/back_load") {
            let layer = Layer {
                id: UI_LOAD_BASE,
                z: 50,
                visible: true,
                x: 0.0,
                y: 0.0,
                scale_x: LOGICAL_W / 1920.0,
                scale_y: LOGICAL_H / 1080.0,
                alpha: 1.0,
                rotation: 0.0,
                resource: Some(rid),
            };
            self.bridge.scene_mut().upsert_layer(layer);
        } else {
            self.ui_fill(UI_LOAD_BASE, [8, 8, 14, 255], 50);
        }
        self.scan_save_entries();
        // 存档行(最多 8 行;半透明底板 + 白字 + 不可见命中钮)
        let row_labels: Vec<String> = self
            .save_entries
            .iter()
            .take(8)
            .map(|(_, l)| l.clone())
            .collect();
        for (i, label) in row_labels.iter().enumerate() {
            let y = 200.0 + i as f32 * 92.0;
            // 2×1 半透明深色板(拉伸为行底)
            let px = [16u8, 14, 24, 170, 16, 14, 24, 170];
            let plate = ResourceId(0x5C_7000_0100 + i as u64);
            if !self.loaded.contains(&plate.0) {
                if let Some(b) = self.backend.as_mut() {
                    let _ = b.load_image_rgba(plate, &px, 2, 1);
                    self.loaded.insert(plate.0);
                }
            }
            let plate_layer = Layer {
                id: UI_LOAD_BASE + 0x40 + i as u64,
                z: 51,
                visible: true,
                x: 320.0,
                y,
                scale_x: 1280.0,
                scale_y: 80.0,
                alpha: 1.0,
                rotation: 0.0,
                resource: Some(plate),
            };
            self.bridge.scene_mut().upsert_layer(plate_layer);
            self.ui_text(UI_LOAD_BASE + 0x10 + i as u64, label, 352.0, y + 18.0, 52);
            self.ui_buttons.push(TitleButton {
                rect: [320.0, y, 1280.0, 80.0],
                id: UI_LOAD_BASE + 0x80 + i as u64,
                rids: [None, None, None],
                shown: TITLE_BTN_OFF,
                hovered: false,
                active: true,
            });
        }
        if self.save_entries.is_empty() {
            self.ui_text(UI_LOAD_BASE + 0x10, "セーブデータがありません", 760.0, 480.0, 52);
        }
        self.ui_button(
            UI_LOAD_BASE + 1,
            [1660.0, 940.0, 186.0, 87.0],
            r"cgsys/saveload/btn_back",
            ["_off", "_on", "_over"],
            53,
        );
        eprintln!("[scenario] LOAD 画面(存档 {} 件)", self.save_entries.len());
    }

    /// P3-2:CG 鉴赏(原生 yst00257 = cgmode 屏,EXTRA 直达无落地菜单;
    /// cgmode/back 3×3 画格 + 顶部标签行 + bt3 返回钮)。
    fn open_extra_cg(&mut self, page: usize) {
        self.close_subui();
        self.subui = SubUi::ExtraCg;
        if self
            .load_scenario_image(r"cgsys/extra/cgmode/back")
            .is_none()
        {
            self.ui_fill(UI_CG_BASE, [234, 244, 248, 255], 50);
        }
        self.open_tab_bar(UI_CG_BASE, true);
        if self.ev_list.is_empty() {
            self.ev_list = self.list_ev_paths();
        }
        const PER: usize = 9; // 原生画格 3×3(cgmode/back 白格)
        let pages = self.ev_list.len().div_ceil(PER).max(1);
        let page = page.min(pages - 1);
        self.cg_page = page;
        let page_items: Vec<String> = self
            .ev_list
            .iter()
            .skip(page * PER)
            .take(PER)
            .cloned()
            .collect();
        // 画格锚点(cgmode/back 白格实测:列 277/736/1192 起、行 141/435/733 起,
        // 格 ~458×294;缩略图内缩 ~7px)
        for (i, path) in page_items.iter().enumerate() {
            let x = 284.0 + (i % 3) as f32 * 458.0;
            let y = 148.0 + (i / 3) as f32 * 296.0;
            self.load_thumb(UI_CG_BASE + 0x10 + i as u64, path, x, y, 445.0, 284.0);
            self.ui_buttons.push(TitleButton {
                rect: [x, y, 445.0, 284.0],
                id: UI_CG_BASE + 0x80 + i as u64,
                rids: [None, None, None],
                shown: TITLE_BTN_OFF,
                hovered: false,
                active: true,
            });
        }
        self.ui_text(
            UI_CG_BASE + 0x50,
            &format!("{} / {}  ({} CG)", page + 1, pages, self.ev_list.len()),
            640.0,
            984.0,
            52,
        );
        if page > 0 {
            self.ui_buttons.push(TitleButton {
                rect: [250.0, 950.0, 220.0, 100.0],
                id: UI_CG_PREV,
                rids: [None, None, None],
                shown: TITLE_BTN_OFF,
                hovered: false,
                active: true,
            });
            self.ui_text(UI_CG_BASE + 0x51, "前", 330.0, 985.0, 52);
        }
        if page + 1 < pages {
            self.ui_buttons.push(TitleButton {
                rect: [1350.0, 950.0, 220.0, 100.0],
                id: UI_CG_NEXT,
                rids: [None, None, None],
                shown: TITLE_BTN_OFF,
                hovered: false,
                active: true,
            });
            self.ui_text(UI_CG_BASE + 0x52, "次", 1430.0, 985.0, 52);
        }
        self.open_back_button(UI_CG_BACK, [1620.0, 930.0, 237.0, 80.0], 53);
        eprintln!(
            "[scenario] CG 鉴赏 {}/{}({} 件)",
            page + 1,
            pages,
            self.ev_list.len()
        );
    }

    /// P3-2:BGM 鉴赏入口(保持上次页码)。
    fn open_extra_bgm(&mut self) {
        self.open_extra_bgm_page(self.bgm_page);
    }

    /// P3-2:BGM 鉴赏指定页(原生 extra/back = EXTRAS 双列列表底;曲目点播,
    /// 戻る停止播放)。22 曲/页(2 列 × 11 行,对齐底图画线)。
    fn open_extra_bgm_page(&mut self, page: usize) {
        self.close_subui();
        self.subui = SubUi::ExtraBgm;
        if self.load_scenario_image(r"cgsys/extra/back").is_none() {
            self.ui_fill(UI_BGM_BASE, [234, 244, 248, 255], 50);
        }
        self.open_tab_bar(UI_BGM_BASE, false);
        if self.bgm_tracks.is_empty() {
            self.bgm_tracks = self.list_bgm_tracks();
        }
        const PER: usize = 22;
        let pages = self.bgm_tracks.len().div_ceil(PER).max(1);
        let page = page.min(pages - 1);
        self.bgm_page = page;
        let tracks: Vec<String> = self
            .bgm_tracks
            .iter()
            .skip(page * PER)
            .take(PER)
            .cloned()
            .collect();
        for (i, t) in tracks.iter().enumerate() {
            let col = i / 11;
            let row = i % 11;
            let x = 368.0 + col as f32 * 600.0;
            let y = 250.0 + row as f32 * 56.0;
            self.ui_text(UI_BGM_BASE + 0x10 + i as u64, t, x, y, 52);
            self.ui_buttons.push(TitleButton {
                rect: [x - 16.0, y - 8.0, 560.0, 50.0],
                id: UI_BGM_BASE + 0x80 + i as u64,
                rids: [None, None, None],
                shown: TITLE_BTN_OFF,
                hovered: false,
                active: true,
            });
        }
        if self.bgm_tracks.is_empty() {
            self.ui_text(UI_BGM_BASE + 0x10, "BGM なし", 880.0, 480.0, 52);
        }
        self.ui_text(
            UI_BGM_BASE + 0x50,
            &format!("{} / {}", page + 1, pages),
            900.0,
            984.0,
            52,
        );
        if page > 0 {
            self.ui_buttons.push(TitleButton {
                rect: [250.0, 950.0, 220.0, 100.0],
                id: UI_BGM_PREV,
                rids: [None, None, None],
                shown: TITLE_BTN_OFF,
                hovered: false,
                active: true,
            });
            self.ui_text(UI_BGM_BASE + 0x51, "前", 330.0, 985.0, 52);
        }
        if page + 1 < pages {
            self.ui_buttons.push(TitleButton {
                rect: [1350.0, 950.0, 220.0, 100.0],
                id: UI_BGM_NEXT,
                rids: [None, None, None],
                shown: TITLE_BTN_OFF,
                hovered: false,
                active: true,
            });
            self.ui_text(UI_BGM_BASE + 0x52, "次", 1430.0, 985.0, 52);
        }
        self.open_back_button(UI_BGM_BACK, [1620.0, 930.0, 237.0, 80.0], 53);
        eprintln!(
            "[scenario] BGM 鉴赏 {}/{}({} 曲)",
            page + 1,
            pages,
            self.bgm_tracks.len()
        );
    }

    /// P3-3:END 确认对话框(dialog_end + yes/no 三态钮)。
    fn open_confirm_end(&mut self) {
        self.close_subui();
        self.subui = SubUi::ConfirmEnd;
        self.ui_fill(UI_CONFIRM_BASE, [0, 0, 0, 140], 50);
        if let Some(rid) = self.load_scenario_image(r"cgsys/confirm/dialog_end") {
            let layer = Layer {
                id: UI_CONFIRM_BASE + 1,
                z: 51,
                visible: true,
                x: 694.0,
                y: 380.0,
                scale_x: 1.0,
                scale_y: 1.0,
                alpha: 1.0,
                rotation: 0.0,
                resource: Some(rid),
            };
            self.bridge.scene_mut().upsert_layer(layer);
        }
        self.ui_button(
            UI_CONFIRM_BASE + 2,
            [740.0, 470.0, 146.0, 45.0],
            r"cgsys/confirm/btn_yes",
            ["_off", "_on", "_over"],
            52,
        );
        self.ui_button(
            UI_CONFIRM_BASE + 3,
            [1040.0, 470.0, 146.0, 45.0],
            r"cgsys/confirm/btn_no",
            ["_off", "_on", "_over"],
            52,
        );
        eprintln!("[scenario] END 确认对话框");
    }

    /// 子画面点击路由(每帧至多一次;返回恒 None —— scenario 保持标题等待)。
    fn poll_subui(&mut self) {
        if !self.frame_clicked {
            return;
        }
        let (cx, cy) = self.cursor_logical;
        let hit = self.ui_buttons.iter().find(|b| {
            b.active
                && cx >= b.rect[0] as i64
                && cx < (b.rect[0] + b.rect[2]) as i64
                && cy >= b.rect[1] as i64
                && cy < (b.rect[1] + b.rect[3]) as i64
        });
        let Some(id) = hit.map(|b| b.id) else {
            return;
        };
        match self.subui {
            SubUi::Load => {
                if id == UI_LOAD_BASE + 1 {
                    self.play_sysse(TITLE_SE_CANCEL);
                    self.close_subui();
                } else if id >= UI_LOAD_BASE + 0x80 {
                    let i = (id - UI_LOAD_BASE - 0x80) as usize;
                    if let Some((path, _)) = self.save_entries.get(i) {
                        let path = path.clone();
                        self.play_sysse(TITLE_SE_DECIDE);
                        self.request_load_path = Some(path);
                        self.request_title_load = true;
                    }
                }
            }
            SubUi::ExtraCg => {
                // 全图查看:任意点击返回分页
                if self.cg_view.take().is_some() {
                    let scene = self.bridge.scene_mut();
                    scene.hide_layer(UI_CG_BASE + 0x60);
                    scene.hide_layer(UI_CG_BASE + 0x61);
                    return;
                }
                match id {
                    UI_CG_TAB_BGM => {
                        // 标签:BGM 鉴赏
                        self.play_sysse(TITLE_SE_DECIDE);
                        self.open_extra_bgm();
                    }
                    UI_CG_PREV => {
                        self.play_sysse(TITLE_SE_DECIDE);
                        self.open_extra_cg(self.cg_page.saturating_sub(1));
                    }
                    UI_CG_NEXT => {
                        self.play_sysse(TITLE_SE_DECIDE);
                        self.open_extra_cg(self.cg_page + 1);
                    }
                    UI_CG_BACK => {
                        self.play_sysse(TITLE_SE_CANCEL);
                        self.close_subui(); // 戻る → 标题(原生各屏独立返回)
                    }
                    id if (UI_CG_BASE + 0x80..UI_CG_BASE + 0x90).contains(&id) => {
                        let i = (id - UI_CG_BASE - 0x80) as usize;
                        const PER: usize = 9; // 原生画格 3×3
                        if let Some(path) = self.ev_list.get(self.cg_page * PER + i) {
                            let path = path.clone();
                            self.play_sysse(TITLE_SE_DECIDE);
                            self.show_cg_full(&path);
                        }
                    }
                    _ => {}
                }
            }
            SubUi::ExtraBgm => {
                match id {
                    UI_BGM_TAB_CG => {
                        // 标签:CG 鉴赏
                        self.play_sysse(TITLE_SE_DECIDE);
                        self.open_extra_cg(self.cg_page);
                    }
                    UI_BGM_BACK => {
                        self.audio.stop_bgm();
                        self.play_sysse(TITLE_SE_CANCEL);
                        self.close_subui(); // 戻る → 标题(停止 BGM)
                    }
                    UI_BGM_PREV => {
                        self.play_sysse(TITLE_SE_DECIDE);
                        self.open_extra_bgm_page(self.bgm_page.saturating_sub(1));
                    }
                    UI_BGM_NEXT => {
                        self.play_sysse(TITLE_SE_DECIDE);
                        self.open_extra_bgm_page(self.bgm_page + 1);
                    }
                    id if (UI_BGM_BASE + 0x80..UI_BGM_BASE + 0x90).contains(&id) => {
                        let i = (id - UI_BGM_BASE - 0x80) as usize;
                        const PER: usize = 22;
                        if let Some(t) = self.bgm_tracks.get(self.bgm_page * PER + i) {
                            let t = t.clone();
                            self.play_sysse(TITLE_SE_DECIDE);
                            match self.resolve_audio(&self.audio_packs, "", &t) {
                                Some(data) => self.audio.play_bgm(&t, data, None),
                                None => eprintln!("[audio] bgm 未命中: {t}"),
                            }
                        }
                    }
                    _ => {}
                }
            }
            SubUi::ConfirmEnd => {
                if id == UI_CONFIRM_BASE + 2 {
                    self.play_sysse(TITLE_SE_DECIDE);
                    self.request_quit = true; // はい:主循环 event_loop.exit()
                } else if id == UI_CONFIRM_BASE + 3 {
                    self.play_sysse(TITLE_SE_CANCEL);
                    self.close_subui();
                }
            }
            SubUi::None => {}
        }
    }

    /// CG 全图查看(ev 原图整屏;点击由 poll_subui 关闭)。
    fn show_cg_full(&mut self, path: &str) {
        let Some(rid) = self.load_scenario_image(path) else {
            eprintln!("[scenario] CG 全图未命中: {path}");
            return;
        };
        let (iw, ih) = self
            .backend
            .as_ref()
            .and_then(|b| b.image_size(rid.0))
            .unwrap_or((1920, 1080));
        let layer = Layer {
            id: UI_CG_BASE + 0x60,
            z: 60,
            visible: true,
            x: 0.0,
            y: 0.0,
            scale_x: LOGICAL_W / iw as f32,
            scale_y: LOGICAL_H / ih as f32,
            alpha: 1.0,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
        self.ui_text(UI_CG_BASE + 0x61, "クリックで戻る", 830.0, 1020.0, 61);
        self.cg_view = Some(rid);
    }


    /// 渲染台词窗底框(txspace 纹理 + 半透明底)到场景(z=80)。
    fn show_window_frame(&mut self) {
        let tex = self
            .load_scenario_image("cgsys/main/button/type1/tip_meswindow_txspace")
            .map(|r| (r, 1350u32, 200u32));
        let (rid, sw, sh) = match tex {
            Some((r, w, h)) => (r, w, h),
            None => (ResourceId(0x5C_0000_0004), 2u32, 2u32),
        };
        if tex.is_none() {
            // 回退:2×2 半透明黑
            if !self.loaded.contains(&rid.0) {
                if let Some(b) = self.backend.as_mut() {
                    let _ = b.load_image_rgba(rid, &[10, 10, 16, 160, 10, 10, 16, 160, 10, 10, 16, 160, 10, 10, 16, 160], 2, 2);
                    self.loaded.insert(rid.0);
                }
            }
        }
        let layer = Layer {
            id: SC_WIN,
            z: 80,
            visible: true,
            x: 0.0,
            y: LOGICAL_H - 250.0,
            scale_x: LOGICAL_W / sw as f32,
            scale_y: 250.0 / sh as f32,
            alpha: 0.9,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
    }

}

impl ScenarioHost for PlayerCore {
    fn reset_scene(&mut self) {
        // 与标题层清理统一(含 0x5C_7000_* 标题层 / 按钮 / 精灵 / 文本窗)
        self.reset_title_layers();
    }

    fn show_bg(&mut self, name: &str, _fade_ms: u64, color: [u8; 3]) {
        let trimmed = name.trim();
        let rid = self.load_scenario_image(trimmed);
        eprintln!(
            "[scenario] BG {trimmed:?} 解析={} color={color:?}",
            if rid.is_some() { "命中" } else { "纯色" }
        );
        let Some(rid) = rid else {
            // 纯色(white/black 等):上传 1×1 色图
            let rid = ResourceId(SC_BG);
            let px = vec![color[0], color[1], color[2], 255];
            if let Some(b) = self.backend.as_mut() {
                let _ = b.load_image(rid, &px);
            }
            self.loaded.insert(rid.0);
            let layer = Layer {
                id: SC_BG,
                z: 0,
                visible: true,
                x: 0.0,
                y: 0.0,
                scale_x: LOGICAL_W,
                scale_y: LOGICAL_H,
                alpha: 1.0,
                rotation: 0.0,
                resource: Some(rid),
            };
            self.bridge.scene_mut().upsert_layer(layer);
            return;
        };
        let (iw, ih) = self
            .backend
            .as_ref()
            .and_then(|b| b.image_size(rid.0))
            .unwrap_or((1920, 1080));
        let layer = Layer {
            id: SC_BG,
            z: 0,
            visible: true,
            x: 0.0,
            y: 0.0,
            scale_x: LOGICAL_W / iw as f32,
            scale_y: LOGICAL_H / ih as f32,
            alpha: 1.0,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
    }

    fn show_sprite(&mut self, name: &str, path: Option<&str>, x: i64, y: i64, fade_ms: u64) {
        // \S:(x,y) = 左上角(0,0 = 全屏图原位),原生尺寸(成果 69 勘误)
        let key = path.unwrap_or(name);
        let Some(rid) = self.load_scenario_image(key) else {
            eprintln!("[scenario] 资源未命中: {key}");
            return;
        };
        let id = fnv1a(name.as_bytes());
        if !self.sprites.contains(&id) {
            self.sprites.push(id);
        }
        let alpha = if fade_ms > 0 { 0.0 } else { 1.0 };
        let layer = Layer {
            id,
            z: 10,
            visible: true,
            x: x as f32,
            y: y as f32,
            scale_x: 1.0,
            scale_y: 1.0,
            alpha,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
        if fade_ms > 0 {
            self.sprite_fades.push((id, Instant::now(), fade_ms, false));
        }
    }

    fn show_tachie(&mut self, name: &str, x: i64, y: i64, fade_ms: u64) {
        // \T:立绘(stand 分层整图近似);x = 中心偏移,y = 底部偏移(Likely)
        let Some(rid) = self.load_scenario_image(name) else {
            eprintln!("[scenario] 立绘未命中: {name}");
            return;
        };
        let (iw, ih) = self
            .backend
            .as_ref()
            .and_then(|b| b.image_size(rid.0))
            .unwrap_or((880, 1200));
        let id = fnv1a(name.as_bytes());
        if !self.sprites.contains(&id) {
            self.sprites.push(id);
        }
        let px = LOGICAL_W / 2.0 + x as f32 - iw as f32 / 2.0;
        let py = LOGICAL_H - ih as f32 + y as f32;
        eprintln!("[scenario] 立绘 {name}: tex={iw}x{ih} → ({px},{py}) fade={fade_ms}");
        let alpha = if fade_ms > 0 { 0.0 } else { 1.0 };
        let layer = Layer {
            id,
            z: 10,
            visible: true,
            x: px,
            y: py,
            scale_x: 1.0,
            scale_y: 1.0,
            alpha,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
        if fade_ms > 0 {
            self.sprite_fades.push((id, Instant::now(), fade_ms, false));
        }
    }

    fn hide_sprite(&mut self, name: &str, fade_ms: u64) {
        let id = fnv1a(name.as_bytes());
        if fade_ms > 0 {
            // 淡出:动画完成由 tick 收尾隐藏。先撤销同 id 未完成的淡入条目
            // (否则 in/out 双条目同帧互写 alpha,闪烁且终态不确定)。
            self.sprite_fades.retain(|(fid, _, _, _)| fid != &id);
            self.sprite_fades.push((id, Instant::now(), fade_ms, true));
            return;
        }
        self.bridge.scene_mut().hide_layer(id);
    }

    fn fade(&mut self, out: bool, ms: u64, color: [u8; 3]) {
        self.fade = Some(FadeState {
            out,
            start: Instant::now(),
            ms,
            color,
        });
    }

    fn show_text(&mut self, _line_id: Option<u32>, lt: &str, _le: &str) {
        // VM 消息窗部件已上屏时不再画近似底框(双重叠层修复,P8.2b 第 3 步;
        // 原生消息窗 = VM 部件自绘,scenario 只出文字)
        if self.vm_ui_layers.is_empty() {
            self.show_window_frame();
        }
        // 繁→简(显示层转换,不改剧本数据;fast2s 单字映射 + 词汇表)
        let lt: String = fast2s::convert(lt);
        let Some(rid) = self.render_text_layer(&lt) else {
            eprintln!("[scenario] 台词纹理生成失败");
            return;
        };
        eprintln!("[scenario] 台词层就绪 {lt:?}");
        let (tw, th) = self
            .backend
            .as_ref()
            .and_then(|b| b.image_size(rid.0))
            .unwrap_or((1700, 200));
        let layer = Layer {
            id: SC_TEXT,
            z: 90,
            visible: true,
            x: (LOGICAL_W - tw as f32) / 2.0,
            y: LOGICAL_H - 250.0 + (250.0 - th as f32) / 2.0,
            scale_x: 1.0,
            scale_y: 1.0,
            alpha: 1.0,
            rotation: 0.0,
            resource: Some(rid),
        };
        self.bridge.scene_mut().upsert_layer(layer);
    }

    fn clear_text(&mut self) {
        self.bridge.scene_mut().hide_layer(SC_TEXT);
    }

    fn play_voice(&mut self, name: &str) {
        if let Some(data) = self.resolve_audio(&self.audio_packs, "voice\\", name) {
            self.audio.play_voice(name, data);
        } else {
            eprintln!("[audio] voice 未命中: {name}");
        }
    }

    fn play_bgm(&mut self, name: &str, volume_permille: Option<i64>) {
        if name.trim().is_empty() {
            // \BGM(,800):仅调音量
            self.audio.play_bgm("__cur__", Vec::new(), volume_permille);
            return;
        }
        if let Some(data) = self.resolve_audio(&self.audio_packs, "bgm\\", name) {
            self.audio.play_bgm(name, data, volume_permille);
        } else {
            eprintln!("[audio] bgm 未命中: {name}");
        }
    }

    fn play_se(&mut self, name: &str) {
        if let Some(data) = self.resolve_audio(&self.audio_packs, "se\\", name) {
            self.audio.play_se(name, data);
        } else {
            eprintln!("[audio] se 未命中: {name}");
        }
    }

    fn title_screen(&mut self) {
        // 内置标题:eyecatch/st 分层素材组合(bg01a + sir/han + logo + 真实按钮列)
        // 布局对齐真机截图(用户 2026-09-05 提供):银发左 / 双马尾中 / logo 右上 / 按钮右下
        self.reset_title_layers();
        // 标题接管视觉:隐藏 VM UI 层(P8.2b;厂商期已门控不建,此处清
        // 游戏中返回标题的可能残留)+ 置 title_seen(此后放行 VM UI)
        let scene = self.bridge.scene_mut();
        for id in self.vm_ui_layers.drain(..) {
            scene.hide_layer(id);
        }
        self.vm_ui_prio.clear();
        self.title_seen = true;
        // 全屏底层(path, x, y, id)→ 拉伸全屏
        let layers: [(&str, i64, i64, u64); 5] = [
            ("eyecatch/st/bg01a", 0, 0, 0x5C_7000_0000),
            ("eyecatch/st/tet", 0, 0, 0x5C_7000_0001),
            ("eyecatch/st/sir", -1058, 0, 0x5C_7000_0002),
            ("eyecatch/st/han", -86, 0, 0x5C_7000_0003),
            ("eyecatch/st/logo", 1046, -255, 0x5C_7000_0004),
        ];
        // 按钮列:三态素材 `cgsys/title/btn_{名}_{off,on,over}`(成果 78 图像
        // 实证);默认显示 `_off` 常态,悬停/按下由 update_title_buttons 切换。
        // 坐标/绑定 = yst00259 es.BT.XY.SET/es.BT.SET 原生真值(成果 79):
        // arasuji→BTN.START(2)、start→BTN.START(1)、extra(1314)/config(1477,
        // 未实装)/end(1640) 同排 y=745。
        let buttons: [(&str, i64, i64, u64); 6] = [
            ("cgsys/title/btn_arasuji", 1393, 384, 0x5C_7000_000A),
            ("cgsys/title/btn_start", 1393, 439, 0x5C_7000_0005),
            ("cgsys/title/btn_load", 1393, 533, 0x5C_7000_0006),
            ("cgsys/title/btn_lastload", 1393, 627, 0x5C_7000_0007),
            ("cgsys/title/btn_extra", 1314, 745, 0x5C_7000_0008),
            ("cgsys/title/btn_end", 1640, 745, 0x5C_7000_0009),
        ];
        // 无存档判定(P1-4):快存文件不存在 → CONTINUE 用 `_na` 灰化态且
        // 点击无效(原生 es.BT 按 save/*.sd 存在性条件注册 BTN.LLOAD vs
        // BTN.LLOAD.NA 两个同位按钮,成果 79;本实现以快存文件为判据)。
        let has_save = self
            .game_dir
            .join("save")
            .join("yskernel_qsave.json")
            .is_file();
        let white = ResourceId(0x5C_0000_0005);
        if !self.loaded.contains(&white.0) {
            if let Some(b) = self.backend.as_mut() {
                let _ = b.load_image(white, &[255, 255, 255, 255]);
                self.loaded.insert(white.0);
            }
        }
        let bgw = Layer {
            id: SC_BG,
            z: 0,
            visible: true,
            x: 0.0,
            y: 0.0,
            scale_x: LOGICAL_W,
            scale_y: LOGICAL_H,
            alpha: 1.0,
            rotation: 0.0,
            resource: Some(white),
        };
        {
            let scene = self.bridge.scene_mut();
            scene.upsert_layer(bgw);
        }
        for (path, x, y, id) in layers {
            let Some(rid) = self.load_scenario_image(path) else {
                eprintln!("[scenario] 标题层未命中: {path}");
                continue;
            };
            let (iw, ih) = self
                .backend
                .as_ref()
                .and_then(|b| b.image_size(rid.0))
                .unwrap_or((1920, 1080));
            let layer = Layer {
                id,
                z: 1,
                visible: true,
                x: x as f32,
                y: y as f32,
                scale_x: LOGICAL_W / iw as f32,
                scale_y: LOGICAL_H / ih as f32,
                alpha: 1.0,
                rotation: 0.0,
                resource: Some(rid),
            };
            let scene = self.bridge.scene_mut();
            scene.upsert_layer(layer);
        }
        for (base, x, y, id) in buttons {
            // CONTINUE 无存档 → `_na` 单素材灰化(无三态/点击无效,成果 79)。
            let na = !has_save && base.ends_with("btn_lastload");
            let mut rids: [Option<ResourceId>; 3] = [None, None, None];
            if na {
                rids[TITLE_BTN_OFF] = self.load_scenario_image(&format!("{base}_na"));
            } else {
                // 三态预载(off/on/over;单态失败 = None,不切该态)
                for (i, suffix) in ["_off", "_on", "_over"].iter().enumerate() {
                    rids[i] = self.load_scenario_image(&format!("{base}{suffix}"));
                }
            }
            let Some(rid0) = rids[TITLE_BTN_OFF] else {
                eprintln!("[scenario] 标题按钮未命中: {base}_off");
                continue;
            };
            let (iw, ih) = self
                .backend
                .as_ref()
                .and_then(|b| b.image_size(rid0.0))
                .unwrap_or((317, 76));
            // 命中区(成果 76):原生尺寸,左上角 (x,y)
            self.title_buttons.push(TitleButton {
                rect: [x as f32, y as f32, iw as f32, ih as f32],
                id,
                rids,
                shown: TITLE_BTN_OFF,
                active: !na,
                hovered: false,
            });
            let layer = Layer {
                id,
                z: 1,
                visible: true,
                x: x as f32,
                y: y as f32,
                scale_x: 1.0,
                scale_y: 1.0,
                alpha: 1.0,
                rotation: 0.0,
                resource: Some(rid0),
            };
            let scene = self.bridge.scene_mut();
            scene.upsert_layer(layer);
        }
        eprintln!(
            "[scenario] 标题画面(eyecatch 分层组合;按钮菜单:ARASUJI/START/LOAD/LASTLOAD/EXTRA/END; \
             原生 es.BT 坐标;存档={has_save};三态 _off/_on/_over)"
        );
    }

    fn poll_choice(&mut self, count: usize) -> Option<usize> {
        if !self.frame_clicked {
            return None;
        }
        let (cx, cy) = self.cursor_logical;
        let Some(choices) = &self.choices else { return None };
        for (i, (_, rect)) in choices.iter().enumerate().take(count.max(1)) {
            let [x, y, w, h] = *rect;
            if cx >= x as i64
                && cx < (x + w) as i64
                && cy >= y as i64
                && cy < (y + h) as i64
            {
                eprintln!("[scenario] 选择 {i}");
                return Some(i);
            }
        }
        let _ = count;
        None
    }

    fn show_choices(&mut self, choices: &[String]) {
        // 竖排文本按钮(居中);命中区记录进 self.choices
        let mut rects = Vec::new();
        for (i, text) in choices.iter().enumerate() {
            let Some(rid) = self.render_text_layer(text) else { continue };
            let (tw, th) = self
                .backend
                .as_ref()
                .and_then(|b| b.image_size(rid.0))
                .unwrap_or((600, 60));
            let scale = 1.4f32;
            let w = tw as f32 * scale;
            let h = th as f32 * scale;
            let x = (LOGICAL_W - w) / 2.0;
            let y = 260.0 + i as f32 * (h + 40.0);
            // 暗色底板(白字可读)
            let bg_rid = ResourceId(0x5C_4000_0000 + i as u64);
            if !self.loaded.contains(&bg_rid.0) {
                if let Some(b) = self.backend.as_mut() {
                    let px = vec![20u8, 20, 30, 210];
                    let _ = b.load_image_rgba(bg_rid, &px, 1, 1);
                    self.loaded.insert(bg_rid.0);
                }
            }
            let bg_layer = Layer {
                id: 0x5C_5000_0000 + i as u64,
                z: 94,
                visible: true,
                x: x - 30.0,
                y: y - 20.0,
                scale_x: w + 60.0,
                scale_y: h + 40.0,
                alpha: 0.85,
                rotation: 0.0,
                resource: Some(bg_rid),
            };
            self.bridge.scene_mut().upsert_layer(bg_layer);
            let layer = Layer {
                id: 0x5C_3000_0000 + i as u64,
                z: 95,
                visible: true,
                x,
                y,
                scale_x: scale,
                scale_y: scale,
                alpha: 1.0,
                rotation: 0.0,
                resource: Some(rid),
            };
            self.bridge.scene_mut().upsert_layer(layer);
            rects.push((text.clone(), [x, y, w, h]));
        }
        self.choices = Some(rects);
    }

    fn clear_choices(&mut self) {
        let scene = self.bridge.scene_mut();
        if let Some(cs) = &self.choices {
            for i in 0..cs.len() {
                scene.hide_layer(0x5C_3000_0000 + i as u64);
                scene.hide_layer(0x5C_5000_0000 + i as u64);
            }
        }
        self.choices = None;
    }

    fn global(&self, slot: usize) -> i64 {
        // \GO.G.IF 的 G=n:仿真内部全局槽(成果 78 勘误:原 @50[n] 映射被
        // 运行时证伪 —— @50 dims=[1],idx≥1 越界;引擎真值存储 Unknown,
        // 待 es.BT.* 宏链逆向(B5)。本实现仅要求写入/读取内部自洽)。
        self.globals.get(&slot).copied().unwrap_or(0)
    }

    fn set_global(&mut self, slot: usize, value: i64) {
        // \TITLE 按钮选择写入(成果 78);与 global() 同一仿真内部槽。
        self.globals.insert(slot, value);
        eprintln!("[scenario] 写 G{slot}={value}");
    }

    fn log(&mut self, msg: &str) {
        eprintln!("[scenario] {msg}");
    }

    fn poll_title_menu(&mut self) -> Option<TitleMenuAction> {
        // P3 子画面打开中:点击全部由子画面消费,scenario 保持标题等待。
        if self.subui != SubUi::None {
            self.poll_subui();
            return None;
        }
        if !self.frame_clicked {
            return None;
        }
        let (cx, cy) = self.cursor_logical;
        for b in &self.title_buttons {
            if !b.active {
                continue; // `_na` 灰化态:点击无效(原生 BTN.LLOAD.NA,成果 79)
            }
            let [x, y, w, h] = b.rect;
            if cx >= x as i64
                && cx < (x + w) as i64
                && cy >= y as i64
                && cy < (y + h) as i64
            {
                let act = match b.id {
                    0x5C_7000_000A => TitleMenuAction::Outline,
                    0x5C_7000_0005 => TitleMenuAction::Start,
                    0x5C_7000_0006 => TitleMenuAction::Load,
                    0x5C_7000_0007 => TitleMenuAction::LastLoad,
                    0x5C_7000_0008 => TitleMenuAction::Extra,
                    0x5C_7000_0009 => TitleMenuAction::End,
                    _ => continue,
                };
                eprintln!("[scenario] 标题按钮 id={:#x} → {act:?}", b.id);
                self.play_sysse(TITLE_SE_DECIDE); // 决定音(P2-2,成果 80)
                return Some(act);
            }
        }
        None
    }

    fn title_load(&mut self) {
        // P3-1(成果 81):LOAD 存档列表画面(选中槽位经 request_load_path,
        // Player::tick 消费走 restore_from;读档完成 start() 重置等待)。
        self.open_load();
    }

    fn title_extra(&mut self) {
        // P3-2(成果 81):EXTRA → CG 鉴赏屏(原生 yst00257 流程:无落地菜单,
        // 顶部标签切 CG/BGM 模式)。
        self.open_extra_cg(0);
    }

    fn request_quit(&mut self) {
        // P3-3(成果 81):END → 确认对话框(はい 才置 request_quit 真退出)。
        self.open_confirm_end();
    }
}

pub struct Player {
    #[allow(dead_code)]
    pub game_dir: std::path::PathBuf,
    pub core: PlayerCore,
    pub scenario: ScenarioPlayer,
    pub window: Option<Arc<winit::window::Window>>,
    pub last_frame: Instant,
}

impl Player {
    pub fn tick(&mut self) {
        let core = &mut self.core;
        // WAIT 节拍
        if core.wait_frames > 0 {
            core.wait_frames -= 1;
        }
        if let Some(t) = core.wait_until {
            if Instant::now() < t {
                return;
            }
            core.wait_until = None;
        }
        if core.wait_frames > 0 {
            return;
        }
        core.drive_vm();
        // VM UI 层门控(P8.2b):进过标题且非标题等待期才放行;
        // false→true 沿 = 进入游戏 → 按注册表重放(boot 期被过滤的部件)
        let allow_vm_ui = core.title_seen && !self.scenario.in_title_menu();
        if allow_vm_ui && !core.vm_ui_allowed_prev {
            core.rebuild_vm_ui();
        }
        core.vm_ui_allowed_prev = allow_vm_ui;
        core.consume_events(allow_vm_ui);
        core.update_fade();
        core.update_sprite_fades();
        let clicked = std::mem::take(&mut core.clicked);
        self.scenario.tick(core, clicked);
        if core.subui == SubUi::None {
            core.update_title_buttons(); // 标题按钮三态(悬停/按下帧;成果 78)
        } else {
            core.update_ui_buttons(); // 子画面按钮三态(成果 81)
        }
        // 标题 LOAD:tick 内置请求,此处消费(scenario 已在快读中被 start() 重置)
        if core.request_title_load {
            core.request_title_load = false;
            core.frame_clicked = false; // 防快读后首帧被同一次点击推进
            let path = core.request_load_path.take().unwrap_or_else(|| {
                core.game_dir.join("save").join("yskernel_qsave.json")
            });
            drop(core);
            self.restore_from(&path);
            return;
        }
        if core.key_pulse {
            core.key_pulse_frames += 1;
            if core.key_pulse_frames > 120 {
                let _ = core.vm.clear_input_key();
                core.key_pulse = false;
                core.key_pulse_frames = 0;
            }
        }
        core.frame_clicked = false;
    }

    pub fn inject_key(&mut self, key: &str) {
        match self.core.vm.inject_input_key(key.as_bytes()) {
            Ok(()) => {
                self.core.key_pulse = true;
                self.core.key_pulse_frames = 0;
                eprintln!("[player] 注入按键 {key}");
            }
            Err(e) => eprintln!("[player] 注入失败: {e}"),
        }
    }

    pub fn quick_save(&mut self) {
        let Some((file, label)) = self.scenario.save_point().ok() else {
            eprintln!("[save] 当前无可存锚点");
            return;
        };
        let mut globals = Vec::new();
        // 全局槽 = 仿真内部存储(成果 78 勘误:原 @50 映射被证伪);
        // JSON 形状不变(globals[0..64]),槽 i → 下标 i。
        for i in 0..64 {
            globals.push(self.core.globals.get(&i).copied().unwrap_or(0));
        }
        let doc = serde_json::json!({
            "file": file, "label": label, "globals": globals,
        });
        let path = self.game_dir.join("save").join("yskernel_qsave.json");
        // save/ 目录可能不存在(原生引擎首次存档时自建;本目录为干净安装)
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[save] 目录创建失败: {e}");
                return;
            }
        }
        match std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()) {
            Ok(()) => eprintln!("[save] 快存 {file}/{label} → {}", path.display()),
            Err(e) => eprintln!("[save] 写入失败: {e}"),
        }
    }

    pub fn quick_load(&mut self) {
        self.restore_from(&self.game_dir.join("save").join("yskernel_qsave.json"));
    }

    /// 从指定存档 JSON 恢复(F9 快读与 LOAD 画面槽位共用;成果 81)。
    pub fn restore_from(&mut self, path: &std::path::Path) {
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("[load] 无快存");
            return;
        };
        let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("快存解析");
        let (Some(file), Some(label)) = (
            doc["file"].as_str().map(|x| x.to_string()),
            doc["label"].as_str().map(|x| x.to_string()),
        ) else {
            eprintln!("[load] 快存损坏");
            return;
        };
        // 全局槽恢复 = 仿真内部存储(成果 78 勘误;JSON 形状不变)。
        if let Some(arr) = doc["globals"].as_array() {
            for (i, v) in arr.iter().enumerate() {
                if let Some(n) = v.as_i64() {
                    self.core.globals.insert(i, n);
                }
            }
        }
        self.core.close_subui(); // 从 LOAD 画面读档 → 关闭子画面(成果 81)
        let scene = self.core.bridge.scene_mut();
        for id in self.core.sprites.drain(..) {
            scene.hide_layer(id);
        }
        self.core.sprite_fades.clear(); // 挂起淡入会把已隐藏层重新点亮(残留修复)
        scene.hide_layer(SC_TEXT);
        scene.hide_layer(SC_WIN);
        self.core.clear_choices();
        self.core.fade = None;
        let _ = self.scenario.start(&file, &label);
        eprintln!("[load] 恢复 {file}/{label}");
    }
}


impl PlayerCore {
    /// 标题层清理(reset 与进入对话时)。
    fn reset_title_layers(&mut self) {
        self.close_subui(); // 子画面(LOAD/EXTRA/确认框)一并复位(成果 81)
        let scene = self.bridge.scene_mut();
        for i in 0..12u64 {
            scene.hide_layer(0x5C_7000_0000 + i);
        }
        self.title_buttons.clear(); // 命中区同步失效(成果 78)
        // scenario 精灵层同步隐藏(残留修复,成果 81 勘误 3):快速点击压缩
        // 时序时,\\S.D 的淡出动画可能未完成即遇 reset —— 只清 sprite_fades
        // 不隐藏层,LOGO/注意事项等会以当时 alpha 永久残留(标题与正篇)。
        for id in self.sprites.drain(..) {
            scene.hide_layer(id);
        }
        self.sprite_fades.clear();
        scene.hide_layer(SC_TEXT);
        scene.hide_layer(SC_WIN);
        scene.hide_layer(SC_FADE);
        for i in 0..3u64 {
            scene.hide_layer(0x5C_6000_0000 + i);
        }
        self.clear_choices();
        self.fade = None;
    }
}

#[cfg(test)]
mod text_metrics_tests {
    //! 临时探针:实测字体 metrics(勿久留,验证后可删)。
    use super::*;

    #[test]
    fn probe_stheiti_metrics() {
        let bytes = load_cjk_font().expect("no cjk font");
        let font = fontdue::Font::from_bytes(
            bytes,
            fontdue::FontSettings { collection_index: 0, scale: 40.0, load_substitutions: true },
        )
        .unwrap();
        let hm = font.horizontal_line_metrics(40.0).expect("hm");
        println!("HM ascent={} descent={} line_gap={}", hm.ascent, hm.descent, hm.line_gap);
        for ch in ['猫', '渲', '。', '「', 'A'] {
            let (m, _) = font.rasterize(ch, 40.0);
            println!("'{ch}': width={} height={} xmin={} ymin={} adv={}",
                m.width, m.height, m.xmin, m.ymin, m.advance_width);
        }
    }
}

#[cfg(test)]
mod title_se_tests {
    //! P2-2 回归(成果 80):标题按钮悬停进入/点击 → sysse SE 决策。
    //! 空音频包下 resolve 未命中,决策经 `Audio::se_log` 断言(不出声)。

    use super::*;
    use yuris_format::ystb::YstbFile;

    /// 最小合成 YSTB(1 组 0 窗;GroupVm 唯一构造路径 `load`)。
    fn minimal_vm() -> GroupVm {
        fn xor(r: &[u8], key: [u8; 4]) -> Vec<u8> {
            r.iter().enumerate().map(|(i, b)| b ^ key[i % 4]).collect()
        }
        let key = [0x2b, 0x90, 0x4f, 0x93];
        let part1 = xor(&[0u8, 0, 0, 0], key); // 1 组 0 窗(Σcount*12==0)
        let p4 = xor(&[0u8; 4], key);
        let mut out = Vec::new();
        out.extend_from_slice(b"YSTB");
        out.extend_from_slice(&555u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // unknown1 = 组数
        out.extend_from_slice(&(part1.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // command_len
        out.extend_from_slice(&0u32.to_le_bytes()); // content_len
        out.extend_from_slice(&(p4.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&part1);
        out.extend_from_slice(&p4);
        GroupVm::load(YstbFile::from_bytes(&out, key).expect("ystb")).expect("vm")
    }

    fn make_core() -> PlayerCore {
        let bytes = load_cjk_font().expect("no cjk font");
        let font = fontdue::Font::from_bytes(
            bytes,
            fontdue::FontSettings { collection_index: 0, scale: 40.0, load_substitutions: true },
        )
        .unwrap();
        PlayerCore {
            index: Arc::new(PacFileIndex::default()),
            vm: minimal_vm(),
            bridge: SceneBridge::new(),
            loaded: HashSet::new(),
            backend: None,
            events_cursor: 0,
            wait_frames: 0,
            wait_until: None,
            cursor_logical: (0, 0),
            key_pulse: false,
            key_pulse_frames: 0,
            font,
            fade: None,
            clicked: false,
            sprites: Vec::new(),
            sprite_fades: Vec::new(),
            audio_packs: Arc::new(PacFileIndex::default()),
            audio: crate::audio::Audio::new(),
            choices: None,
            last_cursor_px: None,
            frame_clicked: false,
            title_buttons: Vec::new(),
            request_title_load: false,
            request_quit: false,
            globals: HashMap::new(),
            subui: SubUi::None,
            ui_buttons: Vec::new(),
            save_entries: Vec::new(),
            request_load_path: None,
            ev_list: Vec::new(),
            cg_page: 0,
            cg_view: None,
            bgm_tracks: Vec::new(),
            bgm_page: 0,
            vm_ui_layers: Vec::new(),
            title_seen: false,
            vm_ui_prio: HashMap::new(),
            vm_ui_allowed_prev: false,
            game_dir: std::path::PathBuf::new(),
        }
    }

    fn btn(id: u64, active: bool) -> TitleButton {
        TitleButton {
            rect: [100.0, 100.0, 50.0, 50.0],
            id,
            rids: [None, None, None],
            shown: TITLE_BTN_OFF,
            hovered: false,
            active,
        }
    }

    #[test]
    fn title_hover_se_fires_on_enter_edge_only() {
        let mut core = make_core();
        core.title_buttons.push(btn(0x5C_7000_0005, true));
        core.cursor_logical = (110, 110);
        core.update_title_buttons();
        assert_eq!(core.audio.se_log, vec!["sse02"]);
        core.update_title_buttons(); // 悬停保持:不重复
        assert_eq!(core.audio.se_log.len(), 1);
        core.cursor_logical = (10, 10);
        core.update_title_buttons(); // 离开
        assert_eq!(core.audio.se_log.len(), 1);
        core.cursor_logical = (120, 120);
        core.update_title_buttons(); // 再进入:再响一次
        assert_eq!(core.audio.se_log, vec!["sse02", "sse02"]);
    }

    #[test]
    fn title_click_plays_decide_se() {
        let mut core = make_core();
        core.title_buttons.push(btn(0x5C_7000_0005, true));
        core.cursor_logical = (110, 110);
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), Some(TitleMenuAction::Start));
        assert_eq!(core.audio.se_log, vec!["sse03"]);
    }

    #[test]
    fn title_na_button_is_silent() {
        let mut core = make_core();
        core.title_buttons.push(btn(0x5C_7000_0007, false));
        core.cursor_logical = (110, 110);
        core.update_title_buttons(); // `_na`:不参与三态/不响
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), None); // 点击无效
        assert!(core.audio.se_log.is_empty());
    }
}

#[cfg(test)]
mod subui_tests {
    //! P3 回归(成果 81):END 确认流 / LOAD 槽位选择 / EXTRA 导航。
    //! 空索引 + 无 backend:层不落,状态机与命中区断言照常成立。

    use super::*;

    fn make_core() -> PlayerCore {
        let bytes = load_cjk_font().expect("no cjk font");
        let font = fontdue::Font::from_bytes(
            bytes,
            fontdue::FontSettings { collection_index: 0, scale: 40.0, load_substitutions: true },
        )
        .unwrap();
        PlayerCore {
            index: Arc::new(PacFileIndex::default()),
            vm: minimal_vm(),
            bridge: SceneBridge::new(),
            loaded: HashSet::new(),
            backend: None,
            events_cursor: 0,
            wait_frames: 0,
            wait_until: None,
            cursor_logical: (0, 0),
            key_pulse: false,
            key_pulse_frames: 0,
            font,
            fade: None,
            clicked: false,
            sprites: Vec::new(),
            sprite_fades: Vec::new(),
            audio_packs: Arc::new(PacFileIndex::default()),
            audio: crate::audio::Audio::new(),
            choices: None,
            last_cursor_px: None,
            frame_clicked: false,
            title_buttons: Vec::new(),
            request_title_load: false,
            request_quit: false,
            globals: HashMap::new(),
            subui: SubUi::None,
            ui_buttons: Vec::new(),
            save_entries: Vec::new(),
            request_load_path: None,
            ev_list: Vec::new(),
            cg_page: 0,
            cg_view: None,
            bgm_tracks: Vec::new(),
            bgm_page: 0,
            vm_ui_layers: Vec::new(),
            title_seen: false,
            vm_ui_prio: HashMap::new(),
            vm_ui_allowed_prev: false,
            game_dir: std::path::PathBuf::new(),
        }
    }

    /// 复用 title_se_tests 的最小 VM 构造(同名逻辑独立成此处夹具)。
    fn minimal_vm() -> GroupVm {
        fn xor(r: &[u8], key: [u8; 4]) -> Vec<u8> {
            r.iter().enumerate().map(|(i, b)| b ^ key[i % 4]).collect()
        }
        let key = [0x2b, 0x90, 0x4f, 0x93];
        let part1 = xor(&[0u8, 0, 0, 0], key);
        let p4 = xor(&[0u8; 4], key);
        let mut out = Vec::new();
        out.extend_from_slice(b"YSTB");
        out.extend_from_slice(&555u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(part1.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(p4.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&part1);
        out.extend_from_slice(&p4);
        GroupVm::load(yuris_format::ystb::YstbFile::from_bytes(&out, key).expect("ystb"))
            .expect("vm")
    }

    fn temp_game_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("yuris_p3_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("save")).unwrap();
        dir
    }

    #[test]
    fn end_confirm_yes_quits_no_stays() {
        let mut core = make_core();
        core.title_buttons.push(TitleButton {
            rect: [100.0, 100.0, 50.0, 50.0],
            id: 0x5C_7000_0009, // END
            rids: [None, None, None],
            shown: TITLE_BTN_OFF,
            hovered: false,
            active: true,
        });
        core.cursor_logical = (110, 110);
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), Some(TitleMenuAction::End));
        core.request_quit(); // scenario 路由:END → 确认对话框
        assert_eq!(core.subui, SubUi::ConfirmEnd);
        assert!(!core.request_quit, "确认前不得置退出请求");
        // いいえ → 留在标题
        core.cursor_logical = (1100, 490); // btn_no(1040,470,146,45)
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), None);
        assert_eq!(core.subui, SubUi::None);
        assert!(!core.request_quit);
        assert_eq!(core.audio.se_log.last().map(String::as_str), Some("sse06"));
        // 再走一遍 → はい → 退出请求
        core.request_quit();
        core.cursor_logical = (810, 490); // btn_yes(740,470,146,45)
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), None);
        assert!(core.request_quit);
    }

    #[test]
    fn load_screen_slot_click_requests_restore() {
        let dir = temp_game_dir("load");
        std::fs::write(
            dir.join("save").join("yskernel_qsave.json"),
            r#"{"file":"maho2_01.txt","label":"m1","globals":[0]}"#,
        )
        .unwrap();
        let mut core = make_core();
        core.game_dir = dir.clone();
        core.open_load();
        assert_eq!(core.subui, SubUi::Load);
        assert_eq!(core.save_entries.len(), 1);
        // 戻る → 回标题
        core.cursor_logical = (1700, 960); // btn_back(1660,940,186,87)
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), None);
        assert_eq!(core.subui, SubUi::None);
        // 重开 → 点槽位 0 → 读档请求
        core.open_load();
        core.cursor_logical = (400, 240); // 槽 0(320,200,1280,80)
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), None);
        assert!(core.request_title_load);
        assert_eq!(
            core.request_load_path.as_deref(),
            Some(dir.join("save").join("yskernel_qsave.json").as_path())
        );
        assert_eq!(core.audio.se_log.last().map(String::as_str), Some("sse03"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn vm_ui_filter_rules() {
        // P8.2b(成果 82/勘误 7):debug 覆盖层/标题期/title-extra 通道不建层;
        // main/saveload/config 等全 cgsys 系统 UI 放行
        let main = "cgsys/main/button/type1/tip_meswindow";
        let main_bs = r"cgsys\main\button\type1\tip_meswindow";
        let debug = r"cgsys\debug\btn_back";
        let title = "cgsys/title/btn_start_off";
        let extra = r"cgsys\extra\btn_tab_cgmode_bt3n";
        let saveload = r"cgsys\saveload\btn_back_off";
        assert!(vm_ui_layer_allowed(main, true));
        assert!(vm_ui_layer_allowed(main_bs, true));
        assert!(vm_ui_layer_allowed(saveload, true));
        assert!(!vm_ui_layer_allowed(main, false)); // 标题等待期
        assert!(!vm_ui_layer_allowed(debug, true)); // debug 覆盖层
        assert!(!vm_ui_layer_allowed(title, true)); // 内置标题接管
        assert!(!vm_ui_layer_allowed(extra, true)); // EXTRA 子画面接管
    }

    #[test]
    fn vm_ui_base_prio_collapses_states() {
        // 勘误 8:同钮各态同基名,优先级 OFF(0) 最低 → 只显示 OFF 层
        let off = b"ES.GAMEMAIN.BTN.VOICEM.BT.OFF=0=1";
        let over = b"ES.GAMEMAIN.BTN.VOICEM.BT.OVER=0=1";
        let onov = b"ES.GAMEMAIN.BTN.VOICEM.BT.ONOV=0=1";
        let na = b"ES.GAMEMAIN.TIP.NAMEW.TXM.BT.NA=0=1";
        let plain = b"some-cg-name";
        let base_off = vm_ui_base_prio(off);
        let base_over = vm_ui_base_prio(over);
        let base_onov = vm_ui_base_prio(onov);
        let base_na = vm_ui_base_prio(na);
        assert_eq!(base_off.0, base_over.0);
        assert_eq!(base_over.0, base_onov.0);
        assert!(base_off.1 < base_over.1); // OFF(0) < OVER(2)
        assert!(base_over.1 < base_onov.1); // OVER(2) < ONOV(3)
        assert!(base_na.1 > base_off.1); // NA(4) 最差但 NA-only 也显示
        assert_eq!(vm_ui_base_prio(plain), (plain.to_vec(), 0)); // 无 .BT. 原样
    }

    #[test]
    fn reset_hides_pending_sprites() {
        // 残留修复(成果 81 勘误 3):快速点击时序压缩 → reset 时淡出未完成
        // → 精灵层必须同步隐藏(此前只清 sprite_fades,层以当时 alpha 残留)。
        let mut core = make_core();
        let id = 0x1234;
        core.sprites.push(id);
        core.sprite_fades.push((id, Instant::now(), 800, true));
        core.bridge.scene_mut().upsert_layer(Layer {
            id,
            z: 10,
            visible: true,
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            alpha: 0.4,
            rotation: 0.0,
            resource: None,
        });
        core.reset_title_layers();
        let l = core
            .bridge
            .scene()
            .layers
            .iter()
            .find(|l| l.id == id)
            .expect("层应在场景中(隐藏态)");
        assert!(!l.visible);
        assert!(core.sprite_fades.is_empty());
        assert!(core.sprites.is_empty());
    }

    #[test]
    fn extra_screens_nav_and_back() {
        let mut core = make_core();
        // EXTRA → 直达 CG 鉴赏(原生 yst00257 流程,无落地菜单)
        core.title_extra();
        assert_eq!(core.subui, SubUi::ExtraCg);
        // BGM 标签(1272,10,237,53;4 钮标签行第 4 位)
        core.cursor_logical = (1350, 30);
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), None);
        assert_eq!(core.subui, SubUi::ExtraBgm);
        // CG 标签(531,10,237,53)
        core.cursor_logical = (600, 30);
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), None);
        assert_eq!(core.subui, SubUi::ExtraCg);
        // 戻る(1620,930,237,80)→ 标题
        core.cursor_logical = (1700, 960);
        core.frame_clicked = true;
        assert_eq!(core.poll_title_menu(), None);
        assert_eq!(core.subui, SubUi::None);
    }
}
