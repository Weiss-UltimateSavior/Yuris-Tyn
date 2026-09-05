//! yuris-scene
//!
//! L5:Scene / Layer / Animation **纯状态**。不触碰任何平台 API,
//! 不依赖 yuris-runtime(依赖方向见 `docs/02-workspace-design.md` §3.8)。
//!
//! P3 起步(成果 48):落地 [`Scene`] / [`Layer`] / [`TextLayout`] /
//! [`ResourceId`] 数据类型。字段语义分两级:
//! - 结构本身 = 实现选择(渲染对象的容器);
//! - 字段到引擎行为的对应关系 = 映射草案,**标注等级**见
//!   `yuris-vm::bridge`(`Cg` 事件的槽位映射)。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};

/// crate 版本(与 workspace 同步)
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 资源句柄:图像/音频等已加载资源的稳定标识。
///
/// 由桥接层从引擎侧标识(如 CG id 字符串)派生;派生策略为实现选择
/// (如 FNV-1a 哈希),非引擎行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceId(pub u64);

/// 一个可渲染图层(P3 映射草案;字段与引擎槽位的对应关系见桥接层标注)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// 图层标识(桥接层由 CG id 派生;同 id 重复出现 = 更新而非新建)。
    pub id: u64,
    /// z 序(引擎侧 Z 槽;草层级:Likely)。
    pub z: i32,
    /// 可见性。
    pub visible: bool,
    /// X 坐标(引擎 SX 槽;Likely)。
    pub x: f32,
    /// Y 坐标(引擎 SY 槽;Likely)。
    pub y: f32,
    /// X 缩放(引擎侧对应槽未定性;占位)。
    pub scale_x: f32,
    /// Y 缩放(占位)。
    pub scale_y: f32,
    /// 不透明度 0.0-1.0(引擎侧对应槽未定性;占位)。
    pub alpha: f32,
    /// 旋转角(引擎 RZ 槽;Likely)。
    pub rotation: f32,
    /// 关联资源(未加载时 None)。
    pub resource: Option<ResourceId>,
}

impl Layer {
    /// 以默认变换创建图层(x/y/z/资源由调用方给定)。
    pub fn new(id: u64, z: i32, x: f32, y: f32, resource: Option<ResourceId>) -> Self {
        Self {
            id,
            z,
            visible: true,
            x,
            y,
            scale_x: 1.0,
            scale_y: 1.0,
            alpha: 1.0,
            rotation: 0.0,
            resource,
        }
    }
}

/// 一帧文本布局(TEXT 命令的落点;SJIS 原文由上层解码)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextLayout {
    /// SJIS 原始字节(TEXT 事件原样透传,解码交给字体后端)。
    pub sjis: Vec<u8>,
    /// 是否清除既有文本(TEXT 的 let/clear 旗标族;Likely)。
    pub clear: bool,
}

/// 场景快照:图层的有序集合 + 当前文本。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scene {
    /// 图层集合(绘制顺序 = Vec 顺序;z 排序策略归后端)。
    pub layers: Vec<Layer>,
    /// 当前帧文本(无则 None)。
    pub text: Option<TextLayout>,
}

impl Scene {
    /// 按 id upsert 图层:存在则更新字段,不存在则追加。
    pub fn upsert_layer(&mut self, layer: Layer) {
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == layer.id) {
            *l = layer;
        } else {
            self.layers.push(layer);
        }
    }

    /// 按 id 隐藏图层(找不到则无操作)。
    pub fn hide_layer(&mut self, id: u64) {
        if let Some(l) = self.layers.iter_mut().find(|l| l.id == id) {
            l.visible = false;
        }
    }
}
