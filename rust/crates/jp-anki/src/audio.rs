//! 例句音频片段：用 ffmpeg 从原曲里切出那一句，放进 Anki 的媒体文件夹。
//!
//! 参数和 Python `gui._clip_audio` 一致（开头提前 0.3 秒、结尾多留 0.5 秒、libmp3lame、`-q:a 5`），
//! 文件名 `jpop_{utterance_id}.mp3` 也一致——同一句在两边导出是同一个文件名，Anki 里不会有两份。
//!
//! 用户现有的 1046 张 JPOP Corpus 卡里 1045 张带音频，这是在用的功能。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::card::Example;
use crate::connect::{AnkiConnect, AnkiError};
use crate::export::AudioOptions;

/// 找 ffmpeg：先看 PATH，再看项目根目录下的可执行文件（Python 版也是这两处）。
pub fn find_ffmpeg(project_root: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let local = project_root.join(exe);
    local.is_file().then_some(local)
}

/// 放进 Anki 的文件名。
pub fn clip_name(utterance_id: i64) -> String {
    format!("jpop_{utterance_id}.mp3")
}

/// ffmpeg 的 (起点, 终点)，单位秒。
///
/// 和 Python 一致：导出线程传的是 `end_sec or t_sec + 5.0`，切片函数里再判断
/// 「结束比开始晚不到 0.5 秒就按 6 秒算」。所以**没有下一行时间戳时是 5 秒**，
/// 6 秒只在下一行时间戳贴得太近时才用。
pub fn clip_window(start_sec: f64, end_sec: Option<f64>) -> (f64, f64) {
    let end = end_sec.unwrap_or(start_sec + 5.0);
    let end = if end > start_sec + 0.5 { end } else { start_sec + 6.0 };
    ((start_sec - 0.3).max(0.0), end + 0.5)
}

/// 切一句，成功返回 true。
///
/// 源文件不在、ffmpeg 出错、30 秒没切完都算失败，**不抛错**——一句切不出来不该拦住整张卡。
pub fn clip(ffmpeg: &Path, audio_path: &Path, start_sec: f64, end_sec: Option<f64>, out: &Path) -> bool {
    if !audio_path.is_file() {
        return false;
    }
    let (from, to) = clip_window(start_sec, end_sec);
    let mut command = Command::new(ffmpeg);
    command
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(audio_path)
        .args(["-ss", &format!("{from:.3}"), "-to", &format!("{to:.3}")])
        .args(["-acodec", "libmp3lame", "-q:a", "5"])
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // 从 GUI 程序里起 ffmpeg，Windows 默认会闪一个黑色控制台窗口
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success() && out.is_file(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// 给一张卡的例句切音频、传给 Anki，返回 SentenceAudio 字段的内容。
///
/// 一句切不出来就跳过那一句（Python 版也是）。Anki 中途关掉则返回错误，让整批停下。
/// 切出来的临时文件传完就删——Anki 自己留了一份。
pub fn attach(anki: &AnkiConnect, audio: &AudioOptions, examples: &[Example]) -> Result<String, AnkiError> {
    let _ = std::fs::create_dir_all(&audio.work_dir);
    let mut refs: Vec<String> = Vec::new();
    for example in examples {
        let Some(start) = example.time_sec else { continue };
        if example.audio_path.is_empty() {
            continue;
        }
        let name = clip_name(example.utterance_id);
        let out = audio.work_dir.join(&name);
        if !clip(&audio.ffmpeg, Path::new(&example.audio_path), start, example.end_sec, &out) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&out) else { continue };
        let stored = anki.store_media_file(&name, &bytes);
        let _ = std::fs::remove_file(&out);
        match stored {
            Ok(()) => refs.push(format!("[sound:{name}]")),
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
    use serde_json::json;
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
        assert_eq!(clip_name(42), "jpop_42.mp3");
    }

    #[test]
    fn a_missing_source_file_is_a_quiet_failure() {
        let out = std::env::temp_dir().join("jp-anki-never.mp3");
        assert!(!clip(Path::new("ffmpeg"), Path::new("Z:/没有这个文件.flac"), 1.0, None, &out));
    }

    fn project_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap().to_path_buf()
    }

    /// 用本机真的 ffmpeg 现生成一段 4 秒的测试音，再切出一句、传给假 Anki。
    /// 不碰任何真实曲目，也不碰真实的 Anki。
    #[test]
    fn a_real_clip_is_cut_and_handed_to_anki() {
        let Some(ffmpeg) = find_ffmpeg(&project_root()) else {
            eprintln!("跳过：找不到 ffmpeg");
            return;
        };
        let dir = std::env::temp_dir().join(format!("jp-anki-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("tone.wav");
        let made = Command::new(&ffmpeg)
            .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=4"])
            .arg(&source)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(made, "ffmpeg 生成测试音失败");

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
        let fake = Arc::new(FakeAnki::new(&[("storeMediaFile", json!("jpop_7.mp3"))]));
        let options = AudioOptions {
            ffmpeg,
            work_dir: dir.join("clips"),
        };
        let field = attach(&client(fake.clone()), &options, &[example]).unwrap();
        assert_eq!(field, "[sound:jpop_7.mp3]");

        let params = fake.params_of("storeMediaFile").unwrap();
        assert_eq!(params["filename"], "jpop_7.mp3");
        let data = params["data"].as_str().unwrap();
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
        // 一句约 1.8 秒的 mp3，至少几 KB；更重要的是它确实是 mp3（ID3 头或帧同步字）
        assert!(bytes.len() > 2_000, "切出来只有 {} 字节", bytes.len());
        assert!(bytes.starts_with(b"ID3") || (bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0));
        // 临时文件传完就删
        assert!(!options.work_dir.join("jpop_7.mp3").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
