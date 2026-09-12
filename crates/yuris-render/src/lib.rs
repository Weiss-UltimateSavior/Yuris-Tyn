//! yuris-render
//!
//! L6:GraphicsBackend 实现 —— winit + wgpu(P8.1/P8.2,成果 65)。
//!
//! - **P8.1 窗口/设备初始化**:winit 窗口 + wgpu surface/adapter/device。
//! - **P8.2 图层合成**:引擎逻辑分辨率 1920×1080(WINDOWINFO SX/SY oracle,
//!   成果 60)→ 视口 letterbox 缩放;Layer 的 x/y/scale/alpha/z 合成
//!   (z 升序绘制;预乘 alpha 混合)。
//! - `load_image`:PNG(image crate,成果 63 标准 PNG 直读)→ wgpu 纹理。
//! - `draw_text`:**P8.3 待实现**(SJIS 字体/换行细节),当前记录后跳过。
//!
//! 坐标系:引擎逻辑坐标(1920×1080)原点 = 左上;`Layer.x/y` = 图层
//! 左上角;`scale` 以图像原始像素为基准。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use yuris_runtime::{BackendError, GraphicsBackend, Result};
use yuris_scene::{Layer, ResourceId, Scene, TextLayout};

/// crate 版本（与 workspace 同步）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 引擎逻辑分辨率宽(WINDOWINFO SX oracle = 1920,成果 60)。
pub const LOGICAL_W: f32 = 1920.0;
/// 引擎逻辑分辨率高(WINDOWINFO SY oracle = 1080,成果 60)。
pub const LOGICAL_H: f32 = 1080.0;

const UNIFORM_STRIDE: u64 = 256;
const MAX_LAYERS: usize = 1024;

const SHADER: &str = r#"
struct Uniforms {
    transform: mat4x4<f32>,
    color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VSOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) xy: vec2<f32>, @location(1) uv: vec2<f32>) -> VSOut {
    var out: VSOut;
    out.pos = u.transform * vec4<f32>(xy, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv);
    let a = c.a * u.color.a;
    let tinted = c.rgb * u.color.rgb;
    return vec4<f32>(tinted * a, a);
}
"#;

/// 单个已加载纹理。
struct GpuImage {
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

/// wgpu 图形后端(P8.1/P8.2)。
pub struct WgpuBackend {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    uniform_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// 动态 uniform(256B 对齐;矩阵 64 + 颜色 16)。
    uniform_buf: wgpu::Buffer,
    /// 单位方块顶点缓冲(共享)。
    quad_buf: wgpu::Buffer,
    /// 每帧收集的待绘制图层(P8.2:z 升序合成)。
    frame_layers: Vec<Layer>,
    images: HashMap<u64, GpuImage>,
    surface_size: (u32, u32),
}

impl WgpuBackend {
    /// 从 winit 窗口创建后端(阻塞式初始化)。
    ///
    /// 窗口尺寸变化时调用 [`Self::resize`]。
    pub fn new(window: Arc<winit::window::Window>) -> Result<Self> {
        use wgpu::util::DeviceExt;
        let err = |msg: String| BackendError::Other(msg);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let size = window.inner_size();
        let surface = instance
            .create_surface(window)
            .map_err(|e| err(format!("create surface: {e}")))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| err("request adapter: 无兼容适配器".into()))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("yuris-render device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
            }, None))
            .map_err(|e| err(format!("request device: {e}")))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = winit::dpi::PhysicalSize {
            width: size.width.max(1),
            height: size.height.max(1),
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuris-render shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniform layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    // 每图层各自建 bind group 且在 BufferBinding 里给静态
                    // offset(i*256),故此处不声明 dynamic(声明了动态却以
                    // `set_bind_group(.., &[])` 绘制会被 wgpu 拒绝)。
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(80),
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yuris-render pipeline layout"),
            bind_group_layouts: &[&uniform_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuris-render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("yuris-render sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuris-render uniforms"),
            size: UNIFORM_STRIDE * MAX_LAYERS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 单位方块顶点(位置 0..1,uv 0..1;两三角形)
        const QUAD: &[f32] = &[
            0.0, 0.0, 0.0, 0.0, //
            1.0, 0.0, 1.0, 0.0, //
            0.0, 1.0, 0.0, 1.0, //
            0.0, 1.0, 0.0, 1.0, //
            1.0, 0.0, 1.0, 0.0, //
            1.0, 1.0, 1.0, 1.0,
        ];
        let quad_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("yuris-render quad"),
            contents: bytemuck::cast_slice(QUAD),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            uniform_layout,
            texture_layout,
            sampler,
            uniform_buf,
            quad_buf,
            frame_layers: Vec::new(),
            images: HashMap::new(),
            surface_size: (size.width, size.height),
        })
    }

    /// 窗口尺寸变化(surface 重配置)。
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface_size = (width, height);
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// 已加载图像的原始尺寸(居中/布局计算用;未加载 → None)。
    /// 当前 surface 像素尺寸(播放器光标换算用)。
    pub fn surface_size(&self) -> (u32, u32) {
        self.surface_size
    }

    pub fn image_size(&self, id: u64) -> Option<(u32, u32)> {
        self.images.get(&id).map(|i| (i.width, i.height))
    }

    /// 便捷入口:渲染整个 Scene(z 升序合成 + present)。
    pub fn render(&mut self, scene: &Scene) -> Result<()> {
        self.begin_frame()?;
        for layer in scene.layers.iter().filter(|l| l.visible) {
            self.draw_layer(layer)?;
        }
        if let Some(text) = &scene.text {
            self.draw_text(text)?;
        }
        self.end_frame()
    }

    /// 当前逻辑坐标 → NDC 的 letterbox 映射矩阵(列主序)。
    ///
    /// 引擎 1920×1080 逻辑区等比缩放到视口并居中(信箱留黑边)。
    fn logical_to_ndc(&self) -> [[f32; 4]; 4] {
        let (w, h) = self.surface_size;
        letterbox_matrix(w as f32, h as f32)
    }
}

/// 逻辑坐标(1920×1080)→ NDC 的 letterbox 映射矩阵(列主序;纯函数可单测)。
///
/// **各向同性**:像素空间缩放因子对 x/y 相同(`scale = min(w/1920, h/1080)`),
/// 窗口任意改比例只会改变信箱黑边,不会拉伸素材(实机「拉伸」排查锚点)。
pub fn letterbox_matrix(w: f32, h: f32) -> [[f32; 4]; 4] {
    let scale = (w / LOGICAL_W).min(h / LOGICAL_H);
    let sx = 2.0 * scale / w;
    let sy = 2.0 * scale / h;
    let off_x = -1.0 + (w - LOGICAL_W * scale) / w;
    let off_y = -1.0 + (h - LOGICAL_H * scale) / h;
    // 列主序;Y 翻转(逻辑 Y 向下,NDC Y 向上)
    [
        [sx, 0.0, 0.0, 0.0],
        [0.0, -sy, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [off_x, -off_y, 0.0, 1.0],
    ]
}

#[cfg(test)]
mod letterbox_tests {
    use super::*;

    /// 应用列主序矩阵到 (x,y)。
    fn apply(m: &[[f32; 4]; 4], x: f32, y: f32) -> (f32, f32) {
        (
            m[0][0] * x + m[3][0],
            m[1][1] * y + m[3][1],
        )
    }

    #[test]
    fn exact_fit_and_offsets() {
        // 16:9 精确匹配
        let m = letterbox_matrix(2560.0, 1440.0);
        assert!((apply(&m, 0.0, 0.0).0 + 1.0).abs() < 1e-4);
        assert!((apply(&m, 0.0, 0.0).1 - 1.0).abs() < 1e-4);
        assert!((apply(&m, 1920.0, 1080.0).0 - 1.0).abs() < 1e-4);
        assert!((apply(&m, 1920.0, 1080.0).1 + 1.0).abs() < 1e-4);
        // 4:3 窗口 → 上下黑边 0.25
        let m = letterbox_matrix(1920.0, 1440.0);
        assert!((apply(&m, 0.0, 0.0).1 - 0.75).abs() < 1e-4);
        assert!((apply(&m, 1920.0, 1080.0).1 + 0.75).abs() < 1e-4);
        assert!((apply(&m, 0.0, 0.0).0 + 1.0).abs() < 1e-4);
        // 竖窗 → 左右黑边
        let m = letterbox_matrix(1080.0, 1440.0);
        assert!((apply(&m, 0.0, 0.0).0 + 1.0).abs() < 1e-4);
        assert!((apply(&m, 1920.0, 1080.0).0 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn pixel_space_is_isotropic() {
        // 任意窗口比例:逻辑 (dx,dy) 在像素空间的缩放因子相同(不拉伸)
        for (w, h) in [(1280.0, 720.0), (1920.0, 1440.0), (1080.0, 1440.0), (2560.0, 1080.0)] {
            let m = letterbox_matrix(w, h);
            let (x0, y0) = apply(&m, 0.0, 0.0);
            let (x1, y1) = apply(&m, 100.0, 0.0);
            let (x2, y2) = apply(&m, 0.0, 100.0);
            let px_x = (x1 - x0) * w / 2.0;
            let px_y = (y2 - y0) * h / 2.0;
            // Y 翻转 → 比较幅值
            assert!(
                (px_x.abs() - px_y.abs()).abs() < 1e-2,
                "w={w} h={h} px_x={px_x} px_y={px_y}"
            );
        }
    }
}

impl GraphicsBackend for WgpuBackend {
    fn begin_frame(&mut self) -> Result<()> {
        self.frame_layers.clear();
        Ok(())
    }

    fn draw_layer(&mut self, layer: &Layer) -> Result<()> {
        if self.frame_layers.len() >= MAX_LAYERS {
            return Err(BackendError::Other("frame layer overflow".into()));
        }
        self.frame_layers.push(layer.clone());
        Ok(())
    }

    fn draw_text(&mut self, _text: &TextLayout) -> Result<()> {
        // P8.3:SJIS 字体渲染(对照 YSTCH.DLL;换行细节截图对拍)。
        // 首像素里程碑不含文本,记录后跳过。
        tracing::debug!("draw_text: P8.3 未实现,跳过");
        Ok(())
    }

    fn end_frame(&mut self) -> Result<()> {
        let err = |msg: String| BackendError::Other(msg);
        // z 升序合成(P8.2)
        self.frame_layers.sort_by_key(|l| l.z);

        let frame = self
            .surface
            .get_current_texture()
            .map_err(|e| err(format!("get_current_texture: {e}")))?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("yuris-render encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yuris-render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.quad_buf.slice(..));

            let base = self.logical_to_ndc();
            for (i, layer) in self.frame_layers.iter().enumerate() {
                let Some(img) = layer.resource.and_then(|r| self.images.get(&r.0)) else {
                    continue; // 未加载资源:跳过(资源层 NotFound 语义)
                };
                // 图层变换:逻辑坐标 (x,y) 起,w×scale_x × h×scale_y
                let qw = img.width as f32 * layer.scale_x;
                let qh = img.height as f32 * layer.scale_y;
                let m = mul(&base, &mul(&translate(layer.x, layer.y), &scale(qw, qh)));
                let mut u = [0f32; 20];
                u[..16].copy_from_slice(&[
                    m[0][0], m[0][1], m[0][2], m[0][3], //
                    m[1][0], m[1][1], m[1][2], m[1][3], //
                    m[2][0], m[2][1], m[2][2], m[2][3], //
                    m[3][0], m[3][1], m[3][2], m[3][3],
                ]);
                u[16..20].copy_from_slice(&[1.0, 1.0, 1.0, layer.alpha.clamp(0.0, 1.0)]);
                self.queue.write_buffer(
                    &self.uniform_buf,
                    (i as u64) * UNIFORM_STRIDE,
                    bytemuck::cast_slice(&u),
                );
                let ub = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("yuris-render uniform bind"),
                    layout: &self.uniform_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.uniform_buf,
                            offset: (i as u64) * UNIFORM_STRIDE,
                            size: wgpu::BufferSize::new(80),
                        }),
                    }],
                });
                pass.set_bind_group(0, &ub, &[]);
                pass.set_bind_group(1, &img.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }

    fn load_image(&mut self, id: ResourceId, data: &[u8]) -> Result<()> {
        let err = |msg: String| BackendError::Other(msg);
        let img = image::load_from_memory(data)
            .map_err(|e| err(format!("decode image: {e}")))?
            .to_rgba8();
        let (w, h) = img.dimensions();
        self.load_image_rgba(id, &img, w, h)
    }

}

/// 列主序矩阵乘法。
fn mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0f32; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            out[c][r] = a[0][r] * b[c][0]
                + a[1][r] * b[c][1]
                + a[2][r] * b[c][2]
                + a[3][r] * b[c][3];
        }
    }
    out
}

/// 平移矩阵(列主序)。
fn translate(x: f32, y: f32) -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [x, y, 0.0, 1.0],
    ]
}

/// 缩放矩阵(列主序)。
fn scale(sx: f32, sy: f32) -> [[f32; 4]; 4] {
    [
        [sx, 0.0, 0.0, 0.0],
        [0.0, sy, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

impl WgpuBackend {
    /// 原始 RGBA 上传(P7.3 台词纹理等运行期生成图;非编码数据)。
    pub fn load_image_rgba(
        &mut self,
        id: ResourceId,
        rgba: &[u8],
        w: u32,
        h: u32,
    ) -> Result<()> {

        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yuris-render texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba[..],
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yuris-render texture bind"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.images.insert(
            id.0,
            GpuImage {
                bind_group,
                width: w,
                height: h,
            },
        );
        Ok(())
    }
}
