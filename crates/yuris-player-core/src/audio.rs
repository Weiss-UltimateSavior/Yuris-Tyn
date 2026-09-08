//! P9.1 音频后端(播放器侧):rodio + ogg vorbis。
//!
//! 三通道:BGM(循环,音量可调)/ voice(独占,新替旧)/ SE(并发短促)。
//! 数据源 = 各音频包(vo/bgm/se/sysse.ypf)的 stored OGG 条目字节,
//! 经 `Decoder::new(Cursor)` 解码。无声设备降级为无操作(不 panic)。

use std::io::Cursor;

use rodio::{Decoder, Sink, Source};

/// 三通道音频播放器。
pub struct Audio {
    _stream: Option<rodio::OutputStream>,
    handle: Option<rodio::OutputStreamHandle>,
    bgm: Option<Sink>,
    voice: Option<Sink>,
    /// 当前 BGM 名(重复 \BGM 同名不重启)。
    cur_bgm: Option<String>,
    /// 主音量(\BGM(,800) → 0.8)。
    bgm_volume: f32,
    /// 最近 SE 播放决策留痕(含标题按钮音;上限 32,测试断言/观测用)。
    pub se_log: Vec<String>,
}

impl Audio {
    pub fn new() -> Self {
        match rodio::OutputStream::try_default() {
            Ok((stream, handle)) => Self {
                _stream: Some(stream),
                handle: Some(handle),
                bgm: None,
                voice: None,
                cur_bgm: None,
                bgm_volume: 1.0,
                se_log: Vec::new(),
            },
            Err(e) => {
                eprintln!("[audio] 无输出设备,音频降级: {e}");
                Self {
                    _stream: None,
                    handle: None,
                    bgm: None,
                    voice: None,
                    cur_bgm: None,
                    bgm_volume: 1.0,
                    se_log: Vec::new(),
                }
            }
        }
    }

    /// 播 BGM(循环;同名 + 同音量 = 不重启,`\BGM(,800)` 空首槽语义)。
    pub fn play_bgm(&mut self, name: &str, data: Vec<u8>, volume_permille: Option<i64>) {
        let vol = volume_permille.map(|p| (p.max(0) as f32 / 1000.0).clamp(0.0, 1.0));
        if data.is_empty() {
            // 仅调音量(\BGM(,800) 空首槽语义)
            if let (Some(sink), Some(v)) = (&self.bgm, vol) {
                sink.set_volume(v);
            }
            return;
        }
        if self.cur_bgm.as_deref() == Some(name) {
            if let (Some(sink), Some(v)) = (&self.bgm, vol) {
                sink.set_volume(v);
            }
            return;
        }
        self.stop_bgm();
        let Some(handle) = &self.handle else { return };
        let Ok(sink) = Sink::try_new(handle) else {
            return;
        };
        let src = Decoder::new(Cursor::new(data)).expect("BGM 解码");
        sink.append(src.repeat_infinite());
        sink.set_volume(vol.unwrap_or(self.bgm_volume));
        self.bgm = Some(sink);
        self.cur_bgm = Some(name.to_string());
        eprintln!("[audio] BGM {name} 循环");
    }

    pub fn stop_bgm(&mut self) {
        if let Some(s) = &self.bgm {
            s.stop();
        }
        self.bgm = None;
        self.cur_bgm = None;
    }

    /// 播 voice(独占;新替旧)。
    pub fn play_voice(&mut self, name: &str, data: Vec<u8>) {
        let Some(handle) = &self.handle else { return };
        if let Some(s) = &self.voice {
            s.stop();
        }
        let Ok(sink) = Sink::try_new(handle) else {
            return;
        };
        let Ok(src) = Decoder::new(Cursor::new(data)) else {
            eprintln!("[audio] voice 解码失败 {name}");
            return;
        };
        sink.append(src);
        self.voice = Some(sink);
        eprintln!("[audio] voice {name}");
    }

    /// 播 SE(一次性;不抢占;空数据 = 仅记录决策不出声)。
    pub fn play_se(&mut self, name: &str, data: Vec<u8>) {
        if self.se_log.len() >= 32 {
            self.se_log.remove(0);
        }
        self.se_log.push(name.to_string());
        if data.is_empty() {
            return;
        }
        let Some(handle) = &self.handle else { return };
        let Ok(sink) = Sink::try_new(handle) else {
            return;
        };
        if let Ok(src) = Decoder::new(Cursor::new(data)) {
            sink.append(src);
            sink.detach();
        }
    }
}
