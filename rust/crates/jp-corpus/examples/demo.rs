//! 对着真实库跑一遍主要查询，并计时。
//!
//!     cargo run -p jp-corpus --example demo --release -- 夜

use std::time::Instant;

use anyhow::Result;
use jp_corpus::{Corpus, KwicQuery, MatchField};

fn main() -> Result<()> {
    let keyword = std::env::args().nth(1).unwrap_or_else(|| "夜".to_string());
    let db = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join("corpus.db");

    let t0 = Instant::now();
    let corpus = Corpus::open(&db)?;
    corpus.check_schema()?;
    println!("打开库 {:?}", t0.elapsed());

    let t = Instant::now();
    let overview = corpus.overview()?;
    println!(
        "\n── Corpus Overview ({:?}) ──\n  曲目 {} · 专辑 {} · 人物 {}（演唱 {} / 作曲 {} / 作词 {} / 编曲 {}）\
         \n  歌词 {} 行 · token {} · 词汇 {}",
        t.elapsed(),
        overview.tracks, overview.albums, overview.people, overview.performers,
        overview.composers, overview.lyricists, overview.arrangers,
        overview.lyric_lines, overview.tokens, overview.vocabulary
    );

    let t = Instant::now();
    let hits = corpus.kwic(&KwicQuery {
        keywords: vec![keyword.clone()],
        field: MatchField::Lemma,
        cross_line: false,
        ..Default::default()
    })?;
    println!("\n── KWIC「{keyword}」{} 条 ({:?}) ──", hits.len(), t.elapsed());
    for hit in hits.iter().take(6) {
        println!(
            "  {} - {}  [{}]\n      …{}〈{}〉{}…",
            hit.artist,
            hit.title,
            hit.time_sec.map(|s| format!("{s:.1}s")).unwrap_or_else(|| "无时间轴".into()),
            tail(&hit.left, 12),
            hit.keyword,
            head(&hit.right, 12)
        );
    }

    let t = Instant::now();
    let word = corpus.word_in_corpus(&keyword, None, 5)?;
    println!(
        "\n── Research Mode「{keyword}」({:?}) ──\n  出现 {} 次 · {} 首歌 · {} 位歌手 · {}~{}",
        t.elapsed(),
        word.occurrences, word.song_count, word.artist_count,
        word.first_year.as_deref().unwrap_or("?"),
        word.last_year.as_deref().unwrap_or("?")
    );
    for pc in word.pos_distribution.iter().take(4) {
        println!("    {:6} {}", pc.pos, pc.count);
    }

    let t = Instant::now();
    let found = corpus.search_lyrics(&keyword, 5)?;
    println!("\n── 全文检索（FTS5）{} 条 ({:?}) ──", found.len(), t.elapsed());
    for hit in found.iter().take(3) {
        println!("  {} - {}  {}", hit.artist, hit.title, hit.text);
    }

    let t = Instant::now();
    let albums = corpus.albums(None, 5)?;
    println!("\n── 专辑 ({:?}) ──", t.elapsed());
    for album in &albums {
        println!("  {:3} 首  {:6} {} — {}", album.track_count, album.year, album.album_artist, album.title);
    }

    if let Some(top) = corpus.people_by_role("composer", 1)?.first() {
        let t = Instant::now();
        let edges = corpus.collaborators(top.id, 5)?;
        println!("\n── {} 的合作者 ({:?}) ──", top.name, t.elapsed());
        for e in &edges {
            println!("  {:3} 首同曲  {}  [{}]", e.shared_tracks, e.name, e.roles.join(","));
        }
    }
    Ok(())
}

fn tail(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
