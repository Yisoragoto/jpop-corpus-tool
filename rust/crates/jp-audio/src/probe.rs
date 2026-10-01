//! 读音频文件的时长，不播放、不占用输出设备。
//!
//! 为什么需要它：`songs.duration_sec` 全库为空，导致
//!
//! * 播放条的进度条只能靠解码器给的时长，换歌时会闪
//! * 分析页的「总时长」显示不出来
//! * 刮削时无法用时长区分原版 / TV size / Extended（最可靠的那个信号）
//!
//! 放在 `jp-audio` 而不是单独一个 crate，是因为它和播放共用同一套解码器——
//! 两边读出来的时长必然一致，不会出现「进度条和库里对不上」。
//!
//! **不需要音频设备**，所以 CI 也能跑。

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use rodio::{Decoder, Source};

/// 探测单个文件的时长。
///
/// 返回 `Ok(None)` 表示文件能解码但解码器给不出时长（某些流式 mp3
/// 没有 Xing 头就属于这种）。这和「文件坏了」是两回事，调用方要能区分：
/// 前者应当跳过，后者应当报告。
pub fn probe_duration(path: &Path) -> Result<Option<f64>> {
    let file = File::open(path).with_context(|| format!("打不开 {}", path.display()))?;
    let decoder =
        Decoder::try_from(file).with_context(|| format!("解码失败 {}", path.display()))?;
    Ok(decoder.total_duration().map(|d| d.as_secs_f64()))
}

/// 一次探测的结果。批量回填要如实汇报每一种情况，
/// 而不是笼统地说「成功 N 个」。
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeOutcome {
    /// 读到时长
    Found(f64),
    /// 能解码但解码器给不出时长
    Unknown,
    /// 文件不存在
    Missing,
    /// 打不开或解不了
    Failed(String),
}

impl ProbeOutcome {
    pub fn duration(&self) -> Option<f64> {
        match self {
            Self::Found(d) => Some(*d),
            _ => None,
        }
    }
}

/// 探测一个文件，把所有失败情况都归到 `ProbeOutcome` 而不是抛出去。
///
/// 批量回填时一个坏文件不该中断整轮，所以这里不返回 `Result`。
pub fn probe(path: &Path) -> ProbeOutcome {
    if !path.exists() {
        return ProbeOutcome::Missing;
    }
    match probe_duration(path) {
        Ok(Some(seconds)) if seconds.is_finite() && seconds > 0.0 => ProbeOutcome::Found(seconds),
        // 0 或 NaN 当作「读不出」而不是「时长为 0」——
        // 写个 0 进库比留空更糟，UI 会以为这首歌真的只有 0 秒
        Ok(_) => ProbeOutcome::Unknown,
        Err(err) => ProbeOutcome::Failed(err.to_string()),
    }
}

/// 秒 → `Duration`，负数和非有限值一律归零。
pub fn to_duration(seconds: f64) -> Duration {
    if seconds.is_finite() && seconds > 0.0 {
        Duration::from_secs_f64(seconds)
    } else {
        Duration::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_reported_as_missing_not_failed() {
        // 这两种要分开：文件被移走是常见情况，文件损坏是异常
        assert_eq!(probe(Path::new("完全不存在.flac")), ProbeOutcome::Missing);
    }

    #[test]
    fn a_non_audio_file_fails_cleanly() {
        let path = std::env::temp_dir().join("jp-audio-not-audio.txt");
        std::fs::write(&path, "这不是音频".as_bytes()).unwrap();
        let outcome = probe(&path);
        std::fs::remove_file(&path).ok();
        assert!(matches!(outcome, ProbeOutcome::Failed(_)), "实际是 {outcome:?}");
    }

    #[test]
    fn outcome_duration_only_unwraps_found() {
        assert_eq!(ProbeOutcome::Found(12.5).duration(), Some(12.5));
        assert_eq!(ProbeOutcome::Unknown.duration(), None);
        assert_eq!(ProbeOutcome::Missing.duration(), None);
        assert_eq!(ProbeOutcome::Failed("x".into()).duration(), None);
    }

    #[test]
    fn to_duration_rejects_nonsense() {
        assert_eq!(to_duration(f64::NAN), Duration::ZERO);
        assert_eq!(to_duration(-5.0), Duration::ZERO);
        assert_eq!(to_duration(0.0), Duration::ZERO);
        assert_eq!(to_duration(2.5), Duration::from_secs_f64(2.5));
    }
}
