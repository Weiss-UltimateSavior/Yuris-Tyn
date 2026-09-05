//! M1 首像素示例(P8.1/P8.2,成果 65)。
//!
//! 复现引擎启动序列的图形面(scenario_start.txt,成果 49):
//!   \LOGO.NOSAVE → \FOUT(0,0) → \FIN(500) → \S(logo, item/logo_wp)
//!   → \WA(3000) → \S.D(logo, 800) → \S(attention,…) → \WA…
//! 本例:挂载 cg.ypf(ResourceStack)→ 载入 logo.png / attention_1.png →
//! 时间轴驱动 Scene(logo 淡入 3s → 注意事项图全屏)→ wgpu 合成呈现。
//!
//! 用法(仓库根):cargo run --release -p yuris-render --example first_pixel
//! Esc / 关窗退出。

use std::sync::Arc;
use std::time::Instant;

use yuris_render::{WgpuBackend, LOGICAL_H, LOGICAL_W};
use yuris_resource::ResourceStack;
use yuris_runtime::GraphicsBackend;
use yuris_scene::{Layer, ResourceId, Scene};

/// 图层 id(CG 名派生;M1 阶段工程选择:直接用枚举值)。
const ID_LOGO: u64 = 1;
const ID_ATTENTION: u64 = 2;

fn main() {
    let game_dir = std::path::Path::new("AnimalTrailGirlishSquare 2");
    if !game_dir.is_dir() {
        eprintln!("样本目录不存在:{}", game_dir.display());
        std::process::exit(1);
    }
    let mut stack = ResourceStack::new(0xC9);
    if let Err(e) = stack.mount(game_dir.join("pac").join("cg.ypf")) {
        eprintln!("挂载 cg.ypf 失败: {e}");
        std::process::exit(1);
    }
    let logo = stack.read("cg\\eyecatch\\st\\logo.png").expect("读 logo.png");
    let attention = stack.read("cg\\item\\attention_1.png").expect("读 attention_1.png");

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let mut app = App {
        window: None,
        backend: None,
        logo_size: (1, 1),
        attention_size: (1, 1),
        t0: Instant::now(),
        logo,
        attention,
    };

    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("event loop: {e}");
        std::process::exit(1);
    }
}

struct App {
    window: Option<Arc<winit::window::Window>>,
    backend: Option<WgpuBackend>,
    logo_size: (u32, u32),
    attention_size: (u32, u32),
    t0: Instant,
    logo: Vec<u8>,
    attention: Vec<u8>,
}

impl winit::application::ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = winit::window::Window::default_attributes()
            .with_title("YurisKernel — M1 首像素(logo → attention)")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("create window: {e}");
                event_loop.exit();
                return;
            }
        };
        let mut backend = match WgpuBackend::new(window.clone()) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("wgpu 初始化失败: {e}");
                event_loop.exit();
                return;
            }
        };
        if let Err(e) = backend.load_image(ResourceId(ID_LOGO), &self.logo) {
            eprintln!("载入 logo 失败: {e}");
            event_loop.exit();
            return;
        }
        if let Err(e) = backend.load_image(ResourceId(ID_ATTENTION), &self.attention) {
            eprintln!("载入 attention 失败: {e}");
            event_loop.exit();
            return;
        }
        self.logo_size = backend.image_size(ID_LOGO).expect("logo 尺寸");
        self.attention_size = backend.image_size(ID_ATTENTION).expect("attention 尺寸");
        self.window = Some(window);
        self.backend = Some(backend);
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // 连续重绘(演出时间轴驱动)
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            winit::event::WindowEvent::CloseRequested => event_loop.exit(),
            winit::event::WindowEvent::KeyboardInput { event, .. } => {
                if event.state == winit::event::ElementState::Pressed
                    && event.logical_key
                        == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape)
                {
                    event_loop.exit();
                }
            }
            winit::event::WindowEvent::Resized(size) => {
                if let Some(b) = self.backend.as_mut() {
                    b.resize(size.width, size.height);
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                let Some(backend) = self.backend.as_mut() else {
                    return;
                };
                // 时间轴(对应 scenario_start.txt:\FIN(500) 淡入 → \WA(3000) → attention)
                let t = self.t0.elapsed().as_secs_f32();
                let mut scene = Scene::default();
                if t < 3.0 {
                    // \FIN(500):0.5s 淡入
                    let alpha = (t / 0.5).clamp(0.0, 1.0);
                    let (w, h) = self.logo_size;
                    scene.upsert_layer(Layer {
                        id: ID_LOGO,
                        z: 0,
                        visible: true,
                        x: (LOGICAL_W - w as f32) / 2.0,
                        y: (LOGICAL_H - h as f32) / 2.0,
                        scale_x: 1.0,
                        scale_y: 1.0,
                        alpha,
                        rotation: 0.0,
                        resource: Some(ResourceId(ID_LOGO)),
                    });
                } else {
                    // 注意事项图:铺满逻辑区(按实际尺寸缩放)
                    let (aw, ah) = self.attention_size;
                    scene.upsert_layer(Layer {
                        id: ID_ATTENTION,
                        z: 0,
                        visible: true,
                        x: 0.0,
                        y: 0.0,
                        scale_x: LOGICAL_W / aw as f32,
                        scale_y: LOGICAL_H / ah as f32,
                        alpha: 1.0,
                        rotation: 0.0,
                        resource: Some(ResourceId(ID_ATTENTION)),
                    });
                }
                if let Err(e) = backend.render(&scene) {
                    eprintln!("渲染失败: {e}");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}
