//! 变调：ffmpeg 的 rubberband 滤镜把整首歌离线渲染成 FLAC，缓存下来再播。
//!
//! 和 Python 版（`dialogs/song_manager.py` 的 `PitchShiftWorker`、`_pitch_cache_path`）一致：
//!
//! - −6 … +6 半音，比例 2^(n/12)，滤镜 `rubberband=pitch=<比例，8 位小数>`，输出 FLAC；
//! - 缓存 `<缓存目录>/<文件名>_<±n>_<sha1 前 16 位>.flac`，sha1 的原文是
//!   「绝对路径|修改时间(ns)|字节数|半音数」，原曲一变缓存自动失效。文件名和 Python 算的逐字相同，两边缓存互用。
//!
//! 和 Python 不同的一处：先写 `.part` 临时文件，ffmpeg 成功退出后再改名。Python 直接写目标文件、
//! 只看「存在且非空」，渲染到一半被打断会留下半截文件，之后一直当缓存用。
//!
//! 不做实时变调：WSOLA 变调在音乐上金属感很重；rubberband 离线渲染一首歌实测 7 秒（4 分 19 秒的 FLAC）。
//! 时长和时间轴不变（合成信号上三处静音标记前后差 3 ms 以内），歌词时间轴照用。

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::sha1::sha1_hex;

pub const MIN_SEMITONES: i32 = -6;
pub const MAX_SEMITONES: i32 = 6;

/// 和 Python 版一样的上限。一首歌实测 7 秒，180 秒只是防卡死
const RENDER_TIMEOUT: Duration = Duration::from_secs(180);

pub fn clamp_semitones(semitones: i32) -> i32 {
    semitones.clamp(MIN_SEMITONES, MAX_SEMITONES)
}

pub fn pitch_ratio(semitones: i32) -> f64 {
    2f64.powf(f64::from(semitones) / 12.0)
}

/// 这首歌升降 `semitones` 个半音后的缓存文件位置。读不到原文件时报错。
pub fn cache_path(cache_dir: &Path, source: &Path, semitones: i32) -> std::io::Result<PathBuf> {
    let absolute = std::fs::canonicalize(source)?;
    let metadata = std::fs::metadata(&absolute)?;
    let mtime_ns = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = plain_path(&absolute);
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(cache_dir.join(cache_file_name(&path, mtime_ns, metadata.len(), semitones, &stem)))
}

/// `canonicalize` 在 Windows 上带 `\\?\` 前缀，Python 的 `Path.resolve()` 不带
fn plain_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        text.into_owned()
    }
}

fn cache_file_name(path: &str, mtime_ns: u128, size: u64, semitones: i32, stem: &str) -> String {
    let digest = sha1_hex(format!("{path}|{mtime_ns}|{size}|{semitones}").as_bytes());
    format!("{}_{semitones:+}_{}.flac", safe_stem(stem), &digest[..16])
}

/// `re.sub(r"[^0-9A-Za-z._-]+", "_", stem).strip("._") or "song"`
fn safe_stem(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    let mut in_run = false;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
            in_run = false;
        } else if !in_run {
            out.push('_');
            in_run = true;
        }
    }
    let trimmed = out.trim_matches(|c| c == '.' || c == '_');
    if trimmed.is_empty() { "song".to_owned() } else { trimmed.to_owned() }
}

fn ffmpeg_args(source: &Path, output: &Path, semitones: i32) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-y", "-hide_banner", "-loglevel", "error", "-i"].map(OsString::from).to_vec();
    args.push(source.as_os_str().to_owned());
    for arg in ["-filter:a", &format!("rubberband=pitch={:.8}", pitch_ratio(semitones)), "-vn", "-c:a", "flac", "-f", "flac"] {
        args.push(arg.into());
    }
    args.push(output.as_os_str().to_owned());
    args
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderOutcome {
    /// 这次渲染出来的
    Rendered,
    /// 已经有缓存
    Cached,
    /// 中途取消（换歌、换了半音数），临时文件已删
    Cancelled,
}

/// 渲染到 `output`。`cancel` 置位后尽快杀掉 ffmpeg。
pub fn render(ffmpeg: &Path, source: &Path, output: &Path, semitones: i32, cancel: &AtomicBool) -> Result<RenderOutcome> {
    if is_usable(output) {
        return Ok(RenderOutcome::Cached);
    }
    if let Some(dir) = output.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("建不了缓存目录 {}", dir.display()))?;
    }
    let mut part = output.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);

    let mut command = Command::new(ffmpeg);
    command.args(ffmpeg_args(source, &part, semitones)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().with_context(|| format!("启动不了 ffmpeg：{}", ffmpeg.display()))?;
    // 另起线程读 stderr：管道写满了 ffmpeg 会卡住
    let mut stderr = child.stderr.take();
    let stderr_reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    });

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancel.load(Ordering::Relaxed) || started.elapsed() > RENDER_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            let _ = std::fs::remove_file(&part);
            if cancel.load(Ordering::Relaxed) {
                return Ok(RenderOutcome::Cancelled);
            }
            bail!("变调超时（超过 {} 秒）", RENDER_TIMEOUT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let message = stderr_reader.join().unwrap_or_default();
    if !status.success() || !is_usable(&part) {
        let _ = std::fs::remove_file(&part);
        let message: String = message.trim().chars().take(500).collect();
        bail!("ffmpeg 变调失败：{}", if message.is_empty() { status.to_string() } else { message });
    }
    if is_usable(output) {
        // 另一个进程（比如 PyQt 版）同时渲染完了同一个缓存
        let _ = std::fs::remove_file(&part);
    } else {
        std::fs::rename(&part, output).with_context(|| format!("缓存文件改名失败 {}", output.display()))?;
    }
    Ok(RenderOutcome::Rendered)
}

fn is_usable(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 期望值是用 Python 版 `_pitch_cache_path` 同样的算法（venv 里的 CPython 3.12）算的
    #[test]
    fn cache_names_match_the_python_version() {
        let cases = [
            (r"D:\jp_corpus\raw\audio\サカナクション\001.flac", 1_767_614_948_006_026_600u128, 54_283_434u64, 1, "001", "001_+1_5e21053ad29212e7.flac"),
            (r"D:\jp_corpus\raw\audio\ヨルシカ\ただ君に晴れ.flac", 1_767_614_948_006_026_600, 1, -6, "ただ君に晴れ", "song_-6_35c4089bccca27e4.flac"),
            (r"C:\music\._odd name (live)..flac", 0, 0, 0, "._odd name (live).", "odd_name_live_+0_d7a2a4cbd54e4a22.flac"),
            (r"C:\music\a b.c.flac", 5, 7, 12, "a b.c", "a_b.c_+12_a13714950e801751.flac"),
        ];
        for (path, mtime, size, semitones, stem, expected) in cases {
            assert_eq!(cache_file_name(path, mtime, size, semitones, stem), expected, "{path}");
        }
    }

    /// 真实文件对账：`JP_PITCH_CASE="路径|半音数|Python 算出的文件名"`
    #[test]
    #[ignore = "需要本机音频文件"]
    fn a_real_file_gets_the_same_cache_name_as_python() {
        let case = std::env::var("JP_PITCH_CASE").expect("JP_PITCH_CASE=路径|半音数|期望文件名");
        let mut parts = case.split('|');
        let (path, semitones, expected) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        let name = cache_path(Path::new("cache"), Path::new(path), semitones.parse().unwrap()).unwrap();
        assert_eq!(name.file_name().unwrap().to_string_lossy(), expected);
    }

    #[test]
    fn verbatim_prefixes_are_stripped_like_python_resolve() {
        assert_eq!(plain_path(Path::new(r"\\?\D:\a\b.flac")), r"D:\a\b.flac");
        assert_eq!(plain_path(Path::new(r"\\?\UNC\nas\music\b.flac")), r"\\nas\music\b.flac");
        assert_eq!(plain_path(Path::new(r"D:\a\b.flac")), r"D:\a\b.flac");
    }

    #[test]
    fn ratio_and_filter_argument() {
        assert!((pitch_ratio(12) - 2.0).abs() < 1e-12);
        assert!((pitch_ratio(-12) - 0.5).abs() < 1e-12);
        let args = ffmpeg_args(Path::new("in.flac"), Path::new("out.flac.part"), 1);
        assert!(args.contains(&OsString::from("rubberband=pitch=1.05946309")), "{args:?}");
        // 临时文件没有 .flac 后缀，要显式指定格式
        let format = args.iter().position(|a| a == "-f").unwrap();
        assert_eq!(args[format + 1], "flac");
        assert_eq!(clamp_semitones(9), MAX_SEMITONES);
        assert_eq!(clamp_semitones(-9), MIN_SEMITONES);
    }

    /// 拿本机 ffmpeg 现做一段带三处静音的测试音，变调后静音还在原来的位置、时长不变；
    /// 缓存已有时不再渲染；取消时不留临时文件。找不到 ffmpeg 就跳过。
    #[test]
    fn rendering_keeps_the_timeline_and_leaves_no_partial_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let Some(ffmpeg) = ["ffmpeg.exe", "ffmpeg"].iter().map(|n| root.join(n)).find(|p| p.is_file()) else {
            eprintln!("跳过：没有 ffmpeg");
            return;
        };
        let dir = std::env::temp_dir().join(format!("jp-audio-pitch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("marks.flac");
        let status = Command::new(&ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "sine=f=440:d=12:sample_rate=44100"])
            .args(["-af", "volume='if(between(t,2,2.2)+between(t,6,6.2)+between(t,10,10.2),0,1)':eval=frame", "-ac", "2"])
            .arg(&source)
            .status()
            .unwrap();
        assert!(status.success());

        let output = cache_path(&dir.join("cache"), &source, 2).unwrap();
        let cancel = AtomicBool::new(false);
        assert_eq!(render(&ffmpeg, &source, &output, 2, &cancel).unwrap(), RenderOutcome::Rendered);
        assert_eq!(render(&ffmpeg, &source, &output, 2, &cancel).unwrap(), RenderOutcome::Cached);

        let silences = |path: &Path| -> Vec<f64> {
            let out = Command::new(&ffmpeg)
                .args(["-hide_banner", "-i"])
                .arg(path)
                .args(["-af", "silencedetect=n=-35dB:d=0.1", "-f", "null", "-"])
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stderr);
            text.split("silence_start: ").skip(1).filter_map(|s| s.split_whitespace().next()?.parse().ok()).collect()
        };
        let before = silences(&source);
        let after = silences(&output);
        assert_eq!(before.len(), 3, "{before:?}");
        assert_eq!(after.len(), 3, "{after:?}");
        for (a, b) in before.iter().zip(&after) {
            assert!((a - b).abs() < 0.02, "静音位置 {a} → {b}");
        }
        let (source_len, output_len) = (flac_seconds(&source), flac_seconds(&output));
        assert!((source_len - output_len).abs() < 0.05, "时长 {source_len} → {output_len}");

        let cancelled = cache_path(&dir.join("cache"), &source, -3).unwrap();
        assert_eq!(render(&ffmpeg, &source, &cancelled, -3, &AtomicBool::new(true)).unwrap(), RenderOutcome::Cancelled);
        let leftovers: Vec<_> = std::fs::read_dir(dir.join("cache")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(leftovers.len(), 1, "只该有 +2 那一个缓存：{leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 读 FLAC STREAMINFO 里的总样本数 / 采样率
    fn flac_seconds(path: &Path) -> f64 {
        let head = std::fs::read(path).unwrap();
        assert_eq!(&head[..4], b"fLaC");
        let bits = u64::from_be_bytes(head[18..26].try_into().unwrap());
        let rate = bits >> 44;
        let samples = bits & ((1 << 36) - 1);
        samples as f64 / rate as f64
    }
}
