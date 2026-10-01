//! 端到端验证收听历史链路：真播一首歌，看它有没有进 play_history。
//!
//!     cargo run -p jp-app --example verify_history
//!
//! 这条链路跨了三层（引擎 → tracker → 库），单元测试各测各的，
//! 只有真跑一遍才知道接没接上。

use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .to_path_buf();

    let state = jp_app_lib::state::AppState::new(&root)?;
    let Some(engine) = state.audio() else {
        println!("跳过：没有可用的输出设备");
        return Ok(());
    };

    let before = state.corpus().recently_played(50)?.len();
    println!("回放前 recently_played: {before} 条");

    // 找一首音频文件真实存在的
    let track = state
        .corpus()
        .tracks(200)?
        .into_iter()
        .find(|t| !t.audio_path.is_empty() && std::path::Path::new(&t.audio_path).exists())
        .ok_or_else(|| anyhow::anyhow!("曲库里没有音频文件真实存在的曲目"))?;
    println!("播放：{} - {}", track.artist, track.title);

    engine.set_volume(0.0); // 不出声
    engine.load(std::path::Path::new(&track.audio_path), &track.id)?;

    // 听 6 秒（阈值是 5 秒），每 100ms tick 一次，和前端轮询一致
    for _ in 0..60 {
        state.tick();
        std::thread::sleep(Duration::from_millis(100));
    }
    engine.stop();
    state.tick(); // 停止这一拍会把会话结掉并落库

    let after = state.corpus().recently_played(50)?;
    println!("回放后 recently_played: {} 条", after.len());
    let hit = after.iter().find(|t| t.song_id == track.id);
    match hit {
        Some(t) => println!(
            "  ✓ 记录成功：{} 播放 {} 次，累计 {:.1}s",
            t.title, t.play_count, t.total_listened_sec
        ),
        None => anyhow::bail!("链路没通：听了 6 秒却没有记录"),
    }
    Ok(())
}
