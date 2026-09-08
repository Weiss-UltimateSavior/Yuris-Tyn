//! yuris-cli 可执行文件:兼容内核播放器(桌面壳)。
//!
//! 桌面平台壳:winit 事件循环 + 窗口创建 + 键鼠输入翻译。播放核心
//! (PlayerCore/Player/scenario/音频)已抽至 `yuris-player-core`,与
//! Android 壳(`neko-android`)共用。
//!
//! `run <游戏目录>`:双脚本系统并行驱动 ——
//! 1. **YSTB VM**:Bootstrap 启动链(SYSTEM_START)→ 帧节拍驱动
//!    (WAIT FRAME/TIME 挂起恢复)→ SceneBridge(CG/CGEND/TEXT → 图层);
//! 2. **scenario 播放器**(P7.3):sc.ypf 明文剧本(Big5)→ 背景/立绘/
//!    淡入淡出/双语台词(点击推进)。
//!
//! 用法:cargo run --release -p yuris-cli -- run "AnimalTrailGirlishSquare 2"

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use yuris_render::WgpuBackend;
use yuris_vm::bridge::SceneBridge;
use yuris_vm::boot::Bootstrap;
use yuris_vm::host::PacFileIndex;

use yuris_player_core::{letterbox_logical, load_cjk_font, Player, PlayerCore, ScenarioPlayer};

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    if cmd != "run" {
        eprintln!("用法: yuris-cli run <游戏目录>");
        std::process::exit(2);
    }
    let dir = args.next().unwrap_or_else(|| ".".into());
    let rest: Vec<String> = args.collect();
    let at_label: Option<String> = rest
        .iter()
        .position(|a| a == "--at")
        .and_then(|i| rest.get(i + 1).cloned());
    // 调试项:YSTB 4 字节循环 XOR 密钥(不同游戏各异)。缺省 = 本项目样本密钥。
    // 正式方案是启动链对首个 YSTB 条目自动 guess-key(见 yuris ystb guess-key)。
    let ystb_key: [u8; 4] = rest
        .iter()
        .position(|a| a == "--key-hex")
        .and_then(|i| rest.get(i + 1))
        .and_then(|hex| {
            if hex.len() == 8 {
                u32::from_str_radix(hex, 16)
                    .ok()
                    .map(|v| v.to_be_bytes())
            } else {
                None
            }
        })
        .unwrap_or([0x2b, 0x90, 0x4f, 0x93]);
    // 调试项:非 strict 模式 —— Unsupported 命令记录事件后继续(默认 strict 挂起)。
    let lenient = rest.iter().any(|a| a == "--lenient");
    let game_dir = std::path::PathBuf::from(&dir);
    if !game_dir.is_dir() {
        eprintln!("游戏目录不存在:{}", game_dir.display());
        std::process::exit(2);
    }

    // ---- YSTB VM boot ----
    let ypf = game_dir.join("pac").join("bn.ypf");
    let bytes = std::fs::read(&ypf).unwrap_or_else(|e| panic!("读 {}: {e}", ypf.display()));
    let mut index = PacFileIndex::scan_game_dir(&game_dir, 0xC9).expect("扫描 pac");
    index.add_virtual("cg/thumb_cg/A_HAN_2002_a.png"); // 引擎环境复现(成果 58)
    let index = Arc::new(index);
    let booted = Bootstrap {
        ypf_bytes: bytes,
        name_key: 0xC9,
        key: ystb_key,
        entry_label: None, // SYSTEM_START
    }
    .boot()
    .expect("启动链编排");
    eprintln!(
        "[player] 入口 script {} pc {}(YSVR {} 条)",
        booted.script_id, booted.entry_pc, booted.applied_ysvr
    );
    let mut vm = booted.vm;
    vm.set_file_probe(index.clone());
    if lenient {
        vm.set_strict(false);
    }

    // ---- scenario 文件(P7.3)----
    let sc_ypf = game_dir.join("pac").join("sc.ypf");
    let sc_bytes = std::fs::read(&sc_ypf).expect("读 sc.ypf");
    let arch = yuris_format::ypf::YpfArchive::from_bytes(sc_bytes, 0xC9).expect("sc.ypf 索引");
    let mut sc_files: Vec<(String, Vec<u8>)> = Vec::new();
    for e in arch.entries() {
        if e.name.ends_with(".txt") {
            let data = arch.read(&e.name).expect("读 scenario 条目");
            sc_files.push((e.name.clone(), data));
        }
    }
    eprintln!("[player] scenario 文件 {} 个", sc_files.len());
    let scenario = ScenarioPlayer::new(sc_files);

    // ---- 音频包(P9.1)----
    let audio_packs = yuris_vm::host::PacFileIndex::scan_game_dir(&game_dir, 0xC9)
        .expect("音频包索引");
    let audio = yuris_player_core::audio::Audio::new();

    // ---- CJK 字体(繁中优先;按平台回退)----
    let font_bytes = load_cjk_font().expect("系统 CJK 字体");
    let font = fontdue::Font::from_bytes(
        font_bytes,
        fontdue::FontSettings {
            collection_index: 0,
            scale: 40.0,
            load_substitutions: true,
        },
    )
    .expect("字体解析");

    let core = PlayerCore {
        index,
        vm,
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
        audio_packs: Arc::new(audio_packs),
        audio,
        choices: None,
        last_cursor_px: None,
        frame_clicked: false,
        title_buttons: Vec::new(),
        request_title_load: false,
        request_quit: false,
        globals: std::collections::HashMap::new(),
        subui: yuris_player_core::SubUi::None,
        ui_buttons: Vec::new(),
        save_entries: Vec::new(),
        request_load_path: None,
        ev_list: Vec::new(),
        cg_page: 0,
        cg_view: None,
        bgm_tracks: Vec::new(),
        bgm_page: 0,
        game_dir: game_dir.clone(),
    };
    let mut player = Player {
        game_dir,
        core,
        scenario,
        window: None,
        last_frame: Instant::now(),
    };
    // scenario 入口:scenario_start.txt → RELEASE 段(构建模式段选择,Likely)
    // scenario 入口:--at 调试跳转 或 scenario_start.txt RELEASE 段
    let started = if let Some(l) = &at_label {
        match player.scenario.goto(l) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[scenario] --at {l} 跳转失败: {e}");
                false
            }
        }
    } else {
        player
            .scenario
            .start("scenario\\scenario_start.txt", "SCENARIO_START_RELEASE")
            .is_ok()
    };
    if !started {

    }

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let mut wrapper = AppWrapper(Some(player));
    if let Err(e) = event_loop.run_app(&mut wrapper) {
        eprintln!("event loop: {e}");
        std::process::exit(1);
    }
}

/// 客户区像素 → 逻辑坐标(整窗线性映射;letterbox 误差由渲染侧吸收)。
/// winit 0.30 ApplicationHandler 要求 &mut App;包一层以支持延迟构造窗口。
struct AppWrapper(Option<Player>);

impl winit::application::ApplicationHandler for AppWrapper {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(player) = self.0.as_mut() else {
            return;
        };
        if player.window.is_some() {
            return;
        }
        let attrs = winit::window::Window::default_attributes()
            .with_title("YurisKernel — Kemonomichi Girlish Square 2")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("create window: {e}");
                event_loop.exit();
                return;
            }
        };
        match WgpuBackend::new(window.clone()) {
            Ok(b) => {
                player.core.backend = Some(b);
                player.window = Some(window);
            }
            Err(e) => {
                eprintln!("wgpu 初始化失败: {e}");
                event_loop.exit();
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(p) = self.0.as_mut() {
            if p.window.is_some() && p.last_frame.elapsed().as_millis() >= 15 {
                p.window.as_ref().unwrap().request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let Some(player) = self.0.as_mut() else {
            return;
        };
        match event {
            winit::event::WindowEvent::CloseRequested => event_loop.exit(),
            winit::event::WindowEvent::KeyboardInput { event, .. } => {
                if event.state == winit::event::ElementState::Pressed
                    && event.logical_key
                        == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape)
                {
                    event_loop.exit();
                }
                if event.state == winit::event::ElementState::Pressed {
                    let name: Option<&str> = match &event.logical_key {
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter) => {
                            Some("ENTER")
                        }
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Space) => {
                            Some("SPACE")
                        }
                        winit::keyboard::Key::Character(c) => Some(c.as_str()),
                        _ => None,
                    };
                    if let Some(k) = name {
                        player.inject_key(k);
                    }
                    match event.logical_key {
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::F5) => {
                            player.quick_save();
                        }
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::F9) => {
                            player.quick_load();
                        }
                        _ => {}
                    }
                }
            }
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                player.core.last_cursor_px = Some((position.x, position.y));
                let (ww, wh) = player
                    .core
                    .backend
                    .as_ref()
                    .map(|b| b.surface_size())
                    .unwrap_or((0, 0));
                let (lx, ly) = letterbox_logical(ww, wh, position.x, position.y);
                player.core.cursor_logical = (lx, ly);
                player.core.vm.set_input_cursor(lx, ly);
            }
            winit::event::WindowEvent::MouseInput { state, button, .. } => {
                if state == winit::event::ElementState::Pressed
                    && button == winit::event::MouseButton::Left
                {
                    // 合成点击不一定产生 CursorMoved:用最近一次移动的像素位
                    if let Some((px, py)) = player.core.last_cursor_px {
                        let (ww, wh) = player
                            .core
                            .backend
                            .as_ref()
                            .map(|b| b.surface_size())
                            .unwrap_or((0, 0));
                        let (lx, ly) = letterbox_logical(ww, wh, px, py);
                        player.core.cursor_logical = (lx, ly);
                        player.core.vm.set_input_cursor(lx, ly);
                    }
                    let (x, y) = player.core.cursor_logical;
                    player.inject_key("MOUSE_L");
                    player.core.clicked = true;
                    player.core.frame_clicked = true;
                }
            }
            winit::event::WindowEvent::Resized(size) => {
                if let Some(b) = player.core.backend.as_mut() {
                    b.resize(size.width, size.height);
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                player.tick();
                if player.core.request_quit {
                    event_loop.exit();
                    return;
                }
                if let Some(b) = player.core.backend.as_mut() {
                    if let Err(e) = b.render(player.core.bridge.scene()) {
                        eprintln!("渲染失败: {e}");
                        event_loop.exit();
                        return;
                    }
                }
                player.last_frame = Instant::now();
                if let Some(w) = player.window.as_ref() {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}
