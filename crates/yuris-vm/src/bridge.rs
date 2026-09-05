//! VmEvent → Scene 桥(P3 映射草案,**等级标注**)。
//!
//! 职责:把 VM 事件流翻译成 [`yuris_scene::Scene`] 状态更新;每帧边界
//! (`VmSuspend::Wait`)由驱动层调用后端绘制。依赖方向:yuris-vm →
//! yuris-scene/yuris-runtime(预埋),后端实现(yuris-render 等)→ 本 crate
//! 定义 trait,均不反向。
//!
//! ## CG 事件 → 渲染对象映射草案(等级标注,成果 48)
//!
//! | VmEvent 字段 | Scene/Layer 落点 | 等级 | 依据 |
//! |---|---|---|---|
//! | `Cg.id`(槽 0 字符串) | `Layer.id` = FNV-1a(id)、`resource` = 同哈希 | Confirmed(槽=ID)/Likely(哈希派生为实现选择) | YSCM 首参数名 `ID`;真处理器 0x423864:93(`DAT_00662300[slot0]` 字符串读取) |
//! | `Cg.position` 槽 4/5/6 | `Layer.x/y/z` | Likely | 引擎槽位消费(00423864 a9/aa 族)+ 数值实测;逐槽语义未逐步验证 |
//! | `Cg.param_count` | 其余 ~55 槽(渐变/缩放/效果族) | Unknown | 处理器 22942B 未逐段定性 |
//! | `CgEnd.id` | `Layer.visible = false` | Likely | CGEND 语义(显示结束通知) |
//! | `Text.file/let/clear` | `Scene.text` | Likely | TEXT 事件透传,SJIS 解码归字体后端 |
//! | `CgInfo` 空结果 | 无后端 ⇒ CG 不存在 ⇒ 结果清零 | Confirmed | 引擎「不存在」路径(成果 42) |
//!
//! 引擎处理器勘误:cmd 0x01 CG = **0x423864**(表初始化 DAT_0078b024);
//! 0x43c984 = cmd 0x0a DIALOG(此前 PROGRESS/lib.rs 误标为 CG,已更正)。

use yuris_scene::{Layer, ResourceId, Scene};

use crate::VmEvent;

/// VmEvent → Scene 桥。持有 id → 数值 映射保证同一 CG id 稳定。
#[derive(Debug, Default)]
pub struct SceneBridge {
    scene: Scene,
    ids: HashMap<String, u64>,
}

use std::collections::HashMap;

/// FNV-1a 64 位(引擎无此哈希;实现选择:CG id 字符串 → 稳定数值 id)。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl SceneBridge {
    /// 新桥(空场景)。
    pub fn new() -> Self {
        Self::default()
    }

    /// 场景只读访问(驱动层每帧绘制时读取)。
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// 场景可变访问(特殊驱动需求)。
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// 消费一批 VM 事件(增量游标),更新场景。返回是否产生了可视变化
    /// (图层/文本变更;纯流程事件返回 false)。
    pub fn apply(&mut self, events: &[VmEvent]) -> bool {
        let mut changed = false;
        for ev in events {
            match ev {
                VmEvent::Cg { id, position, .. } => {
                    let Some(name) = id else { continue };
                    let num = self.intern(name);
                    let (x, y, z) = position.unwrap_or((0, 0, 0));
                    let layer = Layer::new(
                        num,
                        z as i32,
                        x as f32,
                        y as f32,
                        Some(ResourceId(num)),
                    );
                    self.scene.upsert_layer(layer);
                    changed = true;
                }
                VmEvent::CgEnd { id, .. } => {
                    if let Some(name) = id {
                        let num = self.intern(name);
                        self.scene.hide_layer(num);
                        changed = true;
                    }
                }
                VmEvent::Text { file, clear_flag, .. } => {
                    self.scene.text = Some(yuris_scene::TextLayout {
                        sjis: file.clone().unwrap_or_default(),
                        clear: *clear_flag,
                    });
                    changed = true;
                }
                _ => {}
            }
        }
        changed
    }

    fn intern(&mut self, name: &str) -> u64 {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = fnv1a(name.as_bytes());
        self.ids.insert(name.to_string(), id);
        id
    }
}
