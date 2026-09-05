//! yuris-resource
//!
//! 资源管理器:多包挂载 + 免封包(松散文件)优先级 + seek 式随机读取 +
//! PNG/OGG 解码链路(P6.3,成果 64)。
//!
//! 引擎语义对齐:
//! - **松散文件优先于封包**(FILEPRIORITY 机制,Confirmed —— YSCM
//!   FILEPRIORITY\* 键族 + 免封包文章,成果 63 §4);
//! - **包间同名条目**:后挂载者优先(Likely —— 工程默认:更新包覆盖
//!   基础包;`mount_game_dir` 按文件名排序挂载使 update\* 自然最后。
//!   引擎挂载顺序真值 Unknown,留待 P8 截图对拍校准,API 显式可控);
//! - 图像 = 标准 PNG / 音频 = 标准 OGG(明文 stored,成果 63 魔数实证),
//!   image/symphonia 直读,无需引擎 DLL(YSPNG 等)逆向。
//!
//! > 所有未确认的行为一律返回明确错误,**禁止猜测**。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use yuris_core::Error;
use yuris_format::ypf::YpfReader;

/// crate 版本（与 workspace 同步）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 资源读取错误(`NotFound` = 引擎「资源不存在」语义,对齐
/// `yuris_runtime::BackendError::ResourceNotFound`)。
#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    /// 路径在任何挂载面(松散根/封包)都不存在。
    #[error("resource not found: {0}")]
    NotFound(String),
    /// 封包/解码/IO 层错误(透传 `yuris_core::Error`)。
    #[error("resource io/format: {0}")]
    Inner(#[from] Error),
    /// 图像解码失败(image crate)。
    #[error("image decode: {0}")]
    Image(String),
    /// 音频解码失败(symphonia)。
    #[error("audio decode: {0}")]
    Audio(String),
}

/// 多包资源栈:松散根 + 已挂载封包(后挂载优先)。
///
/// cg.ypf(827MB)级封包走 [`YpfReader`] seek 式访问,内存只保留索引。
pub struct ResourceStack {
    packs: Vec<YpfReader>,
    /// 松散文件查找根(优先于封包;引擎 FILEPRIORITY,Confirmed)。
    loose_roots: Vec<PathBuf>,
    name_key: u8,
}

impl Default for ResourceStack {
    fn default() -> Self {
        Self::new(0xC9)
    }
}

impl ResourceStack {
    /// 空栈。`name_key` 为 YPF 条目名 XOR key(样本 `0xC9`)。
    pub fn new(name_key: u8) -> Self {
        Self {
            packs: Vec::new(),
            loose_roots: Vec::new(),
            name_key,
        }
    }

    /// 追加一个松散文件查找根(先加者优先)。
    pub fn add_loose_root(&mut self, dir: impl Into<PathBuf>) -> &mut Self {
        self.loose_roots.push(dir.into());
        self
    }

    /// 挂载一个 YPF 封包(后挂载者优先于先挂载 —— 见模块文档等级说明)。
    ///
    /// 非 YPF 文件(如 op.ypf = ASF/WMV)→ `Err`(BadMagic 透传)。
    pub fn mount(&mut self, ypf_path: impl AsRef<Path>) -> std::result::Result<(), ResourceError> {
        let reader = YpfReader::open(ypf_path.as_ref(), self.name_key)?;
        self.packs.push(reader);
        Ok(())
    }

    /// 挂载游戏目录:`<game>/pac/*.ypf` + `<game>/*.ypf`(按文件名排序
    /// 挂载,读取 LIFO ⇒ update\* 自然覆盖基础包,Likely 工程默认)。
    ///
    /// 非 YPF 文件跳过(如 op.ypf = ASF/WMV,记录 tracing;不阻断)。
    /// 松散根 = 游戏根 / pac / save(引擎 VFS 原生前缀,成果 59e)。
    pub fn mount_game_dir(&mut self, dir: impl AsRef<Path>) -> std::result::Result<(), ResourceError> {
        let dir = dir.as_ref();
        let (game_root, pac_dir) = if dir.join("pac").is_dir() {
            (dir.to_path_buf(), dir.join("pac"))
        } else {
            (
                dir.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| dir.to_path_buf()),
                dir.to_path_buf(),
            )
        };
        let mut cands: Vec<PathBuf> = Vec::new();
        for root in [&game_root, &pac_dir] {
            if let Ok(rd) = std::fs::read_dir(root) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|s| s.to_str()) == Some("ypf") {
                        cands.push(p);
                    }
                }
            }
        }
        cands.sort();
        cands.dedup();
        for p in &cands {
            match YpfReader::open(p, self.name_key) {
                Ok(r) => self.packs.push(r),
                Err(Error::BadMagic { .. }) => {
                    tracing::info!(path = ?p, "跳过非 YPF 文件(ASF/WMV 等)");
                }
                Err(e) => return Err(e.into()),
            }
        }
        for r in [&game_root, &pac_dir, &game_root.join("save")] {
            self.loose_roots.push(r.to_path_buf());
        }
        Ok(())
    }

    /// 路径是否存在(松散根或任一封包)。
    pub fn exists(&mut self, path: &str) -> bool {
        if self.read_loose(path).is_some() {
            return true;
        }
        let back = path.replace('/', "\\");
        self.packs
            .iter()
            .any(|p| p.index().map.contains_key(back.as_bytes()))
    }

    /// 读取资源:松散根优先(Confirmed),封包 LIFO(后挂载优先)。
    ///
    /// 路径分隔符归一(`/` 与 `\` 等价)。
    pub fn read(&mut self, path: &str) -> std::result::Result<Vec<u8>, ResourceError> {
        if let Some(data) = self.read_loose(path) {
            return Ok(data);
        }
        let back = path.replace('/', "\\");
        for p in self.packs.iter_mut().rev() {
            if let Ok(data) = p.read(back.as_bytes()) {
                return Ok(data);
            }
        }
        Err(ResourceError::NotFound(path.to_string()))
    }

    /// 读取并解码为图像(标准 PNG,成果 63)。
    pub fn read_image(
        &mut self,
        path: &str,
    ) -> std::result::Result<image::DynamicImage, ResourceError> {
        let data = self.read(path)?;
        image::load_from_memory(&data).map_err(|e| ResourceError::Image(e.to_string()))
    }

    /// 读取音频(标准 OGG,成果 63)并探测编码参数,返回
    /// `(声道数, 采样率)`。symphonia probe 即验证容器/codec 可解码。
    pub fn read_audio_header(
        &mut self,
        path: &str,
    ) -> std::result::Result<(u32, u32), ResourceError> {
        let data = self.read(path)?;
        let mss = symphonia::core::io::MediaSourceStream::new(
            Box::new(std::io::Cursor::new(data)),
            Default::default(),
        );
        let probed = symphonia::default::get_probe()
            .format(&symphonia::core::probe::Hint::new(), mss, &Default::default(), &Default::default())
            .map_err(|e| ResourceError::Audio(e.to_string()))?;
        let track = probed
            .format
            .default_track()
            .ok_or_else(|| ResourceError::Audio("no default track".into()))?;
        let params = &track.codec_params;
        Ok((
            params.channels.map(|c| c.count() as u32).unwrap_or(0),
            params.sample_rate.unwrap_or(0),
        ))
    }

    /// 读松散文件(roots 顺序,分隔符归一)。
    fn read_loose(&self, path: &str) -> Option<Vec<u8>> {
        let norm = path.replace('\\', "/");
        self.loose_roots
            .iter()
            .map(|r| r.join(&norm))
            .find_map(|p| std::fs::read(p).ok())
    }
}
