//! 变调：Rubber Band 把整首歌离线渲染成 WAV，缓存下来再播。
//!
//! 以前是起一个 ffmpeg 子进程、用它的 `rubberband` 滤镜渲染成 FLAC。安装包不带 ffmpeg，
//! 没装的人变调就不可用。现在 Rubber Band 直接编进程序里（见 `rubberband.rs`），
//! 解码用播放的那个解码器（`decode.rs`），写 WAV 不用编码器（`wav.rs`）。
//! 变调后超出满幅的峰值写成 16 位时夹到满幅，理由见下面「超幅」一节。
//!
//! - −6 … +6 半音，比例 2^(n/12)，时长不变，歌词时间轴照用；
//! - 缓存 `<缓存目录>/<文件名>_<±n>_<引擎>_<sha1 前 16 位>.wav`，sha1 的原文是
//!   「绝对路径|修改时间(ns)|字节数|半音数|引擎和版本」，原曲一变缓存自动失效；
//! - 先写临时文件、写完再改名，渲染到一半被打断不会留下半截文件被当成缓存；
//! - 缓存目录有总量上限，超了按最近使用时间淘汰（[`enforce_limit`]）。
//!
//! **缓存名和 Python 版不再一样。** 以前刻意和 `dialogs/song_manager.py` 的
//! `_pitch_cache_path` 逐字相同、两边缓存互用。现在名字里多了引擎和版本
//! （[`engine_tag`]）：换引擎（R2 ↔ R3）、升级 Rubber Band、改渲染参数之后，
//! 旧缓存不能再被当成新的——同一个名字背后是两种声音，比多渲染一次糟得多。
//! 格式也从 FLAC 换成了 WAV。旧名字的算法留在 [`legacy_cache_path`]，只用来认出并清理旧缓存。
//!
//! 不做实时变调：WSOLA 变调在音乐上金属感很重。
//!
//! # 超幅
//!
//! 变调后的浮点样本会超出 ±1.0：流行歌的母带本来就压在 0 dB 附近，变调改变了各频率的相位关系，
//! 被压平的峰值重新冒出来。写成 16 位时**夹到满幅（硬削）**，不做限幅、不整首压低。
//! 这是量过之后定的（R3，升 3 个半音，两首不同歌手的歌，数字相近；下面是其中一首 4 分 17 秒的）：
//!
//! - 峰值 +5.6 dB，但超幅的样本只有 0.37%，分成两万七千多处，**每处中位 2 个样本（0.05 毫秒）**，
//!   95% 不超过 8 个样本，最长 61 个（1.4 毫秒）；超出量中位 +0.7 dB。削掉的是极短的尖峰；
//! - 硬削：整首 RMS 变化 −0.06 dB，按 0.4 秒一段看响度最多变 0.3 dB；和没削的信号相比误差 −29 dB；
//! - 前视限幅（5 ms 前视，释放 20 / 50 / 100 ms 都试了）：峰值附近要把增益压下去再慢慢放回来，
//!   整首 RMS 轻 1.1–1.6 dB，5% 的 0.4 秒段被压低 2 dB 以上、最深 3 dB——一阵一阵变轻；
//!   误差 −23 dB，比硬削还大；
//! - 整首按峰值压低：不失真，但整首轻 5.6 dB，换调时音量会跳。
//!
//! ffmpeg 渲染的缓存一直也是硬削的（它把浮点转成整数时就是夹住）。
//! 不会回绕成爆音：`wav::quantize` 夹在 16 位范围内。重新量：`a_real_song_renders_to_the_same_length`。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::rubberband::{self, Engine};
use crate::sha1::sha1_hex;
use crate::{decode, wav};

pub const MIN_SEMITONES: i32 = -6;
pub const MAX_SEMITONES: i32 = 6;

/// 用哪个引擎渲染：R3。拿同一段歌的 ±3、±6 半音样本和 R2 对比着听过之后选的，
/// 代价是渲染慢三倍左右（一首歌 20–40 秒，R2 是 7–11 秒）。
///
/// 改这一个常量就换引擎，缓存名跟着变，不用手工清缓存。
pub const ENGINE: Engine = Engine::R3;

/// 渲染方式的版本。**改了会影响输出的任何东西都要加一**：传给 Rubber Band 的选项、
/// 超幅的处理方式、输出的位深……加一之后旧缓存不再被认作这一版的。
/// （换引擎不用加：引擎已经在缓存名里。）
const RENDER_VERSION: u32 = 1;

/// 缓存总量上限的默认值：1.5 GB，约 33 首 4 分钟的歌（一首 16 位立体声 WAV 约 45 MB）。
pub const DEFAULT_CACHE_LIMIT_BYTES: u64 = 1536 * 1024 * 1024;

/// 没写完的临时文件留多久。正常渲染一首不到一分钟；超过这个时间的是崩溃或断电留下的
const STALE_PART_AGE: Duration = Duration::from_secs(3600);

pub fn clamp_semitones(semitones: i32) -> i32 {
    semitones.clamp(MIN_SEMITONES, MAX_SEMITONES)
}

/// 编进缓存名里的「是谁、怎么渲染的」，例如 `rubberband-4.0.0-r3-v1`。
pub fn engine_tag(engine: Engine) -> String {
    format!("rubberband-{}-{}-v{RENDER_VERSION}", rubberband::LIBRARY_VERSION, engine.tag())
}

/// 这首歌升降 `semitones` 个半音后的缓存文件位置（当前引擎）。读不到原文件时报错。
pub fn cache_path(cache_dir: &Path, source: &Path, semitones: i32) -> std::io::Result<PathBuf> {
    cache_path_for(cache_dir, source, semitones, ENGINE)
}

pub fn cache_path_for(cache_dir: &Path, source: &Path, semitones: i32, engine: Engine) -> std::io::Result<PathBuf> {
    let identity = SourceIdentity::read(source)?;
    Ok(cache_dir.join(cache_file_name(&identity, semitones, engine)))
}

/// ffmpeg 时代（也是 Python 版）的缓存位置。**不再往这里写**，只用来找到旧缓存。
pub fn legacy_cache_path(cache_dir: &Path, source: &Path, semitones: i32) -> std::io::Result<PathBuf> {
    let identity = SourceIdentity::read(source)?;
    Ok(cache_dir.join(legacy_cache_file_name(&identity.path, identity.mtime_ns, identity.size, semitones, &identity.stem)))
}

/// 这首歌所有可能的缓存文件：每个半音数、两个引擎、加上旧名字。删歌时用。
pub fn cache_paths_of(cache_dir: &Path, source: &Path) -> std::io::Result<Vec<PathBuf>> {
    let identity = SourceIdentity::read(source)?;
    let mut paths = Vec::new();
    for semitones in (MIN_SEMITONES..=MAX_SEMITONES).filter(|&n| n != 0) {
        for engine in [Engine::R2, Engine::R3] {
            paths.push(cache_dir.join(cache_file_name(&identity, semitones, engine)));
        }
        paths.push(cache_dir.join(legacy_cache_file_name(&identity.path, identity.mtime_ns, identity.size, semitones, &identity.stem)));
    }
    Ok(paths)
}

/// 原曲的「身份」：路径、修改时间、大小。任何一样变了，缓存名就变。
struct SourceIdentity {
    path: String,
    mtime_ns: u128,
    size: u64,
    stem: String,
}

impl SourceIdentity {
    fn read(source: &Path) -> std::io::Result<Self> {
        let absolute = std::fs::canonicalize(source)?;
        let metadata = std::fs::metadata(&absolute)?;
        let mtime_ns = metadata.modified()?.duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        Ok(Self {
            path: plain_path(&absolute),
            mtime_ns,
            size: metadata.len(),
            stem: source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        })
    }
}

/// 绝对路径的普通写法：`canonicalize` 在 Windows 上带 `\\?\` 前缀，去掉。
///
/// Anki 例句片段的名字也拿它当「规范化后的音频路径」（`jp-anki` 的 `audio.rs`）。
pub fn plain_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        text.into_owned()
    }
}

fn cache_file_name(identity: &SourceIdentity, semitones: i32, engine: Engine) -> String {
    let SourceIdentity { path, mtime_ns, size, stem } = identity;
    let digest = sha1_hex(format!("{path}|{mtime_ns}|{size}|{semitones}|{}", engine_tag(engine)).as_bytes());
    // 引擎在文件名里也写一遍，是给人看的；真正起作用的是哈希里的那一份（带着库版本和渲染版本）
    format!("{}_{semitones:+}_{}_{}.wav", safe_stem(stem), engine.tag(), &digest[..16])
}

fn legacy_cache_file_name(path: &str, mtime_ns: u128, size: u64, semitones: i32, stem: &str) -> String {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderOutcome {
    /// 这次渲染出来的
    Rendered,
    /// 已经有缓存
    Cached,
    /// 中途取消（换歌、换了半音数），临时文件已删
    Cancelled,
}

/// 用当前引擎渲染到 `output`。`cancel` 置位后尽快停下。
pub fn render(source: &Path, output: &Path, semitones: i32, cancel: &AtomicBool) -> Result<RenderOutcome> {
    render_with(ENGINE, source, output, semitones, cancel)
}

pub fn render_with(engine: Engine, source: &Path, output: &Path, semitones: i32, cancel: &AtomicBool) -> Result<RenderOutcome> {
    if is_usable(output) {
        return Ok(RenderOutcome::Cached);
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(RenderOutcome::Cancelled);
    }
    if let Some(dir) = output.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("建不了缓存目录 {}", dir.display()))?;
    }

    let pcm = decode::decode_all(source)?;
    if pcm.frames() == 0 {
        anyhow::bail!("{} 里没有解出音频", source.display());
    }
    let Some(shifted) = rubberband::shift_pitch(&pcm, f64::from(semitones), engine, cancel)? else {
        return Ok(RenderOutcome::Cancelled);
    };
    drop(pcm);
    // 变调后的峰值会超出满幅；写 16 位时夹住（见模块注释的「超幅」一节）

    // 临时文件名每次不同：被取消的那次渲染可能还没退出，不能和新的一次写同一个文件
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let mut part = output.as_os_str().to_owned();
    part.push(format!(".{}-{}.part", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
    let part = PathBuf::from(part);

    if let Err(err) = wav::write_pcm16(&part, &shifted) {
        let _ = std::fs::remove_file(&part);
        return Err(err.context("写变调缓存失败"));
    }
    if cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&part);
        return Ok(RenderOutcome::Cancelled);
    }
    if is_usable(output) {
        // 另一个进程同时渲染完了同一个缓存
        let _ = std::fs::remove_file(&part);
    } else if let Err(err) = std::fs::rename(&part, output) {
        let _ = std::fs::remove_file(&part);
        return Err(err).with_context(|| format!("缓存文件改名失败 {}", output.display()));
    }
    Ok(RenderOutcome::Rendered)
}

fn is_usable(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
}

// ────────────────────────────── 缓存上限 ──────────────────────────────

/// 记一笔「刚用过」：把文件的修改时间改成现在。淘汰时按它排先后。
///
/// 不用系统的访问时间：NTFS 上它默认是延迟更新甚至关掉的，杀毒软件扫一遍也会改它。
/// 修改时间只有这里会动——缓存认不认得出来靠的是文件名，和它自己的修改时间无关。
pub fn mark_used(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// 缓存目录现在的占用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheUsage {
    pub files: usize,
    pub bytes: u64,
}

/// 一次淘汰的结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Evicted {
    pub files: usize,
    pub bytes: u64,
    /// 淘汰之后还占多少
    pub remaining: CacheUsage,
}

struct CacheFile {
    path: PathBuf,
    bytes: u64,
    used: SystemTime,
}

/// 是不是变调缓存的文件名：`…_<16 位十六进制>.wav`（现在的）或 `.flac`（ffmpeg 时代的）。
///
/// 淘汰只动认得出来的文件。缓存目录里万一有别的东西（用户自己放的、别的程序写的），不碰。
fn is_cache_name(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    if !matches!(ext, "wav" | "flac") {
        return false;
    }
    let Some((_, digest)) = stem.rsplit_once('_') else { return false };
    digest.len() == 16 && digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn cache_files(cache_dir: &Path) -> Vec<CacheFile> {
    let Ok(entries) = std::fs::read_dir(cache_dir) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let metadata = entry.metadata().ok()?;
            (metadata.is_file() && is_cache_name(&name.to_string_lossy())).then(|| CacheFile {
                path: entry.path(),
                bytes: metadata.len(),
                used: metadata.modified().unwrap_or(UNIX_EPOCH),
            })
        })
        .collect()
}

pub fn cache_usage(cache_dir: &Path) -> CacheUsage {
    let files = cache_files(cache_dir);
    CacheUsage { files: files.len(), bytes: files.iter().map(|f| f.bytes).sum() }
}

/// 把缓存目录压到 `limit_bytes` 以内：从最久没用的删起。
///
/// `keep` 是刚渲染好、马上要播的那个文件，**无论如何不删**——哪怕上限比它一个文件还小，
/// 不然就成了「渲染完立刻删掉、再渲染、再删」。
///
/// ffmpeg 时代留下的 `.flac` 旧缓存也算在内：新版本不会再用它们，它们的最近使用时间
/// 也最老，自然最先被淘汰。顺带清掉崩溃时留下的 `.part` 临时文件。
pub fn enforce_limit(cache_dir: &Path, limit_bytes: u64, keep: Option<&Path>) -> Evicted {
    remove_stale_parts(cache_dir);

    let mut files = cache_files(cache_dir);
    files.sort_by_key(|f| f.used);
    let mut total: u64 = files.iter().map(|f| f.bytes).sum();
    let mut left = files.len();
    let mut evicted = Evicted::default();
    for file in &files {
        if total <= limit_bytes {
            break;
        }
        if keep.is_some_and(|keep| same_file(keep, &file.path)) {
            continue;
        }
        // 删不掉（被占用、没权限）就跳过，下次再说
        if std::fs::remove_file(&file.path).is_ok() {
            total -= file.bytes;
            left -= 1;
            evicted.files += 1;
            evicted.bytes += file.bytes;
        }
    }
    evicted.remaining = CacheUsage { files: left, bytes: total };
    evicted
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

fn remove_stale_parts(cache_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else { return };
    for entry in entries.flatten() {
        let is_part = entry.path().extension().is_some_and(|ext| ext == "part");
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > STALE_PART_AGE);
        if is_part && stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Pcm;

    fn identity(path: &str, mtime_ns: u128, size: u64, stem: &str) -> SourceIdentity {
        SourceIdentity { path: path.into(), mtime_ns, size, stem: stem.into() }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jp-audio-pitch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 旧名字的算法不能动：靠它才认得出 ffmpeg 时代（和 Python 版）留下的缓存。
    /// 期望值是用 Python 版 `_pitch_cache_path` 同样的算法（venv 里的 CPython 3.12）算的
    #[test]
    fn legacy_cache_names_still_match_the_python_version() {
        let cases = [
            (r"D:\jp_corpus\raw\audio\サカナクション\001.flac", 1_767_614_948_006_026_600u128, 54_283_434u64, 1, "001", "001_+1_5e21053ad29212e7.flac"),
            (r"D:\jp_corpus\raw\audio\ヨルシカ\ただ君に晴れ.flac", 1_767_614_948_006_026_600, 1, -6, "ただ君に晴れ", "song_-6_35c4089bccca27e4.flac"),
            (r"C:\music\._odd name (live)..flac", 0, 0, 0, "._odd name (live).", "odd_name_live_+0_d7a2a4cbd54e4a22.flac"),
            (r"C:\music\a b.c.flac", 5, 7, 12, "a b.c", "a_b.c_+12_a13714950e801751.flac"),
        ];
        for (path, mtime, size, semitones, stem, expected) in cases {
            assert_eq!(legacy_cache_file_name(path, mtime, size, semitones, stem), expected, "{path}");
        }
    }

    /// 缓存名里带着引擎和版本：同一首歌同一个半音数，换引擎就是另一个文件，
    /// 而且永远不会和旧的 `.flac` 缓存同名。
    #[test]
    fn cache_names_carry_the_engine_and_version() {
        let id = identity(r"D:\music\001.flac", 1_767_614_948_006_026_600, 54_283_434, "001");
        let r3 = cache_file_name(&id, 2, Engine::R3);
        let r2 = cache_file_name(&id, 2, Engine::R2);
        assert_ne!(r3, r2);
        assert!(r3.starts_with("001_+2_r3_") && r3.ends_with(".wav"), "{r3}");
        assert!(r2.starts_with("001_+2_r2_") && r2.ends_with(".wav"), "{r2}");
        // 文件名里的 r3 只是给人看的：哈希本身也不一样（里面有库版本和渲染版本）
        let digest = |name: &str| name.rsplit_once('_').unwrap().1.to_owned();
        assert_ne!(digest(&r3), digest(&r2));
        assert_ne!(digest(&r3), digest(&legacy_cache_file_name(&id.path, id.mtime_ns, id.size, 2, &id.stem)));

        assert_eq!(engine_tag(Engine::R3), format!("rubberband-{}-r3-v{RENDER_VERSION}", rubberband::LIBRARY_VERSION));
        // 原曲变了（大小、修改时间）或半音数变了，名字都要变
        assert_ne!(r3, cache_file_name(&identity(&id.path, id.mtime_ns, id.size + 1, "001"), 2, Engine::R3));
        assert_ne!(r3, cache_file_name(&identity(&id.path, id.mtime_ns + 1, id.size, "001"), 2, Engine::R3));
        assert_ne!(r3, cache_file_name(&id, 3, Engine::R3));
        assert!(is_cache_name(&r3) && is_cache_name(&r2));
    }

    /// 真实文件对账：`JP_PITCH_CASE="路径|半音数|Python 算出的文件名"`
    #[test]
    #[ignore = "需要本机音频文件"]
    fn a_real_file_gets_the_same_legacy_cache_name_as_python() {
        let case = std::env::var("JP_PITCH_CASE").expect("JP_PITCH_CASE=路径|半音数|期望文件名");
        let mut parts = case.split('|');
        let (path, semitones, expected) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        let name = legacy_cache_path(Path::new("cache"), Path::new(path), semitones.parse().unwrap()).unwrap();
        assert_eq!(name.file_name().unwrap().to_string_lossy(), expected);
    }

    /// 拿真实的歌量一遍：`JP_PITCH_SOURCE=路径`。打印各引擎、各半音数的渲染用时和峰值，
    /// 并确认帧数不变。用 `--release --nocapture` 跑才是用户实际感受到的速度。
    #[test]
    #[ignore = "需要本机音频文件"]
    fn a_real_song_renders_to_the_same_length() {
        let source = PathBuf::from(std::env::var("JP_PITCH_SOURCE").expect("JP_PITCH_SOURCE=音频路径"));
        let started = std::time::Instant::now();
        let pcm = decode::decode_all(&source).unwrap();
        println!("解码 {:.1} 秒的音频用时 {:?}", pcm.seconds(), started.elapsed());
        for engine in [Engine::R2, Engine::R3] {
            let wanted = std::env::var("JP_PITCH_SEMITONES").unwrap_or_else(|_| "-6,-3,3,6".into());
            for semitones in wanted.split(',').map(|n| n.trim().parse::<i32>().expect("JP_PITCH_SEMITONES=3,6")) {
                let started = std::time::Instant::now();
                let shifted = rubberband::shift_pitch(&pcm, f64::from(semitones), engine, &AtomicBool::new(false)).unwrap().unwrap();
                let took = started.elapsed();
                let peak = shifted.samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
                println!("{engine:?} {semitones:+}：{took:?}，峰值 {peak:.3}（{:+.2} dB）", 20.0 * peak.log10());
                assert_eq!(shifted.frames(), pcm.frames());
                // 超出满幅的样本有多少、有多集中：决定了削波该怎么处理
                let over = shifted.samples.iter().filter(|s| s.abs() > 1.0).count();
                let mut magnitudes: Vec<f32> = shifted.samples.iter().map(|s| s.abs()).collect();
                magnitudes.sort_by(|a, b| a.total_cmp(b));
                let at = |q: f64| magnitudes[((magnitudes.len() - 1) as f64 * q) as usize];
                let seconds_with_overs = shifted
                    .samples
                    .chunks(pcm.rate as usize * usize::from(pcm.channels))
                    .filter(|second| second.iter().any(|s| s.abs() > 1.0))
                    .count();
                println!(
                    "    超幅样本 {:.4}%，99.9% 分位 {:.3}，99.99% 分位 {:.3}，{} 秒里有 {} 秒出现过超幅",
                    100.0 * over as f64 / shifted.samples.len() as f64,
                    at(0.999),
                    at(0.9999),
                    pcm.seconds() as usize,
                    seconds_with_overs
                );
                // 硬削削掉的是什么样的东西：每一处连着削了多少个样本、超出多少
                {
                    let width = usize::from(pcm.channels);
                    let mut runs: Vec<usize> = Vec::new();
                    for channel in 0..width {
                        let mut run = 0usize;
                        for sample in shifted.samples.iter().skip(channel).step_by(width) {
                            if sample.abs() > 1.0 {
                                run += 1;
                            } else if run > 0 {
                                runs.push(run);
                                run = 0;
                            }
                        }
                    }
                    runs.sort_unstable();
                    let mut depth: Vec<f32> = shifted.samples.iter().filter(|s| s.abs() > 1.0).map(|s| 20.0 * s.abs().log10()).collect();
                    depth.sort_by(|a, b| a.total_cmp(b));
                    let ms = |n: usize| n as f64 * 1000.0 / f64::from(pcm.rate);
                    println!(
                        "    超幅 {} 处：连续长度 中位 {} 个样本（{:.2} ms），95% 分位 {}（{:.2} ms），最长 {}（{:.2} ms）；超出量 中位 {:+.2} dB，95% 分位 {:+.2} dB",
                        runs.len(),
                        runs[runs.len() / 2], ms(runs[runs.len() / 2]),
                        runs[runs.len() * 95 / 100], ms(runs[runs.len() * 95 / 100]),
                        runs[runs.len() - 1], ms(runs[runs.len() - 1]),
                        depth[depth.len() / 2],
                        depth[depth.len() * 95 / 100],
                    );
                }

                // 硬削之后响度变了多少：整首，和按 0.4 秒一段看最多变多少
                let rms = |samples: &[f32]| (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len().max(1) as f64).sqrt().max(1e-9);
                let clamped: Vec<f32> = shifted.samples.iter().map(|s| s.clamp(-1.0, 1.0)).collect();
                let block = (f64::from(pcm.rate) * 0.4) as usize * usize::from(pcm.channels);
                let deepest = clamped
                    .chunks(block)
                    .zip(shifted.samples.chunks(block))
                    .map(|(a, b)| 20.0 * (rms(a) / rms(b)).log10())
                    .fold(0.0f64, f64::min);
                let error: f64 = clamped.iter().zip(&shifted.samples).map(|(a, b)| f64::from(a - b).powi(2)).sum::<f64>() / clamped.len() as f64;
                println!(
                    "    硬削：整首 RMS {:+.2} dB，0.4 秒段最多 {:+.2} dB，误差 {:.1} dB",
                    20.0 * (rms(&clamped) / rms(&shifted.samples)).log10(),
                    deepest,
                    10.0 * (error / rms(&shifted.samples).powi(2)).log10()
                );
            }
        }
    }

    #[test]
    fn verbatim_prefixes_are_stripped() {
        assert_eq!(plain_path(Path::new(r"\\?\D:\a\b.flac")), r"D:\a\b.flac");
        assert_eq!(plain_path(Path::new(r"\\?\UNC\nas\music\b.flac")), r"\\nas\music\b.flac");
        assert_eq!(plain_path(Path::new(r"D:\a\b.flac")), r"D:\a\b.flac");
    }

    #[test]
    fn semitones_are_clamped() {
        assert_eq!(clamp_semitones(9), MAX_SEMITONES);
        assert_eq!(clamp_semitones(-9), MIN_SEMITONES);
        assert_eq!(clamp_semitones(-2), -2);
    }

    /// 测试音：440 Hz，12 秒，2 秒 / 6 秒 / 10 秒处各有 0.2 秒静音。
    /// 静音的位置就是「时间轴」的标记——变调前后要在同一个地方。
    fn marked_tone(rate: u32) -> Pcm {
        let frames = rate as usize * 12;
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f64 / f64::from(rate);
            let silent = [2.0, 6.0, 10.0].iter().any(|start| (*start..start + 0.2).contains(&t));
            let value = if silent { 0.0 } else { (0.5 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as f32 };
            samples.extend([value, value]);
        }
        Pcm { samples, channels: 2, rate }
    }

    /// 每段静音的起点（秒）：10 ms 一格，整格都低于 −35 dB 才算静音
    fn silence_starts(pcm: &Pcm) -> Vec<f64> {
        let window = pcm.rate as usize / 100;
        let quiet: Vec<bool> = pcm
            .samples
            .chunks(window * usize::from(pcm.channels))
            .map(|block| block.iter().all(|s| s.abs() < 0.0178))
            .collect();
        let mut starts = Vec::new();
        let mut run = 0;
        for (i, is_quiet) in quiet.iter().enumerate() {
            run = if *is_quiet { run + 1 } else { 0 };
            // 连着 10 格（0.1 秒）才记一次，记的是这一段开头的那一格
            if run == 10 {
                starts.push((i + 1 - 10) as f64 * 0.01);
            }
        }
        starts
    }

    /// 两个引擎都要：变调后静音还在原来的位置、帧数一个不差、音高确实变了；
    /// 缓存已有时不再渲染；取消时不留临时文件。
    #[test]
    fn rendering_keeps_the_timeline_and_leaves_no_partial_files() {
        let dir = scratch("render");
        let source = dir.join("marks.wav");
        let tone = marked_tone(44_100);
        wav::write_pcm16(&source, &tone).unwrap();
        let cache = dir.join("cache");
        let cancel = AtomicBool::new(false);

        for engine in [Engine::R2, Engine::R3] {
            let output = cache_path_for(&cache, &source, 2, engine).unwrap();
            assert_eq!(render_with(engine, &source, &output, 2, &cancel).unwrap(), RenderOutcome::Rendered);
            assert_eq!(render_with(engine, &source, &output, 2, &cancel).unwrap(), RenderOutcome::Cached);

            let rendered = decode::decode_all(&output).unwrap();
            assert_eq!((rendered.channels, rendered.rate), (2, 44_100));
            assert_eq!(rendered.frames(), tone.frames(), "{engine:?}：时长要一帧不差");
            let (before, after) = (silence_starts(&tone), silence_starts(&rendered));
            assert_eq!(before.len(), 3, "{before:?}");
            assert_eq!(after.len(), 3, "{engine:?} {after:?}");
            for (a, b) in before.iter().zip(&after) {
                assert!((a - b).abs() <= 0.02, "{engine:?}：静音位置 {a} → {b}");
            }
            // 3.0–5.0 秒是纯音：+2 半音后应为 440 × 2^(2/12) ≈ 493.9 Hz
            let mono: Vec<f32> = rendered.samples.as_chunks::<2>().0.iter().skip(44_100 * 3).take(44_100 * 2).map(|f| f[0]).collect();
            let hz = mono.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f64 / 2.0;
            assert!((hz - 493.88).abs() < 5.0, "{engine:?}：{hz} Hz");
        }

        let cancelled = cache_path(&cache, &source, -3).unwrap();
        assert_eq!(render(&source, &cancelled, -3, &AtomicBool::new(true)).unwrap(), RenderOutcome::Cancelled);
        let mut leftovers: Vec<String> =
            std::fs::read_dir(&cache).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        leftovers.sort();
        assert_eq!(leftovers.len(), 2, "只该有 +2 的两个引擎各一个缓存：{leftovers:?}");
        assert!(leftovers.iter().all(|name| name.ends_with(".wav")), "{leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 变调后超幅的音频写成 16 位：超出的部分夹在满幅上，不能回绕成爆音。
    /// 测试音是接近满幅的方波叠正弦——变调会把它的峰值推到满幅以上。
    #[test]
    fn overs_are_clamped_to_full_scale_and_never_wrap() {
        let dir = scratch("overs");
        let source = dir.join("loud.wav");
        let rate = 44_100u32;
        let samples: Vec<f32> = (0..rate as usize * 3)
            .flat_map(|i| {
                let t = i as f64 / f64::from(rate);
                let square = if (t * 220.0).fract() < 0.5 { 0.7 } else { -0.7 };
                let value = (square + 0.3 * (2.0 * std::f64::consts::PI * 1_760.0 * t).sin()) as f32;
                [value, value]
            })
            .collect();
        wav::write_pcm16(&source, &Pcm { samples, channels: 2, rate }).unwrap();

        // 先确认这个测试音变调后真的超幅了，不然下面什么都没测到
        let shifted = rubberband::shift_pitch(&decode::decode_all(&source).unwrap(), 3.0, Engine::R2, &AtomicBool::new(false)).unwrap().unwrap();
        let peak = shifted.samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!(peak > 1.05, "测试音变调后峰值只有 {peak}，没超幅");
        let over = shifted.samples.iter().filter(|s| s.abs() > 1.0).count();

        let output = dir.join("shifted.wav");
        assert_eq!(render_with(Engine::R2, &source, &output, 3, &AtomicBool::new(false)).unwrap(), RenderOutcome::Rendered);
        let rendered = decode::decode_all(&output).unwrap();
        // 回绕的话相邻两个样本会从正满幅直接跳到负满幅：差接近 2.0。正常波形在这个测试音上差不到 1.7
        let left: Vec<f32> = rendered.samples.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
        let worst_jump = left.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(worst_jump < 1.9, "相邻样本跳了 {worst_jump}，像是回绕");
        // 超幅的那些样本，一个不少都贴在满幅上；别的样本没有被连带压低
        let pinned = rendered.samples.iter().filter(|s| s.abs() >= 0.9999).count();
        assert!(pinned >= over, "超幅 {over} 个样本，贴在满幅上的只有 {pinned} 个");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_source_that_cannot_be_decoded_fails_and_leaves_nothing_behind() {
        let dir = scratch("bad");
        let source = dir.join("假的.flac");
        std::fs::write(&source, b"not audio").unwrap();
        let cache = dir.join("cache");
        let output = cache_path(&cache, &source, 1).unwrap();
        let err = format!("{:#}", render(&source, &output, 1, &AtomicBool::new(false)).unwrap_err());
        assert!(err.contains("假的.flac"), "{err}");
        assert_eq!(std::fs::read_dir(&cache).map(|d| d.count()).unwrap_or(0), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 建一个指定大小、指定「多久以前用过」的文件
    fn cached(dir: &Path, name: &str, bytes: usize, seconds_ago: u64) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, vec![0u8; bytes]).unwrap();
        let when = SystemTime::now() - Duration::from_secs(seconds_ago);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        path
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// 超了上限：从最久没用的删起，删到不超为止；不认识的文件不碰。
    #[test]
    fn the_least_recently_used_caches_go_first() {
        let dir = scratch("limit");
        cached(&dir, "a_+1_r3_0000000000000001.wav", 400, 400);
        cached(&dir, "b_+1_r3_0000000000000002.wav", 400, 300);
        cached(&dir, "c_+1_r3_0000000000000003.wav", 400, 200);
        // ffmpeg 时代的旧缓存：也算占用，也能被淘汰
        cached(&dir, "old_+2_00000000000000aa.flac", 400, 500);
        // 不是缓存的文件：再旧也不动
        cached(&dir, "我的备注.txt", 400, 9_000);
        cached(&dir, "listen_to_this.wav", 400, 9_000);

        assert_eq!(cache_usage(&dir), CacheUsage { files: 4, bytes: 1_600 });
        let evicted = enforce_limit(&dir, 1_000, None);
        assert_eq!(evicted, Evicted { files: 2, bytes: 800, remaining: CacheUsage { files: 2, bytes: 800 } });
        assert_eq!(names(&dir), ["b_+1_r3_0000000000000002.wav", "c_+1_r3_0000000000000003.wav", "listen_to_this.wav", "我的备注.txt"]);

        // 没超就什么都不删
        assert_eq!(enforce_limit(&dir, 1_000, None).files, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 「刚用过」的排到最后；刚渲染好的那个哪怕上限比它还小也不删。
    #[test]
    fn a_used_cache_outlives_an_unused_one_and_the_fresh_render_is_never_evicted() {
        let dir = scratch("lru");
        let old = cached(&dir, "a_+1_r3_0000000000000001.wav", 400, 400);
        cached(&dir, "b_+1_r3_0000000000000002.wav", 400, 300);
        let fresh = cached(&dir, "c_+1_r3_0000000000000003.wav", 400, 200);

        mark_used(&old);
        let evicted = enforce_limit(&dir, 800, None);
        assert_eq!(evicted.files, 1);
        assert_eq!(names(&dir), ["a_+1_r3_0000000000000001.wav", "c_+1_r3_0000000000000003.wav"], "b 最久没用，该删的是它");

        // 上限小到一个文件都放不下：别的都删，刚渲染的留着
        let evicted = enforce_limit(&dir, 100, Some(&fresh));
        assert_eq!(evicted.remaining, CacheUsage { files: 1, bytes: 400 });
        assert_eq!(names(&dir), ["c_+1_r3_0000000000000003.wav"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_partial_files_are_swept_but_a_running_render_is_left_alone() {
        let dir = scratch("parts");
        cached(&dir, "a_+1_r3_0000000000000001.wav.123-0.part", 100, 2 * 3600);
        cached(&dir, "b_+1_r3_0000000000000002.wav.123-1.part", 100, 5);
        enforce_limit(&dir, u64::MAX, None);
        assert_eq!(names(&dir), ["b_+1_r3_0000000000000002.wav.123-1.part"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_cache_shaped_names_count() {
        for name in ["001_+2_r3_5e21053ad29212e7.wav", "song_-6_35c4089bccca27e4.flac"] {
            assert!(is_cache_name(name), "{name}");
        }
        for name in ["notes.txt", "001.wav", "001_+2_r3_5e21053ad29212e7.wav.1-0.part", "x_5E21053AD29212E7.wav", "x_5e21053ad29212e.wav", "x_5e21053ad29212e7.mp3"] {
            assert!(!is_cache_name(name), "{name}");
        }
    }
}
