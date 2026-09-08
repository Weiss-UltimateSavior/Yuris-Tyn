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

fn scene_mut_layer(scene: &mut yuris_scene::Scene, id: u64) -> Option<&mut Layer> {
    scene.layers.iter_mut().find(|l| l.id == id)
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
    /// 注(P8 集成取舍):VM 的 CG 图层(含引擎 debug 覆盖层)不进场景 ——
    /// 视觉由 scenario 层驱动;VM 仅维护系统状态(变量/流程/文本事件)。
    fn consume_events(&mut self) {
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
            if let yuris_vm::VmEvent::Cg { pc, script_id, id, file, .. } = ev {
                let Some(name) = id else { continue };
                let rid = fnv1a(name.as_bytes());
                if self.loaded.contains(&rid) {
                    continue;
                }
                let Some(f) = file else { continue };
                if f.is_empty() {
                    continue;
                }
                let path = String::from_utf8_lossy(f).into_owned();
                match self.index.read_image_bytes(&path) {
                    Some(data) => to_load.push((rid, data)),
                    None => eprintln!(
                        "[player] CG 图像解析失败:{path} (s{script_id} pc={pc})"
                    ),
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
    fn update_title_buttons(&mut self) {
        if self.title_buttons.is_empty() {
            return;
        }
        let (cx, cy) = self.cursor_logical;
        let clicked = self.frame_clicked;
        let mut updates: Vec<(u64, ResourceId)> = Vec::new();
        for b in &mut self.title_buttons {
            if !b.active {
                continue; // `_na` 灰化态:单素材,不参与三态(成果 79)
            }
            let [x, y, w, h] = b.rect;
            let hover = cx >= x as i64
                && cx < (x + w) as i64
                && cy >= y as i64
                && cy < (y + h) as i64;
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
            // 淡出:动画完成由 tick 收尾隐藏
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
        self.show_window_frame();
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
            self.audio.play_se(data);
        } else {
            eprintln!("[audio] se 未命中: {name}");
        }
    }

    fn title_screen(&mut self) {
        // 内置标题:eyecatch/st 分层素材组合(bg01a + sir/han + logo + 真实按钮列)
        // 布局对齐真机截图(用户 2026-09-05 提供):银发左 / 双马尾中 / logo 右上 / 按钮右下
        self.reset_title_layers();
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
                return Some(act);
            }
        }
        None
    }

    fn title_load(&mut self) {
        // LOAD/LASTLOAD 屏未实现(P0):两钮暂同走快读恢复(引擎 LASTLOAD = 最近存档);
        // quick_load 需 &mut Player(scenario 分裂借用),此处仅置请求,Player::tick 消费
        self.request_title_load = true;
    }

    fn title_extra(&mut self) {
        eprintln!("[scenario] EXTRA 未实现(留在标题)");
    }

    fn request_quit(&mut self) {
        self.request_quit = true;
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
        core.consume_events();
        core.update_fade();
        core.update_sprite_fades();
        let clicked = std::mem::take(&mut core.clicked);
        self.scenario.tick(core, clicked);
        core.update_title_buttons(); // 标题按钮三态(悬停/按下帧;成果 78)
        // 标题 LOAD/LASTLOAD:tick 内置请求,此处消费(scenario 已在快读中被 start() 重置)
        if core.request_title_load {
            core.request_title_load = false;
            core.frame_clicked = false; // 防快读后首帧被同一次点击推进
            drop(core);
            self.quick_load();
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
        match std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()) {
            Ok(()) => eprintln!("[save] 快存 {file}/{label} → {}", path.display()),
            Err(e) => eprintln!("[save] 写入失败: {e}"),
        }
    }

    pub fn quick_load(&mut self) {
        let path = self.game_dir.join("save").join("yskernel_qsave.json");
        let Ok(bytes) = std::fs::read(&path) else {
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
        let scene = self.core.bridge.scene_mut();
        for id in self.core.sprites.drain(..) {
            scene.hide_layer(id);
        }
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
        let scene = self.bridge.scene_mut();
        for i in 0..12u64 {
            scene.hide_layer(0x5C_7000_0000 + i);
        }
        self.title_buttons.clear(); // 命中区同步失效(成果 78)
        self.sprites.clear();
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
