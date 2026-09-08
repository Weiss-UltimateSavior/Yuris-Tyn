//! P7.3 scenario 播放器(可玩切片,M2)。
//!
//! 执行 sc.ypf 明文剧本(成果 49 语法表):驱动背景/立绘/淡入淡出/等待/
//! 双语台词(`(ID:n)\LE("…")\LT("…")`)。命令语义为**实测近似**
//! (引擎解释器派发链未逆向,逐项 Likely;不猜的部分显式跳过)。
//!
//! 资源解析:
//! - `\S(name, path, …)`:`path` 相对 `cg\`(实证 item/logo_wp);
//! - `\BG(name, …)`:`cg\bg\{name}.png`(实证 bg52);`white`/`black`
//!   等颜色名 → 纯色层;
//! - `\T(name, ms, x, y, …)`:`L_NYA_1A0100` → `cg\stand\*\*\*\{name 小写}.png`
//!   (多候选取首个;角色立绘为分层合成,此处取整图近似)。
//!
//! 台词等待:LT 行显示后**等待点击**(点击推进);\T 的 ms 为自动等待
//! (点击可跳过);\WA(ms) 纯延时。

use std::collections::HashMap;
use std::time::Instant;

use yuris_scenario::{parse_scenario_with, Element, Param, Scenario};

/// 播放层资源解析 + 渲染委托(由播放器实现)。
pub trait ScenarioHost {
    /// 显示/更新背景(淡入 ms;name 为颜色名时给 None 资源 + 颜色)。
    fn show_bg(&mut self, name: &str, fade_ms: u64, color: [u8; 3]);
    /// 显示精灵(\S):path 相对 cg\;(x,y) = 左上角(0,0 = 全屏图原位),
    /// 原生尺寸,fade_ms 淡入。
    fn show_sprite(&mut self, name: &str, path: Option<&str>, x: i64, y: i64, fade_ms: u64);
    /// 显示立绘(\T):x = 中心偏移,y = 底部偏移,fade_ms 淡入。
    fn show_tachie(&mut self, name: &str, x: i64, y: i64, fade_ms: u64);
    /// 处置精灵(淡出 ms)。
    fn hide_sprite(&mut self, name: &str, fade_ms: u64);
    /// 淡出/淡入覆盖层。
    fn fade(&mut self, out: bool, ms: u64, color: [u8; 3]);
    /// 显示一行台词(SJIS/Big5 原文;播放器解码渲染)。
    fn show_text(&mut self, line_id: Option<u32>, lt: &str, le: &str);
    /// 清空台词窗。
    fn clear_text(&mut self);
    /// 语音(P9.1)。
    fn play_voice(&mut self, name: &str);
    /// BGM(音量千分比;空音量 = 仅调音量)。
    fn play_bgm(&mut self, name: &str, volume_permille: Option<i64>);
    /// SE。
    fn play_se(&mut self, name: &str);
    /// 内置标题画面(显示 + 等待菜单点击)。
    fn title_screen(&mut self);
    /// 标题菜单轮询:返回点中的按钮动作(未点/未命中 = None;点背景不推进)。
    fn poll_title_menu(&mut self) -> Option<TitleMenuAction> {
        None
    }
    /// 标题 LOAD/LASTLOAD:恢复存档(实现方走快读;完成后 scenario 已被重置)。
    fn title_load(&mut self) {}
    /// 标题 EXTRA:未实现时实现方留日志,留在标题。
    fn title_extra(&mut self) {}
    /// 标题 END:请求退出播放器。
    fn request_quit(&mut self) {}
    /// 选择肢轮询:count = 选项数;返回被点中的下标(未点 = None)。
    fn poll_choice(&mut self, count: usize) -> Option<usize>;
    /// 显示选择肢按钮。
    fn show_choices(&mut self, choices: &[String]);
    /// 清除选择肢显示。
    fn clear_choices(&mut self);
    /// 场景重置(跨文件 \GO 时清旧精灵/文本/选择肢)。
    fn reset_scene(&mut self);
    /// 读取全局槽(\GO.G.IF 的 G=n)。
    fn global(&self, slot: usize) -> i64;
    /// 写全局槽(\TITLE 按钮选择 → G1;引擎内置标题语义,成果 77/78)。
    fn set_global(&mut self, _slot: usize, _value: i64) {}
    /// 调试日志面。
    fn log(&mut self, msg: &str);
}

/// 标题菜单按钮动作(引擎原生菜单的内置等价路由;成果 76)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleMenuAction {
    /// OUTLINE(あらすじ):前情回顾 —— 原生绑定 `es.BT.SET("BTN.START",2)`
    /// 写 G1=2,经 `\GO.G.IF(1,"==",2,ARA)` 落前情回顾(成果 79)。
    Outline,
    /// START:开始新游戏(推进 scenario)。
    Start,
    /// LOAD:读档界面(内置暂走快读)。
    Load,
    /// LASTLOAD:读最近存档(快读)。
    LastLoad,
    /// EXTRA:鉴赏/附录(未实现)。
    Extra,
    /// END:退出。
    End,
}

/// 运行器等待状态。
#[derive(Debug, Clone)]
enum Wait {
    /// 纯延时(点击可跳过)。
    Timed { until: Instant, ms: u64 },
    /// 台词行:等点击。
    Line,
    /// 淡入淡出进行中(阻塞至结束;点击可跳过)。
    Fading { until: Instant },
    /// 选择肢:等玩家点选(poll_choice)。
    Choices,
    /// 标题菜单:等按钮命中(poll_title_menu;点背景不推进,成果 76)。
    TitleMenu,
}

/// 跨文件标签表:标签名 → (文件名, 元素下标)。
pub struct ScenarioPlayer {
    /// 原始字节(惰性解析:坏文件不阻塞全局)。
    raw: Vec<(String, Vec<u8>)>,
    /// 各文件已解析文本(文件名 → Scenario)。
    files: HashMap<String, Scenario>,
    /// 标签 → (文件, 下标)。惰性构建:每个文件解析成功后登记。
    labels: HashMap<String, (String, usize)>,
    /// 当前文件。
    cur: String,
    /// 当前元素下标。
    pc: usize,
    wait: Option<Wait>,
    /// 待显示台词缓存(LE 先于 LT 到达)。
    pending_le: String,
    pending_id: Option<u32>,
    /// 已完成的 fade 终态(避免逐帧动画未实现时丢终态)。
    done: bool,
    /// 跨文件跳转 → 旧场景精灵应清理(播放器消费)。
    scene_reset: bool,
    /// 最近经过的段标签(存档锚点)。
    last_label: Option<String>,
    /// \SEL.GO 的跳转目标(下一条 \SEL 消费)。
    pending_sel_targets: Option<Vec<String>>,
    /// 当前活动选择肢(poll_choice 透传)。
    active_choices: Option<Vec<String>>,
}

impl ScenarioPlayer {
    /// 从资源字节构建(惰性:文件在 goto 时解析;Big5 = 本样本繁中版)。
    pub fn new(files: Vec<(String, Vec<u8>)>) -> Self {
        Self {
            raw: files,
            files: HashMap::new(),
            labels: HashMap::new(),
            cur: String::new(),
            pc: 0,
            wait: None,
            pending_le: String::new(),
            pending_id: None,
            done: false,
            scene_reset: false,
            last_label: None,
            pending_sel_targets: None,
            active_choices: None,
        }
    }

    /// 就绪入口:解析入口文件并跳到其标签(播放器启动用)。
    pub fn start(&mut self, file: &str, label: &str) -> Result<(), String> {
        // sc.ypf 条目名带虚拟根前缀(#/$/-/:),按后缀匹配
        let real = self
            .raw
            .iter()
            .map(|(n, _)| n.clone())
            .find(|n| n.ends_with(file) || n.ends_with(&format!("/{file}").replace('/', &format!("{}", chr_backslash()))))
            .unwrap_or_else(|| file.to_string());
        self.load_file(&real)?;
        self.goto(label)
    }

    /// 解析单个文件(成功后登记标签;失败 = Err,不缓存)。
    fn load_file(&mut self, name: &str) -> Result<(), String> {
        if self.files.contains_key(name) {
            return Ok(());
        }
        let bytes = self
            .raw
            .iter()
            .find(|(n, _)| n == name || n.ends_with(name) || n.ends_with(&name.replace('/', "\\")))
            .map(|(_, b)| b.clone())
            .ok_or_else(|| format!("scenario 文件不存在: {name}"))?;
        let sc = parse_scenario_with(&bytes, encoding_rs::BIG5)
            .map_err(|e| format!("解析 {name}: {e}"))?;
        for (label, idx) in &sc.labels {
            self.labels.insert(label.clone(), (name.to_string(), *idx));
        }
        self.files.insert(name.to_string(), sc);
        Ok(())
    }

    /// 跳到标签(跨文件;目标文件惰性解析)。
    pub fn goto(&mut self, label: &str) -> Result<(), String> {
        if !self.labels.contains_key(label) {
            // 标签未知:遍历 raw 尝试解析定位(P7.2 语法盲点容错)
            let names: Vec<String> = self.raw.iter().map(|(n, _)| n.clone()).collect();
            for n in &names {
                if self.load_file(n).is_err() {
                    continue;
                }
                if self.labels.contains_key(label) {
                    break;
                }
            }
        }
        let Some((file, idx)) = self.labels.get(label).cloned() else {
            return Err(format!("scenario 标签不存在: {label}"));
        };
        self.load_file(&file)?;
        self.cur = file;
        self.pc = idx;
        self.wait = None;
        self.scene_reset = true;
        Ok(())
    }

    /// 消费场景重置标记(跨文件跳转后,播放器清旧精灵)。
    pub fn take_scene_reset(&mut self) -> bool {
        std::mem::take(&mut self.scene_reset)
    }

    /// 存档点:当前文件 + 最近段标签(P9.3 快存;恢复 = start(file,label))。
    pub fn save_point(&self) -> Result<(String, String), String> {
        let label = self
            .last_label
            .clone()
            .ok_or_else(|| "无标签锚点可存".to_string())?;
        Ok((self.cur.clone(), label))
    }

    /// 活动选择肢数。
    pub fn choice_count(&self) -> usize {
        self.active_choices.as_ref().map(|v| v.len()).unwrap_or(0)
    }

    pub fn running(&self) -> bool {
        !self.done
    }

    /// 推进一帧。`clicked` = 本帧发生了点击。
    pub fn tick(&mut self, host: &mut dyn ScenarioHost, clicked: bool) {
        if self.done {
            return;
        }
        // 标题菜单:按钮命中才路由,点背景不推进(引擎语义,成果 76)。
        if matches!(&self.wait, Some(Wait::TitleMenu)) {
            match host.poll_title_menu() {
                None => return,
                Some(act @ (TitleMenuAction::Start | TitleMenuAction::Outline)) => {
                    // 引擎原生:\TITLE 把按钮选择写入 G1(START=1 / OUTLINE=2,
                    // yst00259 `es.BT.SET("BTN.START",1|2)` 实证,成果 79)后
                    // 返回剧本,经 \GO.G.IF 落 SCENARIO_MAIN / ARA;不再依赖
                    // 末尾 \GO(SCENARIO_MAIN) 兜底。
                    let g = if matches!(act, TitleMenuAction::Outline) { 2 } else { 1 };
                    host.set_global(1, g);
                    self.wait = None;
                }
                Some(TitleMenuAction::Load | TitleMenuAction::LastLoad) => {
                    // 不预清 wait:读档成功 → start() 重置 wait;失败(无快存)→ 保持标题等待
                    host.title_load();
                    return;
                }
                Some(TitleMenuAction::Extra) => {
                    host.title_extra(); // 未实现:留日志,留在标题
                    return;
                }
                Some(TitleMenuAction::End) => {
                    host.request_quit();
                    return;
                }
            }
        }
        // 等待检查
        if let Some(w) = &self.wait {
            if matches!(w, Wait::Choices) {
                // 选择肢:点中任一项 → 跳转目标
                let n = self.active_choices.as_ref().map(|v| v.len()).unwrap_or(0);
                if let Some(idx) = host.poll_choice(n) {
                    let targets = self.pending_sel_targets.take();
                    self.active_choices = None;
                    self.wait = None;
                    host.clear_choices();
                    if let Some(ts) = targets {
                        if let Some(t) = ts.get(idx) {
                            let t = t.clone();
                            host.log(&format!("选择 {idx} → {t}"));
                            let _ = self.goto(&t);
                        }
                    }
                }
                return;
            }
            let expired = match w {
                Wait::Timed { until, .. } | Wait::Fading { until } => Instant::now() >= *until,
                Wait::Line => false,
                Wait::Choices => false, // 已在上方处理(不可达;穷尽性)
                Wait::TitleMenu => false, // 已在上方处理(不可达;穷尽性)
            };
            if !expired && !clicked {
                return;
            }
            if matches!(w, Wait::Line) && !clicked {
                return;
            }
            self.wait = None;
        }
        // 执行直到阻塞
        const MAX_STEPS: usize = 512;
        for _ in 0..MAX_STEPS {
            // 单步克隆元素(免自借用;元素很小)
            let Some(el) = self
                .files
                .get(&self.cur)
                .and_then(|sc| sc.elements.get(self.pc).cloned())
            else {
                // 文件尾 = 结束
                self.done = true;
                host.log(&format!("scenario 文件尾:{}", self.cur));
                return;
            };
            self.pc += 1;
            match &el {
                Element::Label { name, .. } => {
                    self.last_label = Some(name.clone()); // 存档锚点
                }
                Element::Command { name, modifiers, params, .. } => {
                    let done = self.exec(host, name, modifiers, params);
                    if done {
                        return; // 阻塞等待中
                    }
                }
                Element::LineId { id, .. } => {
                    self.pending_id = Some(*id);
                }
                Element::Dialogue { text, .. } => {
                    // 裸文本行(语料 0 例):并入台词显示
                    let lt = self.pending_le.clone();
                    host.show_text(self.pending_id, text, &lt);
                    self.pending_le.clear();
                    self.pending_id = None;
                    self.wait = Some(Wait::Line);
                    return;
                }
            }
        }
    }

    /// 执行一条命令;返回 true = 进入阻塞等待(本帧停)。
    fn exec(
        &mut self,
        host: &mut dyn ScenarioHost,
        name: &str,
        modifiers: &[String],
        params: &[Param],
    ) -> bool {
        let s = |i: usize| -> String {
            match params.get(i) {
                Some(Param::Str(x)) => x.clone(),
                Some(Param::Int(n)) => n.to_string(),
                _ => String::new(),
            }
        };
        let n = |i: usize| -> i64 {
            match params.get(i) {
                Some(Param::Int(x)) => *x,
                Some(Param::Float(x)) => *x as i64,
                Some(Param::Str(x)) => x.trim().parse().unwrap_or(0),
                _ => 0,
            }
        };
        match name {
            "CMXYZ" if !modifiers.is_empty() => {
                // \BG.CMXYZ(x, y, z) = 相机位移(成果 49);不换背景,切片忽略
                host.log(&format!("跳过相机位移 CMXYZ({})", s(0)));
                false
            }
            "BG" => {
                // \BG(name, fade, 0, …):name 可能带尾随空格("bg52 ")
                let raw = s(0);
                let bg = raw.trim();
                let fade = n(1).max(0) as u64;
                let color = color_of(bg);
                host.show_bg(bg, fade, color);
                false
            }
            "S" => {
                // \S(name, file, fade_ms, x, y, z, …);\S.D(name, ms) = 淡出处置
                // 勘误(成果 69):param[2] = 淡入毫秒(logo 1100/attention 1000),
                // (x,y) = param[3..5] = 左上角(0,0 = 全屏图原位)—— 旧实现把
                // 1100 当 X,把厂商 logo 画到屏幕外(「不播放」根因)。
                if modifiers.iter().any(|m| m == "D") {
                    host.hide_sprite(s(0).trim(), n(1).max(0) as u64);
                    return false;
                }
                let sname = s(0);
                let path = s(1);
                let (fade, x, y) = (n(2).max(0) as u64, n(3), n(4));
                host.show_sprite(sname.trim(), Some(path.trim()), x, y, fade);
                false
            }
            "T" => {
                // \T(name, ms, x, y, z?):name 空 = 纯等待;非空 = 立绘淡入后等待
                let tname = s(0).trim().to_string();
                let ms = n(1).max(0) as u64;
                if !tname.is_empty() {
                    let (x, y) = (n(2), n(3));
                    host.show_tachie(&tname, x, y, ms);
                }
                self.wait = Some(Wait::Timed {
                    until: Instant::now() + std::time::Duration::from_millis(ms),
                    ms,
                });
                true
            }
            "WA" => {
                let ms = n(0).max(0) as u64;
                self.wait = Some(Wait::Timed {
                    until: Instant::now() + std::time::Duration::from_millis(ms),
                    ms,
                });
                true
            }
            "FOUT" => {
                // \FOUT(ms, slot, color)
                let ms = n(0).max(0) as u64;
                let cname = s(2);
                host.fade(true, ms, color_of(cname.trim()));
                self.wait = Some(Wait::Fading {
                    until: Instant::now() + std::time::Duration::from_millis(ms),
                });
                true
            }
            "FIN" => {
                let ms = n(0).max(0) as u64;
                host.fade(false, ms, [0, 0, 0]);
                self.wait = Some(Wait::Fading {
                    until: Instant::now() + std::time::Duration::from_millis(ms),
                });
                true
            }
            "VO" => {
                host.play_voice(&s(0));
                false
            }
            "BGM" => {
                // \BGM(name, vol) / \BGM(,vol):空首槽 = 保持曲目仅调音量
                let bname = s(0).trim().to_string();
                let vol = match params.get(1) {
                    Some(Param::Int(v)) => Some(*v),
                    _ => None,
                };
                host.play_bgm(&bname, vol);
                false
            }
            "SE" => {
                host.play_se(&s(0));
                false
            }
            "TITLE" => {
                // \TITLE:内置标题画面 + 菜单等待(按钮命中路由,成果 76;
                // 旧行为 Wait::Line 盲推进 = 「点任意按钮都进游戏」根因)
                host.title_screen();
                self.wait = Some(Wait::TitleMenu);
                true
            }
            "SEL" if modifiers.iter().any(|m| m == "GO") => {
                // \SEL.GO(标签,…):登记跳转目标
                let targets: Vec<String> = params
                    .iter()
                    .filter_map(|p| match p {
                        Param::Str(x) if !x.trim().is_empty() => Some(x.trim().to_string()),
                        _ => None,
                    })
                    .collect();
                self.pending_sel_targets = Some(targets);
                false
            }
            "SEL" => {
                // \SEL("EN"×n, "", "TW"×n, ""):后段 = 本地语文本
                let texts: Vec<String> = params
                    .iter()
                    .filter_map(|p| match p {
                        Param::Str(x) => Some(x.clone()),
                        _ => None,
                    })
                    .collect();
                let n = texts.len() / 2;
                let tw = &texts[n..];
                let choices: Vec<String> = if !tw.is_empty()
                    && tw.iter().any(|x| !x.trim().is_empty())
                {
                    tw.iter().filter(|x| !x.trim().is_empty()).cloned().collect()
                } else {
                    texts.iter().filter(|x| !x.trim().is_empty()).cloned().collect()
                };
                if choices.is_empty() {
                    return false;
                }
                if self.pending_sel_targets.is_none() {
                    host.log("\\SEL 无 \\SEL.GO 目标(跳过)");
                    return false;
                }
                self.active_choices = Some(choices.clone());
                host.show_choices(&choices);
                self.wait = Some(Wait::Choices);
                true
            }
            "LE" => {
                self.pending_le = s(0);
                false
            }
            "LT" | "LC" => {
                // LT = 样本语料(AnimalTrail)主台词行;LC = NEKO-NIN exHeart
                // 语料的中文主台词行(\LE 英文 + \LC 中文成对,LE 先行)。
                // 两者语义对称:主文本 + pending 英文上屏并阻塞等待。
                let lt = s(0);
                host.clear_text();
                host.show_text(self.pending_id, &lt, &self.pending_le.clone());
                self.pending_le.clear();
                self.pending_id = None;
                self.wait = Some(Wait::Line);
                true
            }
            "GO" if modifiers.iter().any(|m| m == "IF") => {
                // \GO.G.IF(槽, op, 值, 目标):全局槽比较跳转。
                // 槽 n = host 仿真内部全局槽(成果 78 勘误:原 @50[n] 映射被
                // 运行时证伪,@50 dims=[1];引擎真值存储 Unknown,B5 待
                // es.BT.* 宏链逆向)。全语料仅 scenario_start.txt 两处,均槽 1。
                let slot = n(0).max(0) as usize;
                let op = s(1);
                let rhs = n(2);
                let target = s(3).trim().to_string();
                let cur = host.global(slot);
                let hit = match op.as_str() {
                    "==" => cur == rhs,
                    "!=" => cur != rhs,
                    ">=" => cur >= rhs,
                    "<=" => cur <= rhs,
                    ">" => cur > rhs,
                    "<" => cur < rhs,
                    _ => false,
                };
                host.log(&format!("GO.G.IF G[{slot}]={cur} {op} {rhs} → {} ", if hit { &target } else { "(不跳)" }));
                if hit {
                    match self.goto(&target) {
                        Ok(()) => host.reset_scene(),
                        Err(e) => host.log(&e),
                    }
                }
                false
            }
            "GO" => {
                let label = s(0).trim().to_string();
                match self.goto(&label) {
                    Ok(()) => {
                        host.reset_scene(); // 立即清旧场景(勿等到 tick 末,防误清新层)
                        host.log(&format!("GO → {label}"));
                    }
                    Err(e) => host.log(&e),
                }
                false
            }
            "END" => {
                host.log("\\END");
                self.done = true;
                true
            }
            other => {
                // 修饰符链/长尾命令(.D 处置、.CMXYZ 相机等)切片外:记录不猜
                let mods = if modifiers.is_empty() {
                    String::new()
                } else {
                    format!(".{}", modifiers.join("."))
                };
                host.log(&format!("跳过命令 \\{other}{mods}({} 参)", params.len()));
                // \S.D(name, ms) / \X.D 族:处置语义按修饰符近似实现
                if modifiers.iter().any(|m| m == "D") {
                    host.hide_sprite(s(0).trim(), n(1).max(0) as u64);
                }
                false
            }
        }
    }
}

fn chr_backslash() -> char {
    '\\'
}

/// 颜色名 → RGB(\BG(white)/\FOUT(…,BLACK))。
fn color_of(name: &str) -> [u8; 3] {
    match name.to_ascii_lowercase().as_str() {
        "white" => [255, 255, 255],
        "black" => [0, 0, 0],
        _ => [0, 0, 0],
    }
}

/// 编码导入占位(保持与 parse_scenario_with 同一编码族引用)。
#[allow(unused_imports)]
use encoding_rs as _;


#[cfg(test)]
mod go_gif_tests {
    //! \GO.G.IF 行为对照测试(Likely 级:@50[n] 映射未真机对照,见 PROGRESS B5;
    //! 本测试验证比较算子与跳转分支逻辑本身)。

    use super::*;

    struct TestHost {
        slot: std::cell::Cell<i64>,
        resets: std::cell::Cell<usize>,
    }
    impl ScenarioHost for TestHost {
        fn show_bg(&mut self, _: &str, _: u64, _: [u8; 3]) {}
        fn show_sprite(&mut self, _: &str, _: Option<&str>, _: i64, _: i64, _: u64) {}
        fn show_tachie(&mut self, _: &str, _: i64, _: i64, _: u64) {}
        fn hide_sprite(&mut self, _: &str, _: u64) {}
        fn fade(&mut self, _: bool, _: u64, _: [u8; 3]) {}
        fn show_text(&mut self, _: Option<u32>, _: &str, _: &str) {}
        fn clear_text(&mut self) {}
        fn play_voice(&mut self, _: &str) {}
        fn play_bgm(&mut self, _: &str, _: Option<i64>) {}
        fn play_se(&mut self, _: &str) {}
        fn title_screen(&mut self) {}
        fn poll_choice(&mut self, _: usize) -> Option<usize> { None }
        fn show_choices(&mut self, _: &[String]) {}
        fn clear_choices(&mut self) {}
        fn reset_scene(&mut self) { self.resets.set(self.resets.get() + 1); }
        fn global(&self, _: usize) -> i64 { self.slot.get() }
        fn log(&mut self, _: &str) {}
    }

    /// scenario:`#S` 段内一条 \GO.G.IF;reset_scene 次数 = 是否跳转。
    fn run_goto_gif(script: &str, slot: i64) -> usize {
        let mut p = ScenarioPlayer::new(vec![("t.txt".into(), script.as_bytes().to_vec())]);
        let _ = p.goto("S"); // 惰性解析:先定位标签
        let mut h = TestHost { slot: std::cell::Cell::new(slot), resets: std::cell::Cell::new(0) };
        p.tick(&mut h, false);
        h.resets.get()
    }

    #[test]
    fn go_gif_hit_and_miss() {
        let sc = "#S\n\\GO.G.IF(1, \"==\", 2, T)\n\\GO(END0)\n#T\n";
        // 槽=2,==2 → 命中跳 T(reset_scene 1 次)
        assert_eq!(run_goto_gif(sc, 2), 1, "== 命中应跳转");
        // 槽=0,==2 → 未命中,不跳(无 reset_scene)
        assert_eq!(run_goto_gif(sc, 0), 0, "== 未命中不应跳转");
    }

    #[test]
    fn go_gif_other_ops() {
        let base = |op: &str| format!("#S\n\\GO.G.IF(1, \"{op}\", 2, T)\n\\GO(END0)\n#T\n");
        assert_eq!(run_goto_gif(&base(">="), 2), 1);
        assert_eq!(run_goto_gif(&base(">="), 1), 0);
        assert_eq!(run_goto_gif(&base("!="), 3), 1);
        assert_eq!(run_goto_gif(&base("!="), 2), 0);
        assert_eq!(run_goto_gif(&base("<"), 1), 1);
        assert_eq!(run_goto_gif(&base("<="), 2), 1);
    }
}

#[cfg(test)]
mod title_menu_tests {
    //! 标题菜单 G1 写入回归(成果 78):\TITLE → 点 START → G1=1 →
    //! \GO.G.IF(1,"==",1,MAIN) 命中(非兜底 FALLBACK),对齐引擎原生流。

    use super::*;

    struct TitleHost {
        globals: std::cell::RefCell<std::collections::HashMap<usize, i64>>,
        resets: std::cell::Cell<usize>,
        /// poll_title_menu 返回的动作(模拟点击的按钮)。
        action: TitleMenuAction,
    }
    impl ScenarioHost for TitleHost {
        fn show_bg(&mut self, _: &str, _: u64, _: [u8; 3]) {}
        fn show_sprite(&mut self, _: &str, _: Option<&str>, _: i64, _: i64, _: u64) {}
        fn show_tachie(&mut self, _: &str, _: i64, _: i64, _: u64) {}
        fn hide_sprite(&mut self, _: &str, _: u64) {}
        fn fade(&mut self, _: bool, _: u64, _: [u8; 3]) {}
        fn show_text(&mut self, _: Option<u32>, _: &str, _: &str) {}
        fn clear_text(&mut self) {}
        fn play_voice(&mut self, _: &str) {}
        fn play_bgm(&mut self, _: &str, _: Option<i64>) {}
        fn play_se(&mut self, _: &str) {}
        fn title_screen(&mut self) {}
        fn poll_title_menu(&mut self) -> Option<TitleMenuAction> {
            Some(self.action) // 模拟点击指定按钮
        }
        fn poll_choice(&mut self, _: usize) -> Option<usize> {
            None
        }
        fn show_choices(&mut self, _: &[String]) {}
        fn clear_choices(&mut self) {}
        fn reset_scene(&mut self) {
            self.resets.set(self.resets.get() + 1);
        }
        fn global(&self, slot: usize) -> i64 {
            self.globals.borrow().get(&slot).copied().unwrap_or(0)
        }
        fn set_global(&mut self, slot: usize, value: i64) {
            self.globals.borrow_mut().insert(slot, value);
        }
        fn log(&mut self, _: &str) {}
    }

    /// scenario:`#TITLE` 段 = \TITLE + 双 \GO.G.IF + 兜底;点击 `action`
    /// 按钮后断言 G1 写入值与跳转落点(reset_scene 次数)。
    fn run_title_click(action: TitleMenuAction) -> (Option<i64>, usize) {
        let sc = "#TITLE\n\\TITLE\n\\GO.G.IF(1, \"==\", 1, MAIN)\n\\GO.G.IF(1, \"==\", 2, ARA)\n\\GO(FALLBACK)\n#MAIN\n\\END\n#ARA\n\\END\n#FALLBACK\n\\END\n";
        let mut p = ScenarioPlayer::new(vec![("t.txt".into(), sc.as_bytes().to_vec())]);
        let _ = p.goto("TITLE");
        let mut h = TitleHost {
            globals: Default::default(),
            resets: std::cell::Cell::new(0),
            action,
        };
        p.tick(&mut h, false); // \TITLE → Wait::TitleMenu
        p.tick(&mut h, false); // poll → action → 写 G1 → 继续执行循环
        let g1 = h.globals.borrow().get(&1).copied();
        (g1, h.resets.get())
    }

    #[test]
    fn title_start_writes_g1_and_branches() {
        // START → G1=1 → \GO.G.IF(1,"==",1,MAIN) 命中(1 次 reset_scene,非兜底)
        let (g1, resets) = run_title_click(TitleMenuAction::Start);
        assert_eq!(g1, Some(1), "START 应写 G1=1");
        assert_eq!(resets, 1, "应命中 \\GO.G.IF 跳 MAIN(1 次 reset_scene),而非兜底");
    }

    #[test]
    fn title_outline_writes_g2_and_branches() {
        // OUTLINE(あらすじ)→ G1=2 → \GO.G.IF(1,"==",2,ARA) 命中(成果 79)
        let (g1, resets) = run_title_click(TitleMenuAction::Outline);
        assert_eq!(g1, Some(2), "OUTLINE 应写 G1=2");
        assert_eq!(resets, 1, "应命中 \\GO.G.IF 跳 ARA(1 次 reset_scene),而非兜底");
    }
}
