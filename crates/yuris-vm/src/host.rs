//! 跨脚本上下文:多脚本执行宿主(P0)。
//!
//! 单脚本 [`GroupVm`](crate::GroupVm) 是引擎执行单元的核心;但真实游戏流程
//! 在 `yst%05d.ybn` 之间不断跳转(GOSUB 占语料 27065 组,最高频)。
//! 本模块提供「按脚本号取脚本上下文」的接口与实现,供 GroupVm 跨脚本切换。
//!
//! [`YpfScriptHost`] 从 REAL YPF 封包按 script_id 读 `%ysbin\yst%05d.ybn`。

use std::collections::HashMap;

use yuris_core::{Error, Result};
use yuris_format::ystb::{CommandGroup, YstbFile};

/// 一个已解析脚本的可执行上下文(组表 + 各组窗口起始下标)。
#[derive(Debug, Clone)]
pub struct ScriptCtx {
    /// 脚本号(yst%05d)。
    pub script_id: u16,
    /// 已解析的 YSTB。
    pub script: YstbFile,
    /// 命令组表(part1 区)。
    pub groups: Vec<CommandGroup>,
    /// 每组第一个窗口在 slots 区的下标(引擎 obj+0x10 记录指针数组的等价)。
    pub first_slots: Vec<usize>,
}

impl ScriptCtx {
    /// 从脚本字节解析出上下文。组模型校验失败(非 v555 形态)→ `Err`。
    pub fn parse(script_id: u16, script: YstbFile) -> Result<Self> {
        let groups = script.groups()?;
        let first_slots = script.group_first_slots(&groups);
        Ok(Self {
            script_id,
            script,
            groups,
            first_slots,
        })
    }
}

/// 脚本加载抽象:按脚本号取 [`ScriptCtx`]。
///
/// 实现方负责从 YPF 封包读取并解析(如 `YpfScriptHost`),可缓存。
pub trait ScriptHost {
    /// 取脚本号 `id` 的上下文(惰性加载 + 缓存由实现方决定)。
    fn load(&mut self, id: u16) -> Result<ScriptCtx>;

    /// 全部可用脚本号(启动链全量声明消费用;默认空)。
    fn script_ids(&mut self) -> Result<Vec<u16>> {
        Ok(Vec::new())
    }
}

/// 基于内存脚本表(测试/少量脚本用)的宿主。
///
/// 由外部预注入 `id → YstbFile`,按需 parse + 缓存。
#[derive(Debug, Clone, Default)]
pub struct InMemoryHost {
    scripts: HashMap<u16, YstbFile>,
}

impl InMemoryHost {
    /// 空宿主。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注入一个脚本。
    pub fn insert(&mut self, id: u16, script: YstbFile) -> &mut Self {
        self.scripts.insert(id, script);
        self
    }
}

impl ScriptHost for InMemoryHost {
    fn load(&mut self, id: u16) -> Result<ScriptCtx> {
        let script = self
            .scripts
            .get(&id)
            .ok_or_else(|| Error::format(format!("script {id} not injected")))?
            .clone();
        ScriptCtx::parse(id, script)
    }
}


/// 基于 YPF 封包的宿主(真实游戏用):**拥有** [`YpfArchive`],
/// 按 script_id 拼 `$ysbin\yst%05d.ybn` 从封包读取并解析,结果缓存(惰性加载)。
///
/// 拥有所有权使其为 `'static`,可放进 [`Box<dyn ScriptHost>`](crate::ScriptHost)。
pub struct YpfScriptHost {
    /// 脚本封包(如 bn.ypf,由 [`YpfArchive::from_bytes`] 从内存读入拥有)。
    ypf: yuris_format::ypf::YpfArchive,
    /// 脚本区 XOR 密钥(4 字节循环,样本 `2b904f93`)。
    key: [u8; 4],
    /// 缓存(script_id → 已解析上下文)。
    cache: HashMap<u16, ScriptCtx>,
}

impl YpfScriptHost {
    /// 从封包字节构建宿主(装载 ypf 全量到内存并解析索引)。
    pub fn from_ypf_bytes(bytes: Vec<u8>, name_key: u8, key: [u8; 4]) -> Result<Self> {
        let ypf = yuris_format::ypf::YpfArchive::from_bytes(bytes, name_key)?;
        Ok(Self { ypf, key, cache: HashMap::new() })
    }

    /// 读封包内任一条目(启动链读 YSLB/YSVR 等非脚本数据用)。
    pub fn read_entry(&self, path: &str) -> Result<Vec<u8>> {
        self.ypf.read(path)
    }
}

impl ScriptHost for YpfScriptHost {
    fn load(&mut self, id: u16) -> Result<ScriptCtx> {
        if let Some(ctx) = self.cache.get(&id) {
            return Ok(ctx.clone());
        }
        let path = format!("$ysbin\\yst{id:05}.ybn");
        let bytes = self.ypf.read(&path)?;
        let script = yuris_format::ystb::YstbFile::from_bytes(&bytes, self.key)?;
        let ctx = ScriptCtx::parse(id, script)?;
        self.cache.insert(id, ctx.clone());
        Ok(ctx)
    }

    fn script_ids(&mut self) -> Result<Vec<u16>> {
        let mut ids: Vec<u16> = self
            .ypf
            .entries()
            .iter()
            .filter_map(|e| {
                e.name.strip_prefix("$ysbin\\yst").and_then(|s| {
                    s.strip_suffix(".ybn").and_then(|n| n.parse::<u16>().ok())
                })
            })
            .collect();
        ids.sort_unstable();
        Ok(ids)
    }
}

/// 游戏**虚拟文件系统**存在性索引(P5.2,引擎 FILEINFO EXIST 语义)。
///
/// 引擎的变量 FS = 游戏目录下的 `pac\*.ypf` 封包 + 松散文件;脚本侧
/// FILEINFO(0x15) 的 EXIST 查询即在此 FS 上判定(实证:s207 es.R18Check
/// 查 `cg/thumb_cg/A_HAN_2002_a.png`,引擎返回 1)。
///
/// 只解析各封包**索引区**(`YpfIndex`,数据区不触碰),松散文件按查询时
/// `Path::exists` 判定。
#[derive(Debug, Default)]
pub struct PacFileIndex {
    names: std::collections::HashSet<Vec<u8>>,
    roots: Vec<std::path::PathBuf>,
    /// 解析失败被跳过的封包(如头加密的 op.ypf —— 密钥派生属 P6 资源层)。
    pub skipped: Vec<String>,
    /// P1 收尾:保留各封包的解析索引(条目偏移/类型码),供
    /// [`Self::image_dims`](PacFileIndex::image_dims) 按需读图像头
    /// (CGINFO 槽13/14 = 图像真实宽/高,引擎 watch oracle 实证)。
    packs: Vec<(std::path::PathBuf, yuris_format::ypf::YpfIndex)>,
    /// (pack, entry) 下标查找:全名与剥根名并存,首命中。
    entry_lookup: std::collections::HashMap<Vec<u8>, (usize, usize)>,
}

impl PacFileIndex {
    /// 扫描游戏目录:`<game>\pac\*.ypf` 与 `<game>\*.ypf` 的索引并入;
    /// `<game>`、`<game>\pac`、`<game>\save`(存档,引擎 VFS 原生前缀之一,
    /// s25/s35 的 *.sd 存在性查询在此命中 —— 成果 59e)作为松散文件查找根。
    /// 传 `pac` 目录本身亦可(自动上溯一级作为游戏根)。
    pub fn scan_game_dir(dir: &std::path::Path, name_key: u8) -> Result<Self> {
        let (game_root, pac_dir) = if dir.join("pac").is_dir() {
            (dir.to_path_buf(), dir.join("pac"))
        } else {
            (
                dir.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| dir.to_path_buf()),
                dir.to_path_buf(),
            )
        };
        let mut idx = Self::default();
        for root in [game_root.clone(), pac_dir.clone()] {
            if let Ok(rd) = std::fs::read_dir(&root) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|s| s.to_str()) == Some("ypf") {
                        match yuris_format::ypf::YpfIndex::from_path(&p, name_key) {
                            Ok(y) => {
                                let pi = idx.packs.len();
                                for (ei, e) in y.entries.iter().enumerate() {
                                    idx.entry_lookup
                                        .entry(e.name.clone())
                                        .or_insert((pi, ei));
                                    if e.name.len() > 1 {
                                        idx.entry_lookup
                                            .entry(e.name[1..].to_vec())
                                            .or_insert((pi, ei));
                                    }
                                }
                                idx.packs.push((p.clone(), y));
                                // names 并入(保留原行为:纯存在性判定)
                                for e in &idx.packs[pi].1.entries {
                                    idx.names.insert(e.name.clone());
                                }
                            }
                            Err(e) => {
                                // 头加密等不可解析包:跳过(不阻断存在性检查),
                                // 但必须留痕 —— 静默跳包会让整包资源「无声消失」
                                // (NEKO-NIN exHeart se.ypf 实证)。
                                eprintln!(
                                    "[pac] 跳过不可解析包 {}: {e}",
                                    p.file_name()
                                        .map(|s| s.to_string_lossy())
                                        .unwrap_or_default()
                                );
                                idx.skipped.push(
                                    p.file_name()
                                        .map(|s| s.to_string_lossy().into_owned())
                                        .unwrap_or_default(),
                                );
                            }
                        }
                    }
                }
            }
        }
        idx.roots = vec![game_root.clone(), pac_dir, game_root.join("save")];
        Ok(idx)
    }

    /// 注入虚拟条目(仅存在性判定)。
    ///
    /// 用途:复现 oracle 采集时的文件系统输入 —— 引擎 trace 时刻游戏目录
    /// 存在松散的 R18 标记文件(`cg/thumb_cg/A_HAN_2002_a.png`,引擎
    /// FindFirstFileA/VFS 命中 → EXIST=1),现安装包内无此文件。重放对拍
    /// 须复现相同输入;内容从不读取,无版权物引入。
    pub fn add_virtual(&mut self, path: &str) {
        self.names.insert(path.as_bytes().to_vec());
        self.names.insert(path.replace('/', "\\").into_bytes());
    }

    /// 路径是否存在(封包条目精确/反斜杠归一,或松散文件)。
    pub fn exists(&self, path: &str) -> bool {
        let direct = path.as_bytes().to_vec();
        let back = path.replace('/', "\\").into_bytes();
        if self.names.contains(&direct) || self.names.contains(&back) {
            return true;
        }
        self.roots
            .iter()
            .any(|r| r.join(path.replace('\\', "/")).exists())
    }

    /// 读松散文件内容(LOAD/YSSD 装载用)。
    ///
    /// 只查 `roots`(游戏根/pac/save);封包内条目不在此读(YSSD 存档
    /// 恒为 save/ 下松散文件,引擎 VFS 原生前缀,成果 59e)。分隔符归一
    /// 与 [`Self::exists`] 一致。
    pub fn read_loose(&self, path: &str) -> Option<Vec<u8>> {
        let norm = path.replace('\\', "/");
        self.roots
            .iter()
            .map(|r| r.join(&norm))
            .find_map(|p| std::fs::read(p).ok())
    }

    /// 读图像条目头部,返回 (宽, 高)(P1 收尾,CGINFO 槽13/14 真值源)。
    ///
    /// 引擎 watch oracle 实证:CGINFO SX/SY 应答 = CG 装载图像的真实尺寸
    /// (script9 g1089 写 @1705:occ1 = 1.0 = `tip_meswindow.png`(1×1)、
    /// occ2 = 1350.0 = `tip_meswindow_txspace.png`(1350×200);两图均经
    /// 包扫描确认存在)。仅支持 **stored PNG**(se 型类型码 0xCB,zlib
    /// 条目不触碰数据区 —— CG 资源恒 stored,成果 63);未命中/不支持
    /// → `None`(调用方以 0 应答,偏离引擎处逐项定性)。
    ///
    /// 路径形态勘误:脚本 FILE 参数**无扩展名**且用 `/`(引擎 VFS 自行
    /// 补扩展名;实测 `cgsys/main/button/type1/tip_meswindow`),故候选名
    /// = 原/归一路径 + {.png,.jpg,.bmp,.gif}(YSCM 图像类型族)。
    pub fn image_dims(&self, path: &str) -> Option<(i64, i64)> {
        use std::io::{Read, Seek, SeekFrom};
        let (pack_path, e) = self.resolve_entry(path, true)?;
        if e.flag != 0xCB {
            // 0xCB = PNG 类型码;bn 型 zlib(stored 0/1)与非 PNG 类型不支持
            return None;
        }
        let mut f = std::fs::File::open(pack_path).ok()?;
        f.seek(SeekFrom::Start(e.offset as u64)).ok()?;
        let mut head = [0u8; 24];
        f.read_exact(&mut head).ok()?;
        if head[..8] != *b"\x89PNG\r\n\x1a\n" {
            return None;
        }
        let w = u32::from_be_bytes(head[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(head[20..24].try_into().ok()?);
        Some((w as i64, h as i64))
    }

    /// 解析 FILE 参数 → (包路径, 条目)。候选 = 原/归一路径 +
    /// {.png,.jpg,.bmp,.gif}(`image_dims` 文档),再退 basename 前缀模糊。
    /// `png_only` = 限定 stored PNG(尺寸查询用);内容读取放宽。
    fn resolve_entry(&self, path: &str, _png_only: bool) -> Option<(&std::path::PathBuf, &yuris_format::ypf::YpfEntryInfo)> {
        let norm = path.replace('/', "\\");
        let mut cands: Vec<Vec<u8>> = vec![
            path.as_bytes().to_vec(),
            norm.as_bytes().to_vec(),
        ];
        for base in [path, norm.as_str()] {
            for ext in [".png", ".jpg", ".bmp", ".gif"] {
                cands.push(format!("{base}{ext}").into_bytes());
            }
        }
        let &(pi, ei) = cands
            .iter()
            .filter_map(|k| self.entry_lookup.get(k))
            .next()
            .or_else(|| self.fuzzy_lookup(&norm))?;
        let (pack_path, index) = &self.packs[pi];
        Some((pack_path, &index.entries[ei]))
    }

    /// 读 FILE 参数指向的图像**内容**(播放器渲染用;P8 集成)。
    /// 仅 stored(se 型)条目直接 seek 读;zlib 条目返回 None
    /// (内容读取走 `yuris_resource::ResourceStack`,此处不重复解压栈)。
    pub fn read_image_bytes(&self, path: &str) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let (pack_path, e) = self.resolve_entry(path, false)?;
        if e.flag != 0xCB {
            return None;
        }
        let mut f = std::fs::File::open(pack_path).ok()?;
        f.seek(SeekFrom::Start(e.offset as u64)).ok()?;
        let mut raw = vec![0u8; e.compressed_len as usize];
        f.read_exact(&mut raw).ok()?;
        Some(raw)
    }

    /// scenario 资源名解析(播放器;P7.3):
    /// 1. 直名/归一/补扩展名(`resolve_entry`);
    /// 2. `cg\{path}`(\S 的 path 相对 cg\);
    /// 3. `cgg\{name}.png` / `cg\item\{name}.png`(\BG/\S 常用族);
    /// 4. `cg\stand\*\*\*\{name 小写}.png`(立绘分层组合的整图近似,
    ///    多候选取首个 —— m_040/m_066 等定位组,Likely)。
    pub fn read_cg_bytes(&self, path_or_name: &str) -> Option<Vec<u8>> {
        let norm = path_or_name.replace('/', "\\");
        let trimmed = norm.trim();
        let cands: Vec<String> = vec![
            trimmed.to_string(),
            format!("cg\\{}", trimmed),
            format!("cg\\bg\\{}.png", trimmed),
            format!("cg\\item\\{}.png", trimmed),
            format!("{}.png", trimmed),
        ];
        for c in &cands {
            if let Some(b) = self.read_image_bytes(c) {
                return Some(b);
            }
        }
        // 立绘模糊:cg\stand\ 任意定位组,文件名(去扩展)= name 小写
        let want = trimmed.to_ascii_lowercase();
        let stand_prefix = format!("cg\\stand\\");
        for k in self.entry_lookup.keys() {
            let key = String::from_utf8_lossy(k);
            if !key.starts_with(&stand_prefix) {
                continue;
            }
            let fname = key.rsplit('\\').next().unwrap_or(&key);
            let noext = fname.strip_suffix(".png").unwrap_or(fname);
            if noext.eq_ignore_ascii_case(&want) {
                if let Some(b) = self.read_image_bytes(&key) {
                    return Some(b);
                }
            }
        }
        None
    }

    /// 条目键遍历(音频名解析用;含全名与剥根名两套键)。
    pub fn keys(&self) -> impl Iterator<Item = &Vec<u8>> {
        self.entry_lookup.keys()
    }

    /// 读 stored 条目内容(se 型明文 OGG/PNG/WAV;精确键;P9.1 音频用)。
    pub fn read_stored_bytes(&self, path: &str) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let (pack_path, e) = self.resolve_entry(path, false)?;
        if e.flag != 0xCB && e.flag != 0xCF && e.flag != 0xCC {
            return None; // 仅 stored PNG/OGG/WAV
        }
        let mut f = std::fs::File::open(pack_path).ok()?;
        f.seek(SeekFrom::Start(e.offset as u64)).ok()?;
        let mut raw = vec![0u8; e.compressed_len as usize];
        f.read_exact(&mut raw).ok()?;
        Some(raw)
    }

    /// 模糊解析(实验性):FILE 参数与物理条目名不一致时(引擎 VFS 的
    /// 逻辑名映射),在条目中找「同目录下、文件名(去扩展)以参数末段为
    /// 前缀」的候选(实证:`main/button/btn_skip_bt4` →
    /// `main/button/type1/btn_skip_bt4n.png`;type1/ 子目录为皮肤变体,
    /// 候选唯一时取之)。等级 Likely。
    fn fuzzy_lookup(&self, norm: &str) -> Option<&(usize, usize)> {
        let base = norm.rsplit('\\').next()?;
        let dir = match norm.rfind('\\') {
            Some(i) => &norm[..=i],
            None => "",
        };
        self.entry_lookup
            .iter()
            .filter(|(k, _)| {
                let key = String::from_utf8_lossy(k);
                let Some(rest) = key.strip_prefix(dir) else {
                    return false;
                };
                // 末段文件名(去扩展)以 base 为前缀(type1/ 等皮肤子目录可差)
                let fname = rest.rsplit('\\').next().unwrap_or(rest);
                let noext = fname
                    .strip_suffix(".png")
                    .or_else(|| fname.strip_suffix(".jpg"))
                    .or_else(|| fname.strip_suffix(".bmp"))
                    .or_else(|| fname.strip_suffix(".gif"))
                    .unwrap_or(fname);
                noext.starts_with(base)
            })
            .map(|(_, v)| v)
            .next()
    }
}
