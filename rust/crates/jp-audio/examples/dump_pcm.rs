//! 用播放器同一个解码器（rodio / symphonia）把音频完整解出来，写成小端整数 PCM，
//! 用来和 FLAC 文件头里 STREAMINFO 的 MD5 对账（只读原文件）。
//!
//! ```text
//! cargo run --release -p jp-audio --example dump_pcm -- <音频文件> <位深> <输出.pcm>
//! ```

use std::fs::File;
use std::io::{BufWriter, Write};

use rodio::{Decoder, Source};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("音频文件");
    let bits: u32 = args.next().expect("位深").parse()?;
    let output = args.next().expect("输出");
    let decoder = Decoder::try_from(File::open(&input)?)?;
    let channels = decoder.channels().get() as u64;
    let rate = decoder.sample_rate().get();
    let scale = (1i64 << (bits - 1)) as f64;
    let mut out = BufWriter::new(File::create(&output)?);
    let mut count: u64 = 0;
    for sample in decoder {
        let value = (sample as f64 * scale).round().clamp(-scale, scale - 1.0) as i32;
        out.write_all(&value.to_le_bytes()[..(bits / 8) as usize])?;
        count += 1;
    }
    out.flush()?;
    println!("{} 帧，{} 声道，{} Hz，{:.3} 秒", count / channels, channels, rate, (count / channels) as f64 / rate as f64);
    Ok(())
}
