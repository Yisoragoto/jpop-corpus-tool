//! 拿真实音频文件跑的播放测试。
//!
//! 单元测试只验证了状态管理；这里验证**整条链路真的在动**：
//! 解码 → WSOLA → tap → 输出，以及 seek / 循环 / 倍速。
//!
//! 测试期间音量置 0，不会真的出声。tap 挂在 Player 的音量控制**之前**，
//! 所以静音不影响采样流。
//!
//! 没有声卡或找不到音频文件时整体跳过。

use std::path::PathBuf;
use std::time::Duration;

use jp_audio::{AudioEngine, PlayState, SpectrumAnalyzer};

fn project_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .to_path_buf()
}

/// 从曲库里找一个能用的音频文件。
fn sample_audio() -> Option<PathBuf> {
    let audio_dir = project_root().join("raw/audio");
    if !audio_dir.is_dir() {
        eprintln!("跳过：找不到 {}", audio_dir.display());
        return None;
    }
    fn walk(dir: &std::path::Path, depth: usize) -> Option<PathBuf> {
        if depth == 0 {
            return None;
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = walk(&path, depth - 1) {
                    return Some(found);
                }
            } else if matches!(
                path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(),
                Some("flac" | "mp3" | "wav" | "m4a" | "ogg")
            ) {
                return Some(path);
            }
        }
        None
    }
    let found = walk(&audio_dir, 3);
    if found.is_none() {
        eprintln!("跳过：{} 下没有音频文件", audio_dir.display());
    }
    found
}

/// 静音的引擎 + 一个真实文件。缺任何一样就跳过。
fn loaded_engine() -> Option<(AudioEngine, PathBuf)> {
    let path = sample_audio()?;
    let engine = match AudioEngine::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("跳过：{err}");
            return None;
        }
    };
    engine.set_volume(0.0); // 测试不该真的出声
    if let Err(err) = engine.load(&path, "test-song") {
        eprintln!("跳过：加载失败 {err}");
        return None;
    }
    Some((engine, path))
}

macro_rules! setup {
    () => {
        match loaded_engine() {
            Some(pair) => pair,
            None => return,
        }
    };
}

/// 等到条件成立或超时。轮询而不是死等，因为播放是异步的。
fn wait_until(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    check()
}

#[test]
fn loading_a_real_file_starts_playing() {
    let (engine, path) = setup!();
    let state = engine.state();
    assert_eq!(state.song_id, "test-song");
    assert_eq!(state.play_state, PlayState::Playing);
    // 解码器能给出时长的话应当是个正数
    if let Some(duration) = state.duration_sec {
        assert!(duration > 0.0, "时长应当为正: {duration} ({})", path.display());
    }
}

#[test]
fn position_advances_during_playback() {
    let (engine, _) = setup!();
    assert!(
        wait_until(Duration::from_secs(3), || engine.state().position_sec > 0.05),
        "播放中位置应当推进，实际停在 {}",
        engine.state().position_sec
    );
}

#[test]
fn pause_stops_the_clock() {
    let (engine, _) = setup!();
    assert!(wait_until(Duration::from_secs(3), || engine
        .state()
        .position_sec
        > 0.05));
    engine.pause();
    assert_eq!(engine.state().play_state, PlayState::Paused);

    let before = engine.state().position_sec;
    std::thread::sleep(Duration::from_millis(300));
    let after = engine.state().position_sec;
    assert!(
        (after - before).abs() < 0.05,
        "暂停后位置不该继续走: {before} → {after}"
    );
}

#[test]
fn seek_moves_the_position() {
    let (engine, _) = setup!();
    assert!(wait_until(Duration::from_secs(3), || engine
        .state()
        .position_sec
        > 0.05));
    engine.seek(20.0).expect("跳转应当成功");
    assert!(
        wait_until(Duration::from_secs(2), || {
            let pos = engine.state().position_sec;
            (18.0..30.0).contains(&pos)
        }),
        "跳转后位置应当在目标附近，实际 {}",
        engine.state().position_sec
    );
}

#[test]
fn tap_receives_real_samples() {
    let (engine, _) = setup!();
    // 等到攒够一个窗口
    assert!(
        wait_until(Duration::from_secs(5), || {
            engine.samples(1024).iter().any(|&s| s.abs() > 1e-6)
        }),
        "tap 应当收到非零样本——链路没通"
    );
}

#[test]
fn spectrum_responds_to_real_audio() {
    let (engine, _) = setup!();
    let mut analyzer = SpectrumAnalyzer::new(1024, 48, 44_100);
    assert!(
        wait_until(Duration::from_secs(5), || {
            let spectrum = analyzer.analyze(&engine.samples(1024));
            spectrum.iter().any(|&v| v > 0.05)
        }),
        "真实音频应当产生非零频谱"
    );
}

#[test]
fn rate_change_takes_effect_without_reloading() {
    let (engine, _) = setup!();
    assert!(wait_until(Duration::from_secs(3), || engine
        .state()
        .position_sec
        > 0.05));
    engine.set_rate(1.5);
    assert_eq!(engine.state().rate, 1.5);
    // 改倍速不该中断播放
    assert_eq!(engine.state().play_state, PlayState::Playing);
    assert!(
        wait_until(Duration::from_secs(2), || engine.samples(512).iter().any(|&s| s.abs() > 1e-6)),
        "变速后仍应有样本流出"
    );
}

#[test]
fn loop_region_sends_playback_back() {
    let (engine, _) = setup!();
    // 设一个很短的循环区间，然后跳到接近末尾，看它有没有被拉回起点
    assert!(engine.set_loop(Some((5.0, 6.0))));
    engine.seek(5.8).expect("跳转应当成功");

    assert!(
        wait_until(Duration::from_secs(4), || {
            let pos = engine.state().position_sec;
            pos < 5.8
        }),
        "越过区间末尾后应当被拉回起点，实际停在 {}",
        engine.state().position_sec
    );
}

#[test]
fn changing_track_clears_the_loop() {
    let (engine, path) = setup!();
    assert!(engine.set_loop(Some((5.0, 10.0))));
    assert!(engine.state().loop_region.is_some());
    // 上一首的句子边界对新歌没有意义
    engine.load(&path, "another-song").expect("重新加载应当成功");
    assert!(engine.state().loop_region.is_none());
    assert_eq!(engine.state().song_id, "another-song");
}

#[test]
fn stop_resets_to_empty() {
    let (engine, _) = setup!();
    engine.stop();
    assert_eq!(engine.state().play_state, PlayState::Empty);
    assert!(engine.state().loop_region.is_none());
}
