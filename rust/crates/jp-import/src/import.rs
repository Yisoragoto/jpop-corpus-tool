//! 执行计划：把 `New` 的条目真正写进库。
//!
//! ## 一首歌一个事务
//!
//! 不是整批一个事务。第 150 首失败时，前 149 首应该留在库里，
//! 重跑扫描会把它们判成「已在库中」，从第 150 首接着来。整批一个事务
//! 的话一次失败全部回滚，两百首重来一遍。
//!
//! ## 会写哪些表
//!
//! | 表 | 内容 |
//! |---|---|
//! | `songs` | 曲目本身。tag 原值直接写，不做「美化」 |
//! | `utterances` | 歌词行 |
//! | `utterances_fts` | 全文索引。**外部内容表，没有触发器**，必须手动同步 |
//! | `tokens` | 分词。`pos` 列存的是 UPOS，不是日语词性 |
//! | `people` / `track_credits` | 演唱者 + 从 LRC 解析的作词作曲编曲 |
//! | `albums` | 专辑，并回填 `songs.album_id` |
//!
//! ## 已知缺口
//!
//! `tokens.dep` / `tokens.head` 留空。那是 GiNZA 依存分析的产物，
//! sudachi.rs 只做分词，没有依存分析。实测这两列全库 100% 填满但
//! **只写不读**——除了 `scripts/04_export_processed_from_db.py` 那个
//! 导出脚本，Rust 侧、前端、gui.py 的查询都没引用过。

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::filename::matching_key;
use crate::plan::{ImportPlan, PlannedTrack};
use crate::scan::ScannedTrack;

/// SQLite 侧生成时间戳，格式和 Python 的 `_now()` 一致
/// （`2026-09-07T13:04:35+00:00`）。省掉一个 chrono 依赖。
pub(crate) const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%S+00:00','now')";

/// 一首歌的处置结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Outcome {
    // 结构体变体的字段名要单独改：枚举上的 rename_all 只管变体名。
    #[serde(rename_all = "camelCase")]
    Imported {
        lyric_lines: usize,
        tokens: usize,
        credits: usize,
        /// 删歌再导入时套回去的分词校正行数。见 `jp_corpus::corrections`。
        corrections_restored: usize,
    },
    /// 写到一半出错，这首已回滚。**其余的歌不受影响。**
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedTrack {
    pub song_id: String,
    pub path: String,
    pub title: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub tracks: Vec<ImportedTrack>,
}

impl ImportReport {
    pub fn imported(&self) -> usize {
        self.tracks
            .iter()
            .filter(|t| matches!(t.outcome, Outcome::Imported { .. }))
            .count()
    }

    pub fn failed(&self) -> usize {
        self.tracks.len() - self.imported()
    }

    pub fn lyric_lines(&self) -> usize {
        self.tracks
            .iter()
            .map(|t| match t.outcome {
                Outcome::Imported { lyric_lines, .. } => lyric_lines,
                _ => 0,
            })
            .sum()
    }

    pub fn tokens(&self) -> usize {
        self.tracks
            .iter()
            .map(|t| match t.outcome {
                Outcome::Imported { tokens, .. } => tokens,
                _ => 0,
            })
            .sum()
    }

    /// 这一批里一共套回去几行分词校正。
    pub fn corrections_restored(&self) -> usize {
        self.tracks
            .iter()
            .map(|t| match t.outcome {
                Outcome::Imported {
                    corrections_restored,
                    ..
                } => corrections_restored,
                _ => 0,
            })
            .sum()
    }

    /// 失败的条目。报告要能直接告诉用户「哪几首没进去、为什么」，
    /// 而不是只给一个总数。
    pub fn failures(&self) -> impl Iterator<Item = &ImportedTrack> {
        self.tracks
            .iter()
            .filter(|t| matches!(t.outcome, Outcome::Failed { .. }))
    }
}

/// 执行计划。`analyzer` 为 `None` 时只导曲目和歌词，不分词——
/// 分词是这里最慢的一步，先把歌导进去、之后再补是合理的用法。
pub fn execute(
    conn: &mut Connection,
    plan: &ImportPlan,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<ImportReport> {
    execute_with_progress(conn, plan, analyzer, |_, _, _| {})
}

/// 带进度回调的版本。回调参数是 (刚做完的这条, 第几条, 总共几条)。
pub fn execute_with_progress(
    conn: &mut Connection,
    plan: &ImportPlan,
    analyzer: Option<&jp_tokenizer::Analyzer>,
    mut on_progress: impl FnMut(&ImportedTrack, usize, usize),
) -> Result<ImportReport> {
    let todo: Vec<&PlannedTrack> = plan.to_import().collect();
    let total = todo.len();
    let mut report = ImportReport::default();

    for (index, item) in todo.into_iter().enumerate() {
        let song_id = match &item.action {
            crate::plan::Action::New { song_id } => song_id.clone(),
            // to_import() 只放 New，走不到这里
            _ => continue,
        };

        let tx = conn.transaction().context("开不了事务")?;
        let outcome = match import_one(&tx, &item.track, &song_id, analyzer) {
            Ok(stats) => match tx.commit() {
                Ok(()) => Outcome::Imported {
                    lyric_lines: stats.lyric_lines,
                    tokens: stats.tokens,
                    credits: stats.credits,
                    corrections_restored: stats.corrections_restored,
                },
                Err(err) => Outcome::Failed {
                    error: format!("提交失败：{err}"),
                },
            },
            Err(err) => {
                // tx 析构即回滚，这首歌一行都不会留下
                Outcome::Failed {
                    error: format!("{err:#}"),
                }
            }
        };

        let track = ImportedTrack {
            song_id,
            path: item.track.path.clone(),
            title: item.track.title().to_string(),
            outcome,
        };
        on_progress(&track, index + 1, total);
        report.tracks.push(track);
    }
    Ok(report)
}

#[derive(Default)]
struct Stats {
    lyric_lines: usize,
    tokens: usize,
    credits: usize,
    corrections_restored: usize,
}

fn import_one(
    tx: &Transaction,
    track: &ScannedTrack,
    song_id: &str,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Stats> {
    let mut stats = Stats::default();

    // ── songs ──
    // tag 原值直接落库，一个字都不改。需求：永远不覆盖用户文件里的
    // 原始 metadata；这里连「顺手清理一下」都不做。
    tx.execute(
        "INSERT INTO songs \
         (id, title, artist, year, album, genre, audio_path, corpus_type, source_file, duration_sec) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'song', ?8, ?9)",
        params![
            song_id,
            track.title(),
            track.artist(),
            track.tag_year,
            track.album(),
            track.tag_genre,
            track.path,
            track.lyrics_path,
            track.duration_sec,
        ],
    )
    .with_context(|| format!("写 songs 失败（id={song_id}）"))?;

    // ── 演唱者 ──
    // 只按斜杠拆，和 Python 的 split_artists(slash_only=True) 一致。
    // 顿号和 × 会误伤乐队名（「Mrs. GREEN APPLE」这类）。
    for (position, name) in split_slash(track.artist()).into_iter().enumerate() {
        if let Some(person_id) = get_or_create_person(tx, &name)? {
            stats.credits += add_credit(tx, song_id, person_id, "performer", position, "library")?;
        }
    }

    // ── 专辑 ──
    link_album(tx, song_id, track.album(), track.artist(), &track.tag_year)?;

    // ── 歌词 ──
    if let Some(path) = &track.lyrics_path {
        let parsed = crate::lrc::parse_file(std::path::Path::new(path))
            .with_context(|| format!("读歌词失败：{path}"))?;

        for (line_idx, line) in parsed.lines.iter().enumerate() {
            tx.execute(
                "INSERT INTO utterances (song_id, line_idx, time_sec, text) VALUES (?1,?2,?3,?4)",
                params![song_id, line_idx as i64, line.time_sec, line.text],
            )?;
            let utterance_id = tx.last_insert_rowid();
            stats.lyric_lines += 1;

            // utterances_fts 是 content=utterances 的外部内容表，**没有触发器**。
            // 漏掉这一步，歌能存进去但全文检索永远搜不到。
            tx.execute(
                "INSERT INTO utterances_fts (rowid, text) VALUES (?1, ?2)",
                params![utterance_id, line.text],
            )?;

            if let Some(analyzer) = analyzer {
                let tokens = analyzer
                    .analyze(&line.text)
                    .with_context(|| format!("分词失败：{}", line.text))?;
                // 空白不入语料。GiNZA 建库时就没收，实测拿 6,427 句
                // 两边都有的歌词比过：**只**差在空白 token 上，滤掉之后
                // 逐词一致率 100.00%。不滤的话词频统计里会混进空格。
                let meaningful = tokens.iter().filter(|t| !t.surface.trim().is_empty());
                for (token_idx, token) in meaningful.enumerate() {
                    // pos 列存 UPOS——库里存量就是 NOUN/VERB 这套，
                    // 写日语词性进去会让整个语料的词性统计对不上。
                    // dep/head 留空：sudachi.rs 没有依存分析。
                    tx.execute(
                        "INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) \
                         VALUES (?1,?2,?3,?4,?5)",
                        params![
                            utterance_id,
                            token_idx as i64,
                            token.surface,
                            token.lemma,
                            token.upos
                        ],
                    )?;
                    stats.tokens += 1;
                }
            }
        }

        // ── 分词校正 ──
        // 这首歌以前被删过、删之前用户校正过分词的话，校正行还留在表里，套回去。
        // 规则和 Python 导入线程一致，见 `jp_corpus::corrections::restore_for_song`。
        //
        // 没分词（analyzer 为 None）时不套：只给个别行写 token 会得到一首「半分词」的歌。
        // 校正行留在表里不会丢。
        if analyzer.is_some() {
            stats.corrections_restored =
                jp_corpus::corrections::restore_for_song(tx, song_id, track.artist(), track.title())
                    .context("恢复分词校正失败")?;
        }

        // ── 作词 / 作曲 / 编曲 ──
        for (position, credit) in parsed.credits.iter().enumerate() {
            if let Some(person_id) = get_or_create_person(tx, &credit.name)? {
                stats.credits += add_credit(tx, song_id, person_id, credit.role, position, "lrc")?;
            }
        }
    }

    Ok(stats)
}

/// 按归一化名取人，没有就建。
///
/// 归一化键必须和 Python 的 `matching_key` 逐字一致，否则同一个人
/// 会在库里出现两份。
///
/// 接 `&Connection` 而不是 `&Transaction`：刮削那边（`jp-app`）
/// 也要用同一套去重逻辑，它手上只有一个连接。`Transaction` 解引用
/// 就是 `Connection`，导入这边照常传。
pub fn get_or_create_person(tx: &Connection, name: &str) -> Result<Option<i64>> {
    let clean = name.trim();
    let key = matching_key(clean);
    if key.is_empty() {
        return Ok(None);
    }
    if let Some(id) = tx
        .query_row(
            "SELECT id FROM people WHERE normalized_name=?1",
            params![key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(Some(id));
    }
    tx.execute(
        &format!(
            "INSERT INTO people (name, normalized_name, sort_name, created_at) \
             VALUES (?1, ?2, ?1, {NOW})"
        ),
        params![clean, key],
    )?;
    Ok(Some(tx.last_insert_rowid()))
}

/// 写一条信用，返回新增了几条（0 或 1）。
///
/// **已经存在的一律不动**——不管来源是 lrc 还是 manual。
/// 自动流程不该冲掉用户的修正，这是需求里明写的。
pub fn add_credit(
    tx: &Connection,
    song_id: &str,
    person_id: i64,
    role: &str,
    position: usize,
    source: &str,
) -> Result<usize> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT source FROM track_credits WHERE song_id=?1 AND person_id=?2 AND role=?3",
            params![song_id, person_id, role],
            |row| row.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Ok(0);
    }
    tx.execute(
        "INSERT INTO track_credits (song_id, person_id, role, position, source) \
         VALUES (?1,?2,?3,?4,?5)",
        params![song_id, person_id, role, position as i64, source],
    )?;
    Ok(1)
}

/// 找或建专辑，并回填 `songs.album_id`。
pub(crate) fn link_album(tx: &Connection, song_id: &str, album: &str, artist: &str, year: &str) -> Result<()> {
    let title = album.trim();
    if title.is_empty() {
        return Ok(()); // 没有专辑名就不建，空专辑比没专辑更麻烦
    }
    // 同名专辑挂在不同歌手名下是两张不同的专辑
    let album_artist = split_slash(artist).into_iter().next().unwrap_or_default();
    let title_key = matching_key(title);
    let artist_key = matching_key(&album_artist);

    let album_id = match tx
        .query_row(
            "SELECT id FROM albums WHERE normalized_title=?1 AND normalized_album_artist=?2",
            params![title_key, artist_key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
    {
        Some(id) => id,
        None => {
            tx.execute(
                &format!(
                    "INSERT INTO albums (title, normalized_title, album_artist, \
                     normalized_album_artist, year, created_at) VALUES (?1,?2,?3,?4,?5,{NOW})"
                ),
                params![title, title_key, album_artist, artist_key, year],
            )?;
            tx.last_insert_rowid()
        }
    };
    tx.execute(
        "UPDATE songs SET album_id=?1 WHERE id=?2",
        params![album_id, song_id],
    )?;
    Ok(())
}

/// 按斜杠拆歌手串。拆不出就原样一个。
pub(crate) fn split_slash(name: &str) -> Vec<String> {
    let parts: Vec<String> = name
        .split(['/', '／'])
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trimmed.to_string()]
        }
    } else {
        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{LibraryIndex, plan};

    /// 建一个和真库同构的空库。schema 抄自 `scripts/migrate_db.py`——
    /// 这里只为测试，生产库仍由 Python 侧建表。
    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE songs (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, artist TEXT NOT NULL,
                year TEXT, album TEXT, genre TEXT, audio_path TEXT,
                corpus_type TEXT NOT NULL DEFAULT 'song', source_file TEXT,
                cover_path TEXT, duration_sec REAL, album_id INTEGER);
             CREATE TABLE utterances (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                song_id TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
                line_idx INTEGER NOT NULL, time_sec REAL, text TEXT NOT NULL,
                chapter_id INTEGER);
             CREATE VIRTUAL TABLE utterances_fts USING fts5(
                text, content=utterances, content_rowid=id, tokenize='trigram');
             CREATE TABLE tokens (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                utterance_id INTEGER NOT NULL REFERENCES utterances(id),
                token_idx INTEGER NOT NULL, surface TEXT NOT NULL,
                lemma TEXT NOT NULL, pos TEXT, dep TEXT, head TEXT);
             CREATE TABLE people (
                id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                normalized_name TEXT NOT NULL UNIQUE, sort_name TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT '');
             CREATE TABLE track_credits (
                song_id TEXT NOT NULL, person_id INTEGER NOT NULL, role TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0, source TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (song_id, person_id, role));
             CREATE TABLE albums (
                id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL,
                normalized_title TEXT NOT NULL, album_artist TEXT NOT NULL DEFAULT '',
                normalized_album_artist TEXT NOT NULL DEFAULT '',
                year TEXT NOT NULL DEFAULT '', artwork_path TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT '',
                UNIQUE (normalized_title, normalized_album_artist));",
        )
        .unwrap();
        conn
    }

    /// 写一个带歌词的临时目录，返回 (音频路径, lrc 路径)。
    fn with_lyrics(dir: &std::path::Path, stem: &str, lrc_body: &str) -> (String, String) {
        let audio = dir.join(format!("{stem}.flac"));
        let lrc = dir.join(format!("{stem}.lrc"));
        std::fs::write(&audio, b"not really audio").unwrap();
        std::fs::write(&lrc, lrc_body).unwrap();
        (audio.display().to_string(), lrc.display().to_string())
    }

    fn track(path: &str, title: &str, artist: &str) -> ScannedTrack {
        ScannedTrack {
            path: path.into(),
            tag_title: title.into(),
            tag_artist: artist.into(),
            ..Default::default()
        }
    }

    fn run(conn: &mut Connection, tracks: &[ScannedTrack]) -> ImportReport {
        let p = plan(tracks, &LibraryIndex::empty());
        execute(conn, &p, None).unwrap()
    }

    #[test]
    fn an_imported_outcome_uses_the_field_names_the_frontend_reads() {
        // ImportPage.tsx 读的是 outcome.lyricLines / outcome.tokens。
        // 枚举上的 rename_all 只改变体名（kind 的取值），不改结构体变体里的字段名。
        let json = serde_json::to_value(Outcome::Imported {
            lyric_lines: 3,
            tokens: 9,
            credits: 1,
            corrections_restored: 2,
        })
        .unwrap();
        assert_eq!(json["kind"], "imported");
        assert_eq!(json["lyricLines"], 3, "实际序列化出来的是：{json}");
        assert_eq!(json["correctionsRestored"], 2, "实际序列化出来的是：{json}");
    }

    #[test]
    fn a_song_lands_in_the_songs_table_with_tag_values_untouched() {
        let mut conn = test_db();
        let mut t = track("D:/a/1.flac", "夜行", "ヨルシカ");
        t.tag_album = "盗作".into();
        t.tag_year = "2020".into();
        t.tag_genre = "J-Pop".into();
        t.duration_sec = Some(233.5);
        let report = run(&mut conn, &[t]);

        assert_eq!(report.imported(), 1);
        let row: (String, String, String, String, String, Option<f64>) = conn
            .query_row(
                "SELECT title, artist, album, year, genre, duration_sec FROM songs WHERE id='001'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "夜行".into(),
                "ヨルシカ".into(),
                "盗作".into(),
                "2020".into(),
                "J-Pop".into(),
                Some(233.5)
            )
        );
    }

    #[test]
    fn lyrics_and_the_fts_index_stay_in_sync() {
        let dir = tempdir();
        let (audio, _) = with_lyrics(
            &dir,
            "song",
            "[00:10.00]夜が明けるまで\n[00:15.00]君を待っている\n",
        );
        let mut conn = test_db();
        let mut t = track(&audio, "夜行", "ヨルシカ");
        t.lyrics_path = Some(audio.replace(".flac", ".lrc"));
        run(&mut conn, &[t]);

        let lines: i64 = conn
            .query_row("SELECT COUNT(*) FROM utterances", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lines, 2);

        // 关键：外部内容表没有触发器，漏同步的话这里会是 0
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH '夜が明'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1, "全文索引没同步，导进来的歌搜不到");
    }

    #[test]
    fn credits_from_the_lrc_become_people_and_track_credits() {
        let dir = tempdir();
        let (audio, _) = with_lyrics(
            &dir,
            "song",
            "作詞 : n-buna\n作曲 : n-buna\n[00:10.00]歌詞\n",
        );
        let mut conn = test_db();
        let mut t = track(&audio, "夜行", "ヨルシカ");
        t.lyrics_path = Some(audio.replace(".flac", ".lrc"));
        run(&mut conn, &[t]);

        let roles: Vec<String> = conn
            .prepare("SELECT role FROM track_credits WHERE song_id='001' ORDER BY role")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(roles, vec!["composer", "lyricist", "performer"]);

        // 同一个人两个角色只建一份人
        let people: i64 = conn
            .query_row("SELECT COUNT(*) FROM people WHERE name='n-buna'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(people, 1);
    }

    #[test]
    fn the_same_person_written_differently_is_one_person() {
        let mut conn = test_db();
        // 全角空格 vs 无空格
        let t1 = track("D:/a/1.flac", "曲一", "山口　一郎");
        let t2 = track("D:/a/2.flac", "曲二", "山口一郎");
        run(&mut conn, &[t1, t2]);

        let people: i64 = conn
            .query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))
            .unwrap();
        assert_eq!(people, 1, "归一化名相同就该是同一个人");
    }

    #[test]
    fn a_manual_credit_is_never_overwritten() {
        let mut conn = test_db();
        run(&mut conn, &[track("D:/a/1.flac", "曲一", "ヨルシカ")]);
        conn.execute(
            "UPDATE track_credits SET source='manual' WHERE song_id='001'",
            [],
        )
        .unwrap();

        // 再导一首同歌手的，不该把上一条改回 library
        let index = LibraryIndex::from_tracks(&[crate::plan::ExistingTrack {
            id: "001".into(),
            audio_path: "D:/a/1.flac".into(),
            title: "曲一".into(),
            artist: "ヨルシカ".into(),
        }]);
        let p = plan(&[track("D:/a/2.flac", "曲二", "ヨルシカ")], &index);
        execute(&mut conn, &p, None).unwrap();

        let source: String = conn
            .query_row(
                "SELECT source FROM track_credits WHERE song_id='001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "manual");
    }

    #[test]
    fn songs_sharing_an_album_share_one_album_row() {
        let mut conn = test_db();
        let mut a = track("D:/a/1.flac", "曲一", "サカナクション");
        let mut b = track("D:/a/2.flac", "曲二", "サカナクション");
        a.tag_album = "834.194".into();
        b.tag_album = "834.194".into();
        run(&mut conn, &[a, b]);

        let albums: i64 = conn
            .query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))
            .unwrap();
        assert_eq!(albums, 1);
        let linked: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM songs WHERE album_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, 2);
        // 归一化后的标题要和 Python 一致
        let key: String = conn
            .query_row("SELECT normalized_title FROM albums", [], |r| r.get(0))
            .unwrap();
        assert_eq!(key, "834194");
    }

    #[test]
    fn one_bad_song_does_not_take_down_the_rest() {
        let mut conn = test_db();
        let good = track("D:/a/1.flac", "曲一", "ヨルシカ");
        let mut bad = track("D:/a/2.flac", "曲二", "ヨルシカ");
        // 指向一个不存在的歌词文件，读取会失败
        bad.lyrics_path = Some("X:/nope/nope.lrc".into());
        let also_good = track("D:/a/3.flac", "曲三", "ヨルシカ");

        let report = run(&mut conn, &[good, bad, also_good]);
        assert_eq!(report.imported(), 2);
        assert_eq!(report.failed(), 1);

        // 失败那首一行都没留下
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM songs WHERE id='002'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "失败的歌必须整条回滚");
        // 而且报告能说清是哪首、为什么
        let failure = report.failures().next().unwrap();
        assert_eq!(failure.title, "曲二");
        assert!(failure.outcome != Outcome::Imported {
            lyric_lines: 0,
            tokens: 0,
            credits: 0, corrections_restored: 0,
        });
    }

    #[test]
    fn importing_twice_does_not_duplicate() {
        let mut conn = test_db();
        let t = track("D:/a/1.flac", "夜行", "ヨルシカ");
        run(&mut conn, std::slice::from_ref(&t));

        // 第二轮：先重建索引，计划会把它判成已在库中
        let index = LibraryIndex::from_tracks(&[crate::plan::ExistingTrack {
            id: "001".into(),
            audio_path: "D:/a/1.flac".into(),
            title: "夜行".into(),
            artist: "ヨルシカ".into(),
        }]);
        let p = plan(&[t], &index);
        let report = execute(&mut conn, &p, None).unwrap();

        assert_eq!(report.tracks.len(), 0, "已在库中的不该再写一遍");
        let songs: i64 = conn
            .query_row("SELECT COUNT(*) FROM songs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(songs, 1);
    }

    #[test]
    fn an_artist_pair_splits_on_slash_only() {
        let mut conn = test_db();
        // 斜杠拆开
        run(&mut conn, &[track("D:/a/1.flac", "合作", "ヨルシカ / n-buna")]);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);

        // 逗号和 & 不拆——名字本身可能带
        let mut conn = test_db();
        run(&mut conn, &[track("D:/a/1.flac", "曲", "Mrs. GREEN APPLE")]);
        let name: String = conn
            .query_row("SELECT name FROM people", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "Mrs. GREEN APPLE");
    }

    #[test]
    fn whitespace_is_not_a_word() {
        // 需要真词典，机器上没有就跳过
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap()
            .to_path_buf();
        let Some((res, dict)) = jp_tokenizer::locate_sudachipy(&root) else {
            return;
        };
        let analyzer = jp_tokenizer::Analyzer::from_sudachipy(&res, &dict).unwrap();

        let dir = tempdir();
        let (audio, _) = with_lyrics(&dir, "ws", "[00:01.00]逃げるよ 逃げるよ\n");
        let mut conn = test_db();
        let mut t = track(&audio, "曲", "誰か");
        t.lyrics_path = Some(audio.replace(".flac", ".lrc"));
        let p = plan(&[t], &LibraryIndex::empty());
        execute(&mut conn, &p, Some(&analyzer)).unwrap();

        let blanks: i64 = conn
            .query_row("SELECT COUNT(*) FROM tokens WHERE TRIM(surface)=''", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(blanks, 0, "空白不该进语料");

        // 滤掉之后 token_idx 仍要连续，否则和库里存量的编号对不上
        let idx: Vec<i64> = conn
            .prepare("SELECT token_idx FROM tokens ORDER BY token_idx")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(idx, (0..idx.len() as i64).collect::<Vec<_>>());
    }

    #[test]
    fn a_song_without_lyrics_still_imports() {
        let mut conn = test_db();
        let report = run(&mut conn, &[track("D:/a/1.flac", "无歌词", "某人")]);
        assert_eq!(report.imported(), 1);
        assert_eq!(report.lyric_lines(), 0);
    }

    fn tempdir() -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "jp-import-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }
}
