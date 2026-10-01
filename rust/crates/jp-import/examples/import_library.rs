//! 端到端：往一个全新的空库里完整导入真实曲库，再和 Python 当初
//! 建的 `corpus.db` 对账。
//!
//!     cargo run --release -p jp-import --example import_library -- <目标库>
//!
//! 目标库必须是**已建好表、但没有数据**的库。别指向 corpus.db。
//!
//! 对账的意义：Python 侧当初用 SudachiPy + GiNZA 把这 209 首处理进库。
//! Rust 侧走完全独立的一条链路（lofty 读 tag、自己解析 LRC、sudachi.rs
//! 分词），结果应该对得上。对不上的地方要能逐条解释——
//! 「差不多」不算通过。

use std::path::Path;

fn main() -> anyhow::Result<()> {
    let target = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("用法: import_library <目标库路径>");
        std::process::exit(2)
    });
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();

    let mut conn = rusqlite::Connection::open(&target)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    let existing: i64 = conn.query_row("SELECT COUNT(*) FROM songs", [], |r| r.get(0))?;
    anyhow::ensure!(existing == 0, "目标库里已经有 {existing} 首歌，别往真库里导");

    // ── 分词器 ──
    let (resource_dir, system_dict) = jp_tokenizer::locate_sudachipy(&root)
        .ok_or_else(|| anyhow::anyhow!("找不到 SudachiPy 词典"))?;
    let analyzer = jp_tokenizer::Analyzer::from_sudachipy(&resource_dir, &system_dict)?;

    // ── 扫描 → 计划 ──
    let started = std::time::Instant::now();
    let tracks = jp_import::scan_dir(&root.join("raw/audio"), 6);
    let index = jp_import::LibraryIndex::empty();
    let plan = jp_import::plan(&tracks, &index);
    let summary = plan.summary();
    println!(
        "扫描 {} 个文件 → 计划新增 {}（跳过 {}，批内重复 {}）",
        tracks.len(),
        summary.new,
        summary.skipped,
        summary.duplicates_in_batch
    );

    // ── 执行 ──
    let mut last = std::time::Instant::now();
    let report = jp_import::execute_with_progress(
        &mut conn,
        &plan,
        Some(&analyzer),
        |_track, done, total| {
            if last.elapsed().as_millis() > 800 || done == total {
                println!("  {done}/{total}");
                last = std::time::Instant::now();
            }
        },
    )?;
    println!(
        "\n导入完成，用时 {:?}：{} 成功，{} 失败",
        started.elapsed(),
        report.imported(),
        report.failed()
    );
    println!("  歌词行 {}，token {}", report.lyric_lines(), report.tokens());
    for failure in report.failures() {
        println!("  ✗ [{}] {} — {:?}", failure.song_id, failure.title, failure.outcome);
    }

    // ── 和 Python 建的库对账 ──
    let reference = root.join("corpus.db");
    if reference.is_file() {
        println!("\n和 Python 建的库对账：");
        conn.execute("ATTACH DATABASE ?1 AS ref", rusqlite::params![
            reference.display().to_string()
        ])?;
        for (label, table) in [
            ("songs", "songs"),
            ("utterances", "utterances"),
            ("tokens", "tokens"),
            ("people", "people"),
            ("albums", "albums"),
            ("track_credits", "track_credits"),
        ] {
            let mine: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
            let theirs: i64 =
                conn.query_row(&format!("SELECT COUNT(*) FROM ref.{table}"), [], |r| r.get(0))?;
            let mark = if mine == theirs { "=" } else { "≠" };
            println!("  {label:<14} rust={mine:<7} python={theirs:<7} {mark}");
        }

        // 注意：从空库导入时 song_id 按扫描顺序重新分配，两边的 "001"
        // 不是同一首歌。必须按 audio_path 关联。
        //
        // 两首歌要单独摘出来，它们的差异已经查清：
        //   122 — Python 的正则要求时间戳带小数点，这首用的是 `[mm:ss:cc]`
        //         冒号变体，整首歌一行都没进库（Python=0）。
        //   079 — 每行两个时间戳，Python 只剥了第一个，第二个连同方括号
        //         被当成歌词正文存了进去，还多出 4 行纯时间戳的空行。
        let mut stmt = conn.prepare(
            "SELECT u.id, r.id FROM utterances u \
             JOIN songs s ON u.song_id = s.id \
             JOIN ref.songs rs ON rs.audio_path = s.audio_path \
             JOIN ref.utterances r ON r.song_id = rs.id AND r.line_idx = u.line_idx \
             WHERE s.audio_path NOT LIKE '%079.flac' AND s.audio_path NOT LIKE '%122.flac'",
        )?;
        let pairs: Vec<(i64, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;

        let mut surface_same = 0usize;
        let mut surface_diff = 0usize;
        let mut pos_same = 0usize;
        let mut examples: Vec<String> = Vec::new();
        for (mine_id, theirs_id) in &pairs {
            let a = token_row(&conn, "tokens", *mine_id)?;
            // GiNZA 把空白也当成词写进了库（U+3000 标 SYM，还有一个
            // U+00A0 被标成 NOUN），全库共 275 个。Rust 侧不收空白，
            // 比较时要把它们排掉，否则差异会被这些噪音淹掉。
            let b: Vec<(String, String)> = token_row(&conn, "ref.tokens", *theirs_id)?
                .into_iter()
                .filter(|t| !t.0.trim().is_empty())
                .collect();
            if a.iter().map(|t| &t.0).eq(b.iter().map(|t| &t.0)) {
                surface_same += 1;
                if a.iter().map(|t| &t.1).eq(b.iter().map(|t| &t.1)) {
                    pos_same += 1;
                }
            } else {
                surface_diff += 1;
                if examples.len() < 5 {
                    examples.push(format!(
                        "    rust={:?}\n    py  ={:?}",
                        a.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
                        b.iter().map(|t| t.0.as_str()).collect::<Vec<_>>()
                    ));
                }
            }
        }
        let total = surface_same + surface_diff;
        if total > 0 {
            println!(
                "  切分一致的行 : {surface_same}/{total} ({:.2}%)",
                surface_same as f64 / total as f64 * 100.0
            );
            println!(
                "  连词性也一致 : {pos_same}/{total} ({:.2}%)",
                pos_same as f64 / total as f64 * 100.0
            );
            for e in &examples {
                println!("{e}");
            }
        }
    }
    Ok(())
}

fn token_row(
    conn: &rusqlite::Connection,
    table: &str,
    utterance_id: i64,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT surface, COALESCE(pos,'') FROM {table} WHERE utterance_id=?1 ORDER BY token_idx"
    ))?;
    let rows = stmt
        .query_map(rusqlite::params![utterance_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
