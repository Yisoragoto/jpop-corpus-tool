//! 例句音频片段：从原曲里切出那一句，放进 Anki 的媒体文件夹。
//!
//! 全在进程里做，不需要 ffmpeg：解码用播放的那个解码器（`jp_audio::decode_window`），
//! 编码用 LAME（[`crate::mp3`]）。以前是起一个 ffmpeg 子进程，安装包又不带它，
//! 没装的人制卡就没有例句音频。
//!
//! 时间窗和 Python `gui._clip_audio` 一致（开头提前 0.3 秒、结尾多留 0.5 秒），
//! 编码参数也一致（LAME VBR 质量 5，就是 ffmpeg 的 `-q:a 5`）。
//!
//! **文件名和 Python 刻意不同**：Python 是 `jpop_{utterance_id}.mp3`，这里是
//! `jpop_clip_{md5}.mp3`（见 [`clip_name`]）。原因写在那里。

use std::path::Path;

use crate::card::Example;
use crate::connect::{AnkiConnect, AnkiError};

/// 片段的「编码参数版本」，是文件名的一部分。
///
/// **改了会让切出来的东西不一样的任何地方都要加一**：时间窗的规则（[`clip_window`]）、
/// 编码器或它的参数（`mp3.rs`）、声道的处理……加一之后新片段换新名字，
/// 不会和 Anki 里已有的旧片段同名。
pub const CLIP_ENCODING_VERSION: u32 = 1;

/// 放进 Anki 的文件名：`jpop_clip_{md5}.mp3`。md5 的原文是
/// 「规范化后的音频路径|文件大小|起点毫秒-终点毫秒|v编码参数版本」。
///
/// # 为什么和 Python 版不再一样
///
/// Python 版（和 0.2.8 以前的这里）叫 `jpop_{utterance_id}.mp3`，按歌词行号起名。
/// 行号在重建库、换库、迁移之后会从头分配：用户真实的 Anki 里 84 个被引用的旧片段中，
/// 15 个的行号在现在的库里已经是另一句歌词，再给那一行制卡就会把旧卡的音频悄悄换掉。
///
/// 现在的名字由「哪个文件的哪一段、怎么编的」决定：
///
/// - 同一句重复制卡，名字相同、内容也相同，Anki 里只有一份；
/// - 换了音频文件（路径或大小变了）、改了时间轴、改了编码参数，名字都跟着变，
///   不会顶着旧名字放出新内容；
/// - 前缀 `jpop_clip_` 后面是 32 位十六进制，**永远不会和 `jpop_{数字}.mp3` 同名**。
///   旧片段一个不动，引用它们的卡照常能放；Python 版照旧写它的。
///
/// 名字不依赖编码出来的字节，所以以后升级 LAME 只要记得给 [`CLIP_ENCODING_VERSION`] 加一。
/// 万一忘了、同名却不同内容：存的时候传的是 `deleteExisting: false`，Anki 会另起一个名字，
/// 不会覆盖（见 `connect.rs` 的 `store_media_file`）。
///
/// md5 只用来起名、不防恶意碰撞；用的是 `card.rs` 里现成的那份，不为此多一个依赖。
pub fn clip_name(audio_path: &Path, from_sec: f64, to_sec: f64) -> std::io::Result<String> {
    let absolute = std::fs::canonicalize(audio_path)?;
    let size = std::fs::metadata(&absolute)?.len();
    Ok(clip_name_of(&jp_audio::pitch::plain_path(&absolute), size, from_sec, to_sec))
}

fn clip_name_of(path: &str, size: u64, from_sec: f64, to_sec: f64) -> String {
    let (from_ms, to_ms) = ((from_sec * 1000.0).round() as u64, (to_sec * 1000.0).round() as u64);
    let key = format!("{path}|{size}|{from_ms}-{to_ms}|v{CLIP_ENCODING_VERSION}");
    let hash: String = crate::card::md5(key.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    format!("jpop_clip_{hash}.mp3")
}

/// 片段的 (起点, 终点)，单位秒。
///
/// 和 Python 一致：导出线程传的是 `end_sec or t_sec + 5.0`，切片函数里再判断
/// 「结束比开始晚不到 0.5 秒就按 6 秒算」。所以**没有下一行时间戳时是 5 秒**，
/// 6 秒只在下一行时间戳贴得太近时才用。
pub fn clip_window(start_sec: f64, end_sec: Option<f64>) -> (f64, f64) {
    let end = end_sec.unwrap_or(start_sec + 5.0);
    let end = if end > start_sec + 0.5 { end } else { start_sec + 6.0 };
    ((start_sec - 0.3).max(0.0), end + 0.5)
}

/// 切好的一句。
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// 存进 Anki 时用的文件名
    pub name: String,
    pub mp3: Vec<u8>,
}

/// 切一句。切不出来时返回原因（给人看的一句话），**不抛错**——一句切不出来不该拦住整张卡。
pub fn clip(audio_path: &Path, start_sec: f64, end_sec: Option<f64>) -> Result<Clip, String> {
    if !audio_path.is_file() {
        return Err(format!("音频文件不在了：{}", audio_path.display()));
    }
    let (from, to) = clip_window(start_sec, end_sec);
    let name = clip_name(audio_path, from, to).map_err(|err| format!("读不到音频文件 {}：{err}", audio_path.display()))?;
    let pcm = jp_audio::decode_window(audio_path, from, to).map_err(|err| format!("{err:#}"))?;
    if pcm.frames() == 0 {
        return Err(format!("{from:.1} 秒处已经超出了音频的长度"));
    }
    let mp3 = crate::mp3::encode(&pcm).map_err(|err| format!("{err:#}"))?;
    Ok(Clip { name, mp3 })
}

/// 给一张卡的例句切音频、传给 Anki，返回 SentenceAudio 字段的内容。
///
/// 一句切不出来就跳过那一句（Python 版也是）。Anki 中途关掉则返回错误，让整批停下。
pub fn attach(anki: &AnkiConnect, examples: &[Example]) -> Result<String, AnkiError> {
    let mut refs: Vec<String> = Vec::new();
    for example in examples {
        let Some(start) = example.time_sec else { continue };
        if example.audio_path.is_empty() {
            continue;
        }
        let Ok(clip) = clip(Path::new(&example.audio_path), start, example.end_sec) else { continue };
        match anki.store_media_file(&clip.name, &clip.mp3) {
            // 用 Anki 实际存下的名字（同名不同内容时它会改名）
            Ok(stored) => refs.push(format!("[sound:{stored}]")),
            Err(err) if err.is_not_running() => return Err(err),
            Err(_) => {}
        }
    }
    Ok(refs.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::testing::{FakeAnki, client};
    use crate::mp3::inspect::info_tag;
    use jp_audio::Pcm;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn the_window_matches_the_python_export_thread() {
        let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
        // 有下一行：提前 0.3，多留 0.5
        assert!(close(clip_window(10.0, Some(14.2)), (9.7, 14.7)));
        // 没有下一行：按 5 秒（Python 传的是 t_sec + 5.0）
        assert!(close(clip_window(10.0, None), (9.7, 15.5)));
        // 下一行贴得太近：按 6 秒
        assert!(close(clip_window(10.0, Some(10.3)), (9.7, 16.5)));
        // 开头不会是负数
        assert!(close(clip_window(0.1, Some(3.0)), (0.0, 3.5)));
    }

    /// 片段的名字由「哪个文件的哪一段、怎么编的」决定，和歌词行号无关。
    ///
    /// 行号在重建库、换库、迁移之后会从头分配：用户真实的 Anki 里 84 个被引用的
    /// `jpop_{行号}.mp3` 中，15 个的行号在现在的库里已经是另一句歌词了，再给那一行制卡
    /// 就会覆盖掉旧卡的音频。
    #[test]
    fn clip_names_come_from_the_file_and_the_window() {
        let name = clip_name_of(r"D:\music\夜.flac", 54_283_434, 9.7, 14.7);
        assert_eq!(name, clip_name_of(r"D:\music\夜.flac", 54_283_434, 9.7, 14.7), "同一段要同名");
        // 原文是「路径|大小|起止毫秒|版本」
        let expected: String = crate::card::md5(r"D:\music\夜.flac|54283434|9700-14700|v1".as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(name, format!("jpop_clip_{expected}.mp3"));

        for (what, other) in [
            ("换了文件", clip_name_of(r"D:\music\朝.flac", 54_283_434, 9.7, 14.7)),
            ("文件大小变了", clip_name_of(r"D:\music\夜.flac", 54_283_435, 9.7, 14.7)),
            ("起点变了", clip_name_of(r"D:\music\夜.flac", 54_283_434, 9.701, 14.7)),
            ("终点变了", clip_name_of(r"D:\music\夜.flac", 54_283_434, 9.7, 14.701)),
        ] {
            assert_ne!(name, other, "{what}，名字要跟着变");
        }
        // 不到一毫秒的差别不算换了一段
        assert_eq!(name, clip_name_of(r"D:\music\夜.flac", 54_283_434, 9.7000004, 14.7));
    }

    /// 绝不能和旧的 `jpop_{数字}.mp3` 同名：前缀之后是 32 位小写十六进制，不是纯数字。
    #[test]
    fn a_clip_name_can_never_collide_with_an_old_numbered_clip() {
        for (path, size) in [(r"D:\a.flac", 1), (r"D:\b.mp3", 2), ("/music/c.ogg", 3)] {
            let name = clip_name_of(path, size, 1.0, 2.0);
            let hash = name.strip_prefix("jpop_clip_").and_then(|r| r.strip_suffix(".mp3")).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(hash.len(), 32, "{name}");
            assert!(hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{name}");
            // 旧名字是 jpop_ 后面直接跟数字
            let after_prefix = name.strip_prefix("jpop_").unwrap();
            assert!(!after_prefix.starts_with(|c: char| c.is_ascii_digit()), "{name}");
        }
    }

    #[test]
    fn a_missing_source_file_gives_a_reason_instead_of_a_panic() {
        let reason = clip(Path::new("Z:/没有这个文件.flac"), 1.0, None).unwrap_err();
        assert!(reason.contains("没有这个文件.flac"), "{reason}");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jp-anki-audio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 测试音：频率一路升高的扫频，每一刻都和别的时刻不一样——切出来的那段该对上原曲的哪里，
    /// 拿互相关一比就知道，差一个样本都看得出来。
    fn sweep(seconds: f64, rate: u32) -> Pcm {
        let frames = (seconds * f64::from(rate)) as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f64 / f64::from(rate);
            let value = (0.5 * (2.0 * std::f64::consts::PI * (200.0 * t + 100.0 * t * t)).sin()) as f32;
            samples.extend([value, value]);
        }
        Pcm { samples, channels: 2, rate }
    }

    fn left(pcm: &Pcm) -> Vec<f32> {
        pcm.samples.chunks_exact(usize::from(pcm.channels)).map(|f| f[0]).collect()
    }

    /// 把 `clip` 在 `reference` 上前后挪 ±`reach` 个样本，找对得最齐的位置。
    /// 返回（偏移的样本数，那个位置的相关系数）。偏移为正表示片段比该在的位置晚。
    fn best_lag(clip: &[f32], reference: &[f32], expected_start: usize, reach: isize) -> (isize, f64) {
        let take = clip.len().min(20_000);
        let mut best = (0isize, f64::MIN);
        for lag in -reach..=reach {
            let start = expected_start as isize - lag;
            if start < 0 || start as usize + take > reference.len() {
                continue;
            }
            let window = &reference[start as usize..start as usize + take];
            let (mut dot, mut a2, mut b2) = (0.0f64, 0.0f64, 0.0f64);
            for (a, b) in clip[..take].iter().zip(window) {
                dot += f64::from(*a) * f64::from(*b);
                a2 += f64::from(*a).powi(2);
                b2 += f64::from(*b).powi(2);
            }
            let correlation = dot / (a2.sqrt() * b2.sqrt()).max(1e-12);
            if correlation > best.1 {
                best = (lag, correlation);
            }
        }
        best
    }

    /// 验收条件：片段的实际起止时间。
    ///
    /// 把切出来的 MP3 解回来，和原曲比：起点偏差、长度偏差都要在 1 毫秒以内，
    /// 信息帧里记的时长也要等于时间窗的长度。原曲分别用 WAV 和 MP3——
    /// MP3 的原曲要先裁掉编码器垫在开头的样本，数错了整段会晚 26 毫秒。
    #[test]
    fn a_clip_starts_and_ends_where_the_window_says() {
        let dir = scratch("timing");
        let rate = 44_100u32;
        let song = sweep(12.0, rate);
        let wav = dir.join("sweep.wav");
        jp_audio::wav::write_pcm16(&wav, &song).unwrap();
        // MP3 的原曲：就用这边的编码器编一份
        let mp3_source = dir.join("sweep.mp3");
        std::fs::write(&mp3_source, crate::mp3::encode(&song).unwrap()).unwrap();
        let reference = left(&song);

        for source in [&wav, &mp3_source] {
            // 歌词行 4.0–6.5 秒 → 时间窗 3.7–7.0 秒
            let (start, end) = (4.0, Some(6.5));
            let (from, to) = clip_window(start, end);
            let clip = clip(source, start, end).unwrap_or_else(|reason| panic!("{}：{reason}", source.display()));
            let label = source.file_name().unwrap().to_string_lossy().into_owned();

            // 信息帧里的时长 = 时间窗的长度
            let tag = info_tag(&clip.mp3).unwrap_or_else(|| panic!("{label}：片段没有信息帧"));
            let expected_frames = ((to * f64::from(rate)).round() - (from * f64::from(rate)).round()) as u64;
            assert_eq!(tag.samples(), expected_frames, "{label}：信息帧里的样本数");

            // 解回来比
            let path = dir.join(format!("{label}.clip.mp3"));
            std::fs::write(&path, &clip.mp3).unwrap();
            let decoded = jp_audio::decode_all(&path).unwrap();
            assert_eq!((decoded.channels, decoded.rate), (2, rate), "{label}");

            let length_error = (decoded.frames() as f64 - expected_frames as f64) / f64::from(rate);
            assert!(length_error.abs() <= 0.001, "{label}：长度差了 {:.2} 毫秒", length_error * 1000.0);

            let expected_start = (from * f64::from(rate)).round() as usize;
            let (lag, correlation) = best_lag(&left(&decoded), &reference, expected_start, 400);
            assert!(correlation > 0.95, "{label}：和原曲对不上（相关系数 {correlation:.3}）");
            let start_error = lag as f64 / f64::from(rate);
            assert!(start_error.abs() <= 0.001, "{label}：起点差了 {:.2} 毫秒（{lag} 个样本）", start_error * 1000.0);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 拿真实的歌切一句：`JP_CLIP_CASE="音频路径|歌词行起点秒|下一行起点秒|输出.mp3"`。
    /// 打印用时和大小，把片段写到指定位置，好和 ffmpeg 切出来的对账。
    #[test]
    #[ignore = "需要本机音频文件"]
    fn a_real_song_line_is_clipped() {
        let case = std::env::var("JP_CLIP_CASE").expect("JP_CLIP_CASE=音频路径|起点|终点|输出.mp3");
        let parts: Vec<&str> = case.split('|').collect();
        let (source, start, end, out) = (Path::new(parts[0]), parts[1].parse::<f64>().unwrap(), parts[2].parse::<f64>().unwrap(), parts[3]);
        let started = std::time::Instant::now();
        let clip = clip(source, start, Some(end)).unwrap();
        let took = started.elapsed();
        let tag = info_tag(&clip.mp3).unwrap();
        let (from, to) = clip_window(start, Some(end));
        println!(
            "{}：窗口 {from:.3}–{to:.3} 秒，{} 字节（{:.0} kbps），信息帧时长 {:.4} 秒，用时 {took:?}",
            clip.name,
            clip.mp3.len(),
            clip.mp3.len() as f64 * 8.0 / (to - from) / 1000.0,
            tag.seconds()
        );
        assert!((tag.seconds() - (to - from)).abs() < 0.001);
        std::fs::write(out, &clip.mp3).unwrap();
    }

    /// 时间窗超出歌的结尾：切到结尾为止，不报错；起点就已经超出时说明原因。
    #[test]
    fn a_window_running_past_the_end_is_cut_short() {
        let dir = scratch("end");
        let wav = dir.join("short.wav");
        jp_audio::wav::write_pcm16(&wav, &sweep(3.0, 22_050)).unwrap();

        // 最后一行没有下一行时间戳：窗口是 2.2–7.7 秒，歌只有 3 秒
        let clip = clip(&wav, 2.5, None).unwrap();
        let tag = info_tag(&clip.mp3).unwrap();
        assert!((tag.seconds() - 0.8).abs() < 0.001, "{}", tag.seconds());

        let reason = super::clip(&wav, 9.0, None).unwrap_err();
        assert!(reason.contains("超出"), "{reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 切一句、传给假 Anki。不碰任何真实曲目，也不碰真实的 Anki。
    #[test]
    fn a_real_clip_is_cut_and_handed_to_anki() {
        let dir = scratch("attach");
        let source = dir.join("tone.wav");
        jp_audio::wav::write_pcm16(&source, &sweep(4.0, 44_100)).unwrap();

        let example = Example {
            song_id: "001".into(),
            artist: "A".into(),
            title: "B".into(),
            time_sec: Some(1.0),
            text: "夜".into(),
            surface: "夜".into(),
            audio_path: source.display().to_string(),
            end_sec: Some(2.0),
            utterance_id: 7,
        };
        // Anki 那边恰好有同名文件、给它改了名：卡片里要用改过的那个名字
        let fake = Arc::new(FakeAnki::new(&[("storeMediaFile", json!("renamed-by-anki.mp3"))]));
        let field = attach(&client(fake.clone()), &[example]).unwrap();
        assert_eq!(field, "[sound:renamed-by-anki.mp3]");

        let params = fake.params_of("storeMediaFile").unwrap();
        // 请求的名字来自文件和时间窗（0.7–2.5 秒），不是行号 7
        assert_eq!(params["filename"], clip_name(&source, 0.7, 2.5).unwrap());
        // 绝不覆盖 Anki 里已有的同名文件
        assert_eq!(params["deleteExisting"], json!(false));
        let data = params["data"].as_str().unwrap();
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
        let tag = info_tag(&bytes).expect("传给 Anki 的要是带信息帧的 MP3");
        assert!((tag.seconds() - 1.8).abs() < 0.001, "{}", tag.seconds());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 没有时间轴、没有音频文件的例句直接跳过，一个请求都不发。
    #[test]
    fn examples_without_a_time_or_a_file_are_skipped() {
        let example = |time_sec: Option<f64>, audio_path: &str| Example {
            song_id: "001".into(),
            artist: "A".into(),
            title: "B".into(),
            time_sec,
            text: "夜".into(),
            surface: "夜".into(),
            audio_path: audio_path.into(),
            end_sec: None,
            utterance_id: 1,
        };
        let fake = Arc::new(FakeAnki::new(&[]));
        let field = attach(&client(fake.clone()), &[example(None, "D:/a.flac"), example(Some(1.0), ""), example(Some(1.0), "Z:/不存在.flac")]).unwrap();
        assert_eq!(field, "");
        assert!(fake.actions().is_empty(), "{:?}", fake.actions());
    }
}
