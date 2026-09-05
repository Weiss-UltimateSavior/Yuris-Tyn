//! yuris-runtime
//! L4:RuntimeApi / GraphicsBackend / AudioBackend / InputBackend / StorageBackend trait。Kernel 与平台之间的唯一边界。
//!
//! P3 起步(成果 48):落地 trait 定义 + [`NullBackend`](null::NullBackend)
//! (全空,CG「不存在」语义)+ [`mock::MockBackend`](录制式 mock,供
//! Golden/空循环测试断言)。依赖方向:yuris-vm **不**依赖本 crate 的具体
//! 后端;驱动层把 VM 事件(VmEvent)翻译成本 crate 的调用。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::Instant;

use thiserror::Error;
use yuris_scene::{Layer, ResourceId, TextLayout};

/// crate 版本(与 workspace 同步)
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 后端错误(L4 边界统一错误)。
#[derive(Debug, Error)]
pub enum BackendError {
    /// 资源不存在(如 CG id 未加载;NullBackend 恒返回此错 = 引擎「不存在」语义)。
    #[error("resource not found: {0:?}")]
    ResourceNotFound(ResourceId),
    /// 后端内部错误。
    #[error("backend error: {0}")]
    Other(String),
}

/// 后端结果。
pub type Result<T> = std::result::Result<T, BackendError>;

/// 图形后端(每帧:begin_frame → draw_* → end_frame)。
pub trait GraphicsBackend {
    /// 帧开始。
    fn begin_frame(&mut self) -> Result<()>;
    /// 绘制一个图层(按 Scene 顺序逐层调用;z 排序策略归后端)。
    fn draw_layer(&mut self, layer: &Layer) -> Result<()>;
    /// 绘制文本。
    fn draw_text(&mut self, text: &TextLayout) -> Result<()>;
    /// 帧结束(提交)。
    fn end_frame(&mut self) -> Result<()>;
    /// 加载图像资源(CG id → 数据;数据解码归实现)。
    fn load_image(&mut self, id: ResourceId, data: &[u8]) -> Result<()>;
}

/// 音频后端。
pub trait AudioBackend {
    /// 播放 BGM(id 标识)。
    fn play_bgm(&mut self, id: ResourceId) -> Result<()>;
    /// 播放音效。
    fn play_se(&mut self, id: ResourceId) -> Result<()>;
    /// 停止 BGM。
    fn stop_bgm(&mut self) -> Result<()>;
}

/// 输入后端。
pub trait InputBackend {
    /// 轮询点击(引擎文本等待语义;无点击 → None)。
    fn wait_click(&mut self) -> Option<()>;
}

/// 存储后端(存档/系统数据)。
pub trait StorageBackend {
    /// 读存档槽。
    fn read_slot(&self, slot: u32) -> Option<Vec<u8>>;
    /// 槽是否存在。
    fn exists(&self, slot: u32) -> bool;
}

/// L4 聚合入口:Kernel 持有,所有平台影响经此发出。
///
/// `emit` 为跟踪/事件总线(Trace / Golden Test 的数据源)。
pub trait RuntimeApi {
    /// 图形后端。
    fn graphics(&mut self) -> &mut dyn GraphicsBackend;
    /// 音频后端。
    fn audio(&mut self) -> &mut dyn AudioBackend;
    /// 输入后端。
    fn input(&mut self) -> &mut dyn InputBackend;
    /// 存储后端。
    fn storage(&mut self) -> &mut dyn StorageBackend;
    /// 单调时钟(帧等待换算)。
    fn clock(&self) -> Instant;
    /// 跟踪事件发射(实现选择:记录/转发/忽略)。
    fn emit(&mut self, ev: RuntimeEvent);
}

/// 跟踪事件(emit 数据源;serde 预留)。
#[derive(Debug, Clone, serde::Serialize)]
pub enum RuntimeEvent {
    /// 一帧提交。
    FrameSubmitted { index: u64 },
    /// 图层绘制。
    LayerDrawn { id: u64, resource: Option<ResourceId> },
    /// 文本绘制。
    TextDrawn { bytes: usize },
    /// 资源加载。
    ImageLoaded { id: ResourceId, bytes: usize },
}

pub mod null {
    //! //! 全空后端:任何查询返回「不存在」—— 与引擎 `CgInfo` 在无 CG 时的
    //! 「不存在 → 结果清零」语义对齐(成果 42)。

    use super::{BackendError, GraphicsBackend, InputBackend, Result, RuntimeApi, RuntimeEvent, StorageBackend};
    use std::time::Instant;
    use yuris_scene::{Layer, ResourceId, TextLayout};

    /// 全空后端(测试/无头运行)。
    #[derive(Debug, Default)]
    pub struct NullBackend;

    impl GraphicsBackend for NullBackend {
        fn begin_frame(&mut self) -> Result<()> {
            Ok(())
        }
        fn draw_layer(&mut self, _layer: &Layer) -> Result<()> {
            Ok(())
        }
        fn draw_text(&mut self, _text: &TextLayout) -> Result<()> {
            Ok(())
        }
        fn end_frame(&mut self) -> Result<()> {
            Ok(())
        }
        fn load_image(&mut self, _id: ResourceId, _data: &[u8]) -> Result<()> {
            Err(BackendError::ResourceNotFound(_id))
        }
    }

    impl super::AudioBackend for NullBackend {
        fn play_bgm(&mut self, _id: ResourceId) -> Result<()> {
            Ok(())
        }
        fn play_se(&mut self, _id: ResourceId) -> Result<()> {
            Ok(())
        }
        fn stop_bgm(&mut self) -> Result<()> {
            Ok(())
        }
    }

    impl InputBackend for NullBackend {
        fn wait_click(&mut self) -> Option<()> {
            None
        }
    }

    impl StorageBackend for NullBackend {
        fn read_slot(&self, _slot: u32) -> Option<Vec<u8>> {
            None
        }
        fn exists(&self, _slot: u32) -> bool {
            false
        }
    }

    /// NullBackend 组成的 RuntimeApi(全空内核)。
    #[derive(Debug, Default)]
    pub struct NullRuntime {
        graphics: NullBackend,
        audio: NullAudio,
        input: NullInput,
        storage: NullBackend,
    }

    // 复用同一空实现类型承担 audio/input/storage 角色
    type NullAudio = NullBackend;
    type NullInput = NullBackend;

    impl RuntimeApi for NullRuntime {
        fn graphics(&mut self) -> &mut dyn GraphicsBackend {
            &mut self.graphics
        }
        fn audio(&mut self) -> &mut dyn super::AudioBackend {
            &mut self.audio
        }
        fn input(&mut self) -> &mut dyn InputBackend {
            &mut self.input
        }
        fn storage(&mut self) -> &mut dyn StorageBackend {
            &mut self.storage
        }
        fn clock(&self) -> Instant {
            Instant::now()
        }
        fn emit(&mut self, _ev: RuntimeEvent) {}
    }
}

pub mod mock {
    //! 录制式 mock:记录每帧图层/文本/音频调用,供测试断言
    //! 「mock backend 跑通空循环」(P3 验收)。

    use std::time::Instant;

    use super::{AudioBackend, GraphicsBackend, InputBackend, Result, RuntimeApi, RuntimeEvent, StorageBackend};
    use yuris_scene::{Layer, ResourceId, TextLayout};

    /// 录制的图层快照。
    #[derive(Debug, Clone, PartialEq)]
    pub struct RecordedLayer {
        /// 图层 id。
        pub id: u64,
        /// 资源句柄。
        pub resource: Option<ResourceId>,
        /// X 坐标。
        pub x: f32,
        /// Y 坐标。
        pub y: f32,
        /// 可见性。
        pub visible: bool,
    }

    /// Mock 图形后端:记录帧与图层。
    #[derive(Debug, Default)]
    pub struct MockGraphics {
        /// 已提交帧数。
        pub frames: u64,
        /// 当前帧(未提交)绘制的图层。
        pub current: Vec<RecordedLayer>,
        /// 每帧图层快照(end_frame 时归档)。
        pub frame_layers: Vec<Vec<RecordedLayer>>,
        /// 已加载资源。
        pub loaded: Vec<ResourceId>,
    }

    impl GraphicsBackend for MockGraphics {
        fn begin_frame(&mut self) -> Result<()> {
            self.current.clear();
            Ok(())
        }
        fn draw_layer(&mut self, layer: &Layer) -> Result<()> {
            self.current.push(RecordedLayer {
                id: layer.id,
                resource: layer.resource,
                x: layer.x,
                y: layer.y,
                visible: layer.visible,
            });
            Ok(())
        }
        fn draw_text(&mut self, _text: &TextLayout) -> Result<()> {
            Ok(())
        }
        fn end_frame(&mut self) -> Result<()> {
            self.frames += 1;
            self.frame_layers.push(self.current.clone());
            Ok(())
        }
        fn load_image(&mut self, id: ResourceId, _data: &[u8]) -> Result<()> {
            self.loaded.push(id);
            Ok(())
        }
    }

    /// Mock 音频:记录播放调用。
    #[derive(Debug, Default)]
    pub struct MockAudio {
        /// BGM 播放记录。
        pub bgm: Vec<ResourceId>,
        /// 音效播放记录。
        pub se: Vec<ResourceId>,
    }

    impl AudioBackend for MockAudio {
        fn play_bgm(&mut self, id: ResourceId) -> Result<()> {
            self.bgm.push(id);
            Ok(())
        }
        fn play_se(&mut self, id: ResourceId) -> Result<()> {
            self.se.push(id);
            Ok(())
        }
        fn stop_bgm(&mut self) -> Result<()> {
            Ok(())
        }
    }

    /// Mock 输入:恒有点击(推进文本等待)。
    #[derive(Debug, Default)]
    pub struct MockInput {
        /// 被查询次数。
        pub polls: u64,
    }

    impl InputBackend for MockInput {
        fn wait_click(&mut self) -> Option<()> {
            self.polls += 1;
            Some(())
        }
    }

    /// Mock 存储:空。
    #[derive(Debug, Default)]
    pub struct MockStorage;

    impl StorageBackend for MockStorage {
        fn read_slot(&self, _slot: u32) -> Option<Vec<u8>> {
            None
        }
        fn exists(&self, _slot: u32) -> bool {
            false
        }
    }

    /// Mock RuntimeApi 聚合(全部可检视)。
    #[derive(Debug, Default)]
    pub struct MockRuntime {
        /// 图形。
        pub graphics: MockGraphics,
        /// 音频。
        pub audio: MockAudio,
        /// 输入。
        pub input: MockInput,
        /// 存储。
        pub storage: MockStorage,
        /// 发射的事件。
        pub events: Vec<RuntimeEvent>,
        frame_counter: u64,
    }

    impl RuntimeApi for MockRuntime {
        fn graphics(&mut self) -> &mut dyn GraphicsBackend {
            &mut self.graphics
        }
        fn audio(&mut self) -> &mut dyn AudioBackend {
            &mut self.audio
        }
        fn input(&mut self) -> &mut dyn InputBackend {
            &mut self.input
        }
        fn storage(&mut self) -> &mut dyn StorageBackend {
            &mut self.storage
        }
        fn clock(&self) -> Instant {
            Instant::now()
        }
        fn emit(&mut self, ev: RuntimeEvent) {
            if let RuntimeEvent::FrameSubmitted { index } = ev {
                self.frame_counter = index;
            }
            self.events.push(ev);
        }
    }
}
