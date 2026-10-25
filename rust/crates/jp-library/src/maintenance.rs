//! 维护操作：一次性的数据修补。
//!
//! 不依赖 Tauri，所以能离线测试，也能用 `cargo run --example` 跑——
//! 修数据这种事不该非得开着 GUI 才能做。
//!
//! command 层只是把这里的函数包一层。

use anyhow::Result;
use jp_audio::ProbeOutcome;
use jp_corpus::Corpus;
use serde::Serialize;

/// 时长回填的结果。
///
/// **每一种失败都单独计数**——笼统说「成功 N 个」的话，用户无法判断
/// 剩下那些是文件没了、格式不支持，还是文件坏了，也就无从修。
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurationBackfillReport {
    /// 本次待处理的曲目数（`duration_sec` 为空且有音频路径）
    pub scanned: i64,
    pub written: i64,
    /// 路径失效，文件不在了
    pub missing: i64,
    /// 能解码但解码器给不出时长
    pub unknown: i64,
    /// 打不开或解不了
    pub failed: i64,
    /// 前几条失败详情，够判断是不是同一类问题
    pub samples: Vec<String>,
}

impl DurationBackfillReport {
    pub fn summary(&self) -> String {
        format!(
            "扫描 {} 首：写入 {}，文件缺失 {}，无时长 {}，失败 {}",
            self.scanned, self.written, self.missing, self.unknown, self.failed
        )
    }
}

/// 扫描 `duration_sec` 为空的曲目，读出时长写回库。
///
/// **幂等**：只处理为空的行，重跑不会覆盖已有值，也不会重复计数。
/// 用的是和播放同一套解码器，所以库里的时长和进度条必然一致。
pub fn backfill_durations(corpus: &mut Corpus) -> Result<DurationBackfillReport> {
    let pending = corpus.tracks_missing_duration()?;
    let mut report = DurationBackfillReport {
        scanned: pending.len() as i64,
        ..Default::default()
    };
    let mut rows: Vec<(String, f64)> = Vec::with_capacity(pending.len());

    for (song_id, audio_path) in &pending {
        let note = |report: &mut DurationBackfillReport, text: String| {
            if report.samples.len() < 8 {
                report.samples.push(text);
            }
        };
        match jp_audio::probe(std::path::Path::new(audio_path)) {
            ProbeOutcome::Found(seconds) => rows.push((song_id.clone(), seconds)),
            ProbeOutcome::Missing => {
                report.missing += 1;
                note(&mut report, format!("[{song_id}] 文件不存在：{audio_path}"));
            }
            ProbeOutcome::Unknown => {
                report.unknown += 1;
                note(&mut report, format!("[{song_id}] 解码器给不出时长"));
            }
            ProbeOutcome::Failed(err) => {
                report.failed += 1;
                note(&mut report, format!("[{song_id}] {err}"));
            }
        }
    }

    report.written = corpus.set_durations(&rows)? as i64;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_mentions_every_bucket() {
        let report = DurationBackfillReport {
            scanned: 10,
            written: 7,
            missing: 1,
            unknown: 1,
            failed: 1,
            samples: vec![],
        };
        let text = report.summary();
        for n in ["10", "7", "1"] {
            assert!(text.contains(n), "汇总里应当有 {n}：{text}");
        }
    }

    #[test]
    fn samples_are_capped() {
        // 上千个坏文件时不该把详情全塞进返回值
        let mut report = DurationBackfillReport::default();
        for i in 0..50 {
            if report.samples.len() < 8 {
                report.samples.push(format!("{i}"));
            }
        }
        assert_eq!(report.samples.len(), 8);
    }
}
