//! 临时取证工具(P2-1):symphonia 解码探测 OGG 时长/声道/采样率。
//!
//! 用法: cargo run -p yuris-resource --example tmp_se_duration -- <ogg 文件...>

use std::fs::File;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(f) = File::open(&path) else {
            println!("{path}: 打开失败");
            continue;
        };
        let mss = MediaSourceStream::new(Box::new(f), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("ogg");
        let probed = symphonia::default::get_probe().format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        );
        let mut fmt = match probed {
            Ok(p) => p.format,
            Err(e) => {
                println!("{path}: probe 失败 {e}");
                continue;
            }
        };
        let track = match fmt.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL) {
            Some(t) => t,
            None => {
                println!("{path}: 无音轨");
                continue;
            }
        };
        let sr = track.codec_params.sample_rate.unwrap_or(0);
        let ch = track.codec_params.channels.map(|c| c.count()).unwrap_or(0);
        let mut dec = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .expect("decoder");
        let mut frames = 0u64;
        loop {
            match fmt.next_packet() {
                Ok(pkt) => {
                    let Ok(decoded) = dec.decode(&pkt) else { continue };
                    if decoded.spec().rate != sr {
                        continue;
                    }
                    let mut buf = SampleBuffer::<i16>::new(decoded.capacity() as u64, *decoded.spec());
                    buf.copy_interleaved_ref(decoded);
                    frames += pkt.dur() as u64;
                }
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break
                }
                Err(_) => break,
            }
        }
        let secs = if sr > 0 { frames as f64 / sr as f64 } else { 0.0 };
        println!("{path}: {sr}Hz {ch}ch {frames} 帧 = {secs:.3}s");
    }
}
