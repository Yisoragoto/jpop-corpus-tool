//! 时长探测的交叉校验。
//!
//! 光验证「读到了一个数」是不够的——量纲错误（把采样数当毫秒、
//! 把毫秒当秒）照样能读出一个数，而且一眼看不出来。所以拿 mutagen
//! 的结果做基准逐条比对。
//!
//! 基准由 `python scripts/export_durations.py` 生成，
//! 缺文件时整组跳过。

use std::path::{Path, PathBuf};

use jp_audio::probe::{ProbeOutcome, probe};
use serde::Deserialize;

#[derive(Deserialize)]
struct Reference {
    tracks: Vec<RefTrack>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefTrack {
    song_id: String,
    path: String,
    duration_sec: f64,
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .to_path_buf()
}

fn reference() -> Option<Reference> {
    let path = project_root().join("benchmark/durations_mutagen.json");
    if !path.exists() {
        eprintln!(
            "跳过：缺 {}\n  先跑 python scripts/export_durations.py",
            path.display()
        );
        return None;
    }
    Some(serde_json::from_str(&std::fs::read_to_string(&path).ok()?).expect("基准 JSON 解析失败"))
}

macro_rules! reference_or_skip {
    () => {
        match reference() {
            Some(r) => r,
            None => return,
        }
    };
}

/// 允许的误差。不同库对「音频有多长」的定义会差最后一个不完整帧，
/// FLAC 一帧约 4096 样本 ≈ 93ms @ 44.1kHz，取 0.2 秒留余量。
const TOLERANCE_SEC: f64 = 0.2;

#[test]
fn rust_durations_match_mutagen() {
    let reference = reference_or_skip!();
    let mut compared = 0usize;
    let mut mismatched = Vec::new();
    let mut unreadable = Vec::new();

    for track in &reference.tracks {
        match probe(Path::new(&track.path)) {
            ProbeOutcome::Found(seconds) => {
                compared += 1;
                let delta = (seconds - track.duration_sec).abs();
                if delta > TOLERANCE_SEC {
                    mismatched.push(format!(
                        "[{}] Rust={seconds:.3} mutagen={:.3} 差 {delta:.3}s",
                        track.song_id, track.duration_sec
                    ));
                }
            }
            ProbeOutcome::Missing => { /* 文件被移走，不是 Rust 的问题 */ }
            other => unreadable.push(format!("[{}] {other:?}", track.song_id)),
        }
    }

    println!("比对 {compared}/{} 个文件", reference.tracks.len());
    assert!(compared > 0, "一个都没比对上——基准里的路径可能全失效了");
    assert!(
        unreadable.is_empty(),
        "{} 个文件 mutagen 读得出、Rust 读不出：\n  {}",
        unreadable.len(),
        unreadable.join("\n  ")
    );
    assert!(
        mismatched.is_empty(),
        "{} 个文件时长对不上（容差 {TOLERANCE_SEC}s）：\n  {}",
        mismatched.len(),
        mismatched.join("\n  ")
    );
}

#[test]
fn durations_are_plausible_for_pop_songs() {
    let reference = reference_or_skip!();
    // 量纲错了的话这条最先炸：秒当成毫秒会得到几十万，反过来会得到零点几
    for track in reference.tracks.iter().take(50) {
        let ProbeOutcome::Found(seconds) = probe(Path::new(&track.path)) else {
            continue;
        };
        assert!(
            (20.0..1800.0).contains(&seconds),
            "[{}] {seconds}s 不像一首歌的长度",
            track.song_id
        );
    }
}

#[test]
fn probing_is_repeatable() {
    let reference = reference_or_skip!();
    let Some(track) = reference.tracks.first() else {
        return;
    };
    let path = Path::new(&track.path);
    let first = probe(path);
    let second = probe(path);
    assert_eq!(first, second, "同一个文件两次探测结果应当相同");
}
