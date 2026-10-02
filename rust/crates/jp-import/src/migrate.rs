//! 把另一个语料库目录的数据搬进当前这个库。
//!
//! 场景：0.1.x 的库在项目目录里（`D:\jp_corpus`），新装的程序默认用
//! `%LOCALAPPDATA%\JPOP Corpus Tool`，而新库里已经导了几首歌、没有词典。
//! 「切换目录」只能二选一，这里要的是**合并**：旧库的歌、歌词、分词、署名、
//! 收听记录、词典表都搬过来，新库里已经有的那几首不动。
//!
//! ## 怎么搬
//!
//! `ATTACH` 源库，然后基本都在 SQL 里做——2,500,735 行词条逐行读进 Rust 再写回去
//! 没有意义。**只有去重和分配新 id 在 Rust 里**：那两步要用 `jp_normalize` 的
//! 归一化规则，SQL 写不出来。
//!
//! id 会变：两个库的歌都是 `001` 开头，直接搬必撞。所以先在 Rust 里按
//! [`crate::LibraryIndex`] 算出「旧 id → 新 id」，建一张临时表，剩下的表全部
//! 按这张表改写外键。歌词行的 id 也一样重排（`tokens`、`token_corrections`
//! 按它挂）——**按 `ORDER BY id` 顺序分配连号**，这样映射在 SQL 里就能算出来。
//!
//! ## 不搬什么
//!
//! * **音频文件不复制**，`songs.audio_path` 是绝对路径，照旧指向原来的位置。
//!   搬几十 GB 音频不是这个功能该做的事；源目录还在，路径就还通。
//! * 封面、歌手照片、`.lrc` 由应用层复制（文件名里有 song_id，要跟着改），
//!   这一层只返回 id 映射。
//! * `sqlite_sequence` 不碰，SQLite 自己维护。
//!
//! ## 幂等
//!
//! 再跑一次，已经搬过的歌会按「音频路径相同」判成重复而跳过，所以不会出现两份。
//! 词典表只在目标为空时搬（`INSERT OR IGNORE`，有主键的表天然不会重复）。

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::plan::{ExistingTrack, LibraryIndex};

/// 搬什么。
#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateOptions {
    /// 0.1.x 存在 corpus.db 里的词典表（`dict_terms`、`yomitan_*`、`jlpt_cache`）。
    /// 制卡的释义和统计页的 JLPT 等级都读它们。
    pub dictionaries: bool,
    /// 收听记录和收藏
    pub history: bool,
}

/// 预览：搬过去会发生什么。**只读**。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigratePlan {
    /// 源库里的歌
    pub source_songs: usize,
    /// 会搬过来的
    pub new_songs: usize,
    /// 已经在当前库里的（按音频路径或「歌手 + 曲名」判的）
    pub duplicate_songs: usize,
    pub lyric_lines: usize,
    pub tokens: usize,
    pub people: usize,
    pub albums: usize,
    pub credits: usize,
    pub plays: usize,
    pub favorites: usize,
    pub corrections: usize,
    /// 0.1.x 的词典词条数（`dict_terms`）
    pub dict_terms: usize,
    /// JLPT 等级缓存的行数
    pub jlpt_rows: usize,
    /// 当前库里已经有词典表了吗——有的话不搬，免得混成两份
    pub target_has_dictionaries: bool,
    /// 前几首要搬的歌，给人看一眼
    pub samples: Vec<String>,
}

/// 实际搬完的数字 + id 映射（应用层按它复制封面和 .lrc）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateReport {
    pub songs: usize,
    pub lyric_lines: usize,
    pub tokens: usize,
    pub people: usize,
    pub albums: usize,
    pub credits: usize,
    pub plays: usize,
    pub favorites: usize,
    pub corrections: usize,
    pub dict_terms: usize,
    pub jlpt_rows: usize,
    pub skipped: usize,
    /// (旧 id, 新 id)，按旧 id 排序
    pub id_map: Vec<(String, String)>,
}

/// 挂上源库。只读打开——搬数据不该把源库改坏。
fn attach(target: &Connection, source_db: &Path) -> Result<()> {
    anyhow::ensure!(source_db.is_file(), "找不到 {}", source_db.display());
    let uri = format!(
        "file:{}?mode=ro",
        source_db.to_string_lossy().replace('\\', "/")
    );
    target
        .execute("ATTACH DATABASE ?1 AS src", params![uri])
        .with_context(|| format!("挂载源库失败：{}", source_db.display()))?;
    Ok(())
}

fn detach(target: &Connection) {
    let _ = target.execute("DETACH DATABASE src", []);
}

fn count(conn: &Connection, sql: &str) -> Result<usize> {
    Ok(conn.query_row(sql, [], |row| row.get::<_, i64>(0))? as usize)
}

fn table_exists(conn: &Connection, schema: &str, name: &str) -> Result<bool> {
    let sql = format!("SELECT COUNT(*) FROM {schema}.sqlite_master WHERE type='table' AND name=?1");
    Ok(conn.query_row(&sql, params![name], |row| row.get::<_, i64>(0))? > 0)
}

/// 源库的歌，和当前库比对之后分成「要搬的」和「已经有的」。
fn split_songs(target: &Connection) -> Result<(Vec<(String, String, String)>, usize)> {
    // 当前库的索引：音频路径 + 「歌手 + 曲名」，顺带给出下一个可用 id
    let existing: Vec<ExistingTrack> = {
        let mut stmt = target.prepare("SELECT id, COALESCE(audio_path,''), title, artist FROM main.songs")?;
        let rows = stmt.query_map([], |row| {
            Ok(ExistingTrack {
                id: row.get(0)?,
                audio_path: row.get(1)?,
                title: row.get(2)?,
                artist: row.get(3)?,
            })
        })?;
        rows.collect::<Result<_, _>>()?
    };
    let mut index = LibraryIndex::from_tracks(&existing);

    let source: Vec<ExistingTrack> = {
        let mut stmt = target.prepare(
            "SELECT id, COALESCE(audio_path,''), title, artist FROM src.songs ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExistingTrack {
                id: row.get(0)?,
                audio_path: row.get(1)?,
                title: row.get(2)?,
                artist: row.get(3)?,
            })
        })?;
        rows.collect::<Result<_, _>>()?
    };

    let mut moving = Vec::new();
    let mut duplicates = 0usize;
    for track in &source {
        // 路径相同是最硬的证据；其次才是同歌手同曲名
        if index.song_for_path(&track.audio_path).is_some()
            || index.song_for_identity(&track.artist, &track.title).is_some()
        {
            duplicates += 1;
            continue;
        }
        let new_id = index.take_next_id();
        index.remember(&new_id, &track.audio_path, &track.artist, &track.title);
        moving.push((
            track.id.clone(),
            new_id,
            format!("{} — {}", track.artist, track.title),
        ));
    }
    Ok((moving, duplicates))
}

/// 把 (旧 id, 新 id) 放进临时表，后面的 SQL 全靠它改写外键。
fn write_song_map(target: &Connection, moving: &[(String, String, String)]) -> Result<()> {
    target.execute_batch(
        "DROP TABLE IF EXISTS temp.song_map;
         CREATE TEMP TABLE song_map (old_id TEXT PRIMARY KEY, new_id TEXT NOT NULL);",
    )?;
    let mut stmt = target.prepare("INSERT INTO temp.song_map (old_id, new_id) VALUES (?1, ?2)")?;
    for (old, new, _) in moving {
        stmt.execute(params![old, new])?;
    }
    Ok(())
}

/// 预览。不写任何东西。
pub fn plan(target: &Connection, source_db: &Path) -> Result<MigratePlan> {
    attach(target, source_db)?;
    let result = (|| -> Result<MigratePlan> {
        let (moving, duplicates) = split_songs(target)?;
        write_song_map(target, &moving)?;

        let has_dict_src = table_exists(target, "src", "dict_terms")?;
        let has_dict_target = table_exists(target, "main", "dict_terms")?
            && count(target, "SELECT COUNT(*) FROM main.dict_terms")? > 0;

        Ok(MigratePlan {
            source_songs: moving.len() + duplicates,
            new_songs: moving.len(),
            duplicate_songs: duplicates,
            lyric_lines: count(
                target,
                "SELECT COUNT(*) FROM src.utterances WHERE song_id IN (SELECT old_id FROM temp.song_map)",
            )?,
            tokens: count(
                target,
                "SELECT COUNT(*) FROM src.tokens t JOIN src.utterances u ON u.id = t.utterance_id \
                 WHERE u.song_id IN (SELECT old_id FROM temp.song_map)",
            )?,
            people: count(
                target,
                "SELECT COUNT(*) FROM src.people WHERE normalized_name NOT IN \
                 (SELECT normalized_name FROM main.people)",
            )?,
            albums: count(
                target,
                "SELECT COUNT(*) FROM src.albums a WHERE NOT EXISTS (SELECT 1 FROM main.albums b \
                 WHERE b.normalized_title = a.normalized_title \
                 AND b.normalized_album_artist = a.normalized_album_artist)",
            )?,
            credits: count(
                target,
                "SELECT COUNT(*) FROM src.track_credits WHERE song_id IN (SELECT old_id FROM temp.song_map)",
            )?,
            plays: count(
                target,
                "SELECT COUNT(*) FROM src.play_history WHERE song_id IN (SELECT old_id FROM temp.song_map)",
            )?,
            favorites: count(
                target,
                "SELECT COUNT(*) FROM src.favorites WHERE entity_type='song' \
                 AND entity_id IN (SELECT old_id FROM temp.song_map)",
            )?,
            corrections: count(
                target,
                "SELECT COUNT(*) FROM src.token_corrections c JOIN src.utterances u ON u.id = c.utterance_id \
                 WHERE u.song_id IN (SELECT old_id FROM temp.song_map)",
            )?,
            dict_terms: if has_dict_src {
                count(target, "SELECT COUNT(*) FROM src.dict_terms")?
            } else {
                0
            },
            jlpt_rows: if table_exists(target, "src", "jlpt_cache")? {
                count(target, "SELECT COUNT(*) FROM src.jlpt_cache")?
            } else {
                0
            },
            target_has_dictionaries: has_dict_target,
            samples: moving.iter().take(8).map(|(_, _, label)| label.clone()).collect(),
        })
    })();
    detach(target);
    result
}

/// 真的搬。**整件事一个事务**：中途出错就当什么都没发生。
pub fn run(
    target: &mut Connection,
    source_db: &Path,
    options: MigrateOptions,
) -> Result<MigrateReport> {
    attach(target, source_db)?;
    let result = (|| -> Result<MigrateReport> {
        let tx = target.transaction()?;
        let (moving, duplicates) = split_songs(&tx)?;
        write_song_map(&tx, &moving)?;
        let mut report = MigrateReport {
            skipped: duplicates,
            id_map: moving.iter().map(|(a, b, _)| (a.clone(), b.clone())).collect(),
            ..Default::default()
        };

        // ── 人 ──
        // 归一化名是唯一键，OR IGNORE 之后按它 join 就是「旧 id → 新 id」
        report.people = tx.execute(
            "INSERT OR IGNORE INTO main.people (name, normalized_name, sort_name, created_at) \
             SELECT name, normalized_name, sort_name, created_at FROM src.people",
            [],
        )?;

        // ── 专辑 ──
        report.albums = tx.execute(
            "INSERT OR IGNORE INTO main.albums \
             (title, normalized_title, album_artist, normalized_album_artist, year, artwork_path, created_at) \
             SELECT a.title, a.normalized_title, a.album_artist, a.normalized_album_artist, a.year, '', a.created_at \
             FROM src.albums a WHERE a.normalized_title IN \
             (SELECT normalized_title FROM src.songs s JOIN src.albums b ON b.id = s.album_id \
              WHERE s.id IN (SELECT old_id FROM temp.song_map))",
            [],
        )?;

        // ── 歌 ──
        // cover_path / source_file 先留空：那两个文件由应用层按新 id 复制过去再写回
        report.songs = tx.execute(
            "INSERT INTO main.songs \
             (id, title, artist, year, album, genre, audio_path, corpus_type, source_file, cover_path, duration_sec, album_id) \
             SELECT m.new_id, s.title, s.artist, s.year, s.album, s.genre, s.audio_path, s.corpus_type, \
                    NULL, NULL, s.duration_sec, \
                    (SELECT b.id FROM main.albums b JOIN src.albums a ON a.id = s.album_id \
                     WHERE b.normalized_title = a.normalized_title \
                       AND b.normalized_album_artist = a.normalized_album_artist) \
             FROM src.songs s JOIN temp.song_map m ON m.old_id = s.id",
            [],
        )?;

        // ── 歌词行 ──
        // 新 id 按 `ORDER BY id` 连号分配，这样 tokens / 校正那两张表在 SQL 里就能跟着改
        tx.execute_batch(
            "DROP TABLE IF EXISTS temp.utt_map;
             CREATE TEMP TABLE utt_map AS
             SELECT u.id AS old_id,
                    (SELECT COALESCE(MAX(id), 0) FROM main.utterances) + ROW_NUMBER() OVER (ORDER BY u.id) AS new_id
             FROM src.utterances u WHERE u.song_id IN (SELECT old_id FROM temp.song_map);",
        )?;
        report.lyric_lines = tx.execute(
            "INSERT INTO main.utterances (id, song_id, line_idx, time_sec, text, chapter_id) \
             SELECT um.new_id, sm.new_id, u.line_idx, u.time_sec, u.text, NULL \
             FROM src.utterances u \
             JOIN temp.utt_map um ON um.old_id = u.id \
             JOIN temp.song_map sm ON sm.old_id = u.song_id",
            [],
        )?;
        // 外部内容的全文索引没有触发器，漏了这一步搬过来的歌一句都搜不到
        tx.execute(
            "INSERT INTO main.utterances_fts (rowid, text) \
             SELECT um.new_id, u.text FROM src.utterances u JOIN temp.utt_map um ON um.old_id = u.id",
            [],
        )?;

        // ── 分词 ──
        report.tokens = tx.execute(
            "INSERT INTO main.tokens (utterance_id, token_idx, surface, lemma, pos, dep, head) \
             SELECT um.new_id, t.token_idx, t.surface, t.lemma, t.pos, t.dep, t.head \
             FROM src.tokens t JOIN temp.utt_map um ON um.old_id = t.utterance_id",
            [],
        )?;

        // ── 分词校正 ──
        report.corrections = tx.execute(
            "INSERT OR IGNORE INTO main.token_corrections \
             (utterance_id, tokens_json, orig_json, text, song_artist, song_title, updated_at) \
             SELECT um.new_id, c.tokens_json, c.orig_json, c.text, c.song_artist, c.song_title, c.updated_at \
             FROM src.token_corrections c JOIN temp.utt_map um ON um.old_id = c.utterance_id",
            [],
        )?;

        // ── 署名 ──
        report.credits = tx.execute(
            "INSERT OR IGNORE INTO main.track_credits (song_id, person_id, role, position, source) \
             SELECT sm.new_id, p2.id, c.role, c.position, c.source \
             FROM src.track_credits c \
             JOIN temp.song_map sm ON sm.old_id = c.song_id \
             JOIN src.people p1 ON p1.id = c.person_id \
             JOIN main.people p2 ON p2.normalized_name = p1.normalized_name",
            [],
        )?;

        // ── 歌手资料 ──
        if table_exists(&tx, "src", "artists")? && table_exists(&tx, "main", "artists")? {
            // image_path 先留空，照片由应用层复制过去再写回
            tx.execute(
                "INSERT OR IGNORE INTO main.artists (name, image_path, artist_type, country, formed, updated_at) \
                 SELECT name, '', artist_type, country, formed, updated_at FROM src.artists",
                [],
            )?;
        }

        // ── 收听记录与收藏 ──
        if options.history {
            report.plays = tx.execute(
                "INSERT INTO main.play_history (song_id, played_at, listened_sec, position_sec, completed, source) \
                 SELECT sm.new_id, h.played_at, h.listened_sec, h.position_sec, h.completed, h.source \
                 FROM src.play_history h JOIN temp.song_map sm ON sm.old_id = h.song_id",
                [],
            )?;
            report.favorites = tx.execute(
                "INSERT OR IGNORE INTO main.favorites (entity_type, entity_id, created_at) \
                 SELECT 'song', sm.new_id, f.created_at \
                 FROM src.favorites f JOIN temp.song_map sm ON sm.old_id = f.entity_id \
                 WHERE f.entity_type = 'song'",
                [],
            )?;
        }

        // ── 0.1.x 的词典表 ──
        // 制卡的释义（jp-anki/src/dict.rs）和统计页的 JLPT 等级都读这几张，
        // 不搬的话新库里制卡只有词形没有释义。
        if options.dictionaries {
            for table in [
                "dict_registry",
                "dict_terms",
                "yomitan_freq",
                "yomitan_pitch",
                "yomitan_zh",
                "yomitan_meta",
                "jlpt_cache",
            ] {
                if !table_exists(&tx, "src", table)? || !table_exists(&tx, "main", table)? {
                    continue;
                }
                let moved = tx.execute(
                    &format!("INSERT OR IGNORE INTO main.{table} SELECT * FROM src.{table}"),
                    [],
                )?;
                match table {
                    "dict_terms" => report.dict_terms = moved,
                    "jlpt_cache" => report.jlpt_rows = moved,
                    _ => {}
                }
            }
        }

        tx.commit()?;
        Ok(report)
    })();
    detach(target);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两个同构的空库：表结构取自 jp-corpus 的 schema.sql（真库导出来的那一份）
    fn library(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        jp_corpus::Corpus::ensure_schema_on(&conn).unwrap();
        conn
    }

    fn dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jp-migrate-{}-{:?}-{tag}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 往库里塞一首歌：一行歌词、一个词、一个作词
    fn add_song(conn: &Connection, id: &str, artist: &str, title: &str, audio: &str) {
        conn.execute(
            "INSERT INTO songs (id, title, artist, audio_path, corpus_type, duration_sec) \
             VALUES (?1, ?2, ?3, ?4, 'song', 100.0)",
            params![id, title, artist, audio],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO utterances (song_id, line_idx, time_sec, text) VALUES (?1, 0, 1.0, ?2)",
            params![id, format!("{title}の歌詞")],
        )
        .unwrap();
        let utt = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO utterances_fts (rowid, text) VALUES (?1, ?2)",
            params![utt, format!("{title}の歌詞")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) \
             VALUES (?1, 0, '歌詞', '歌詞', 'NOUN')",
            params![utt],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO people (name, normalized_name) VALUES (?1, ?2)",
            params![artist, artist.to_lowercase()],
        )
        .unwrap();
        let person: i64 = conn
            .query_row(
                "SELECT id FROM people WHERE normalized_name=?1",
                params![artist.to_lowercase()],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO track_credits (song_id, person_id, role, position, source) \
             VALUES (?1, ?2, 'performer', 0, 'library')",
            params![id, person],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO play_history (song_id, played_at, listened_sec, position_sec, completed, source) \
             VALUES (?1, '2026-01-01', 60.0, 60.0, 1, 'app')",
            params![id],
        )
        .unwrap();
    }

    #[test]
    fn songs_move_over_with_new_ids_and_everything_that_hangs_off_them() {
        let dir = dir("basic");
        let source_path = dir.join("src.db");
        let src = library(&source_path);
        add_song(&src, "001", "ヨルシカ", "夜行", "D:/jp_corpus/raw/audio/a.flac");
        add_song(&src, "002", "サカナクション", "新宝島", "D:/jp_corpus/raw/audio/b.flac");
        drop(src);

        let mut target = library(&dir.join("target.db"));
        add_song(&target, "001", "ずっと真夜中でいいのに。", "勘ぐれい", "E:/music/c.flac");

        let preview = plan(&target, &source_path).unwrap();
        assert_eq!(preview.source_songs, 2);
        assert_eq!(preview.new_songs, 2);
        assert_eq!(preview.duplicate_songs, 0);
        assert_eq!(preview.lyric_lines, 2);
        assert_eq!(preview.tokens, 2);
        assert_eq!(preview.samples.len(), 2);
        // 预览是只读的
        assert_eq!(
            count(&target, "SELECT COUNT(*) FROM songs").unwrap(),
            1,
            "预览不该写库"
        );

        let report = run(
            &mut target,
            &source_path,
            MigrateOptions { dictionaries: false, history: true },
        )
        .unwrap();
        assert_eq!(report.songs, 2);
        assert_eq!(report.lyric_lines, 2);
        assert_eq!(report.tokens, 2);
        assert_eq!(report.credits, 2);
        assert_eq!(report.plays, 2);
        // 新 id 接着原来的排，不撞车
        assert_eq!(
            report.id_map,
            vec![("001".to_string(), "002".to_string()), ("002".into(), "003".into())]
        );

        let ids: Vec<String> = target
            .prepare("SELECT id FROM songs ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(ids, vec!["001", "002", "003"]);

        // 歌词跟着新 id 走，而且搜得到——外部内容表漏同步的话这里是 0
        let lines: usize = count(&target, "SELECT COUNT(*) FROM utterances WHERE song_id='002'").unwrap();
        assert_eq!(lines, 1);
        // 三元组分词，查询串至少三个字——两个字的 '夜行' 本来就匹配不上
        let hits: usize = count(
            &target,
            "SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH '夜行の'",
        )
        .unwrap();
        assert_eq!(hits, 1, "搬过来的歌词搜不到");

        // 分词挂在新的歌词行上
        let tokens: usize = count(
            &target,
            "SELECT COUNT(*) FROM tokens t JOIN utterances u ON u.id=t.utterance_id WHERE u.song_id='003'",
        )
        .unwrap();
        assert_eq!(tokens, 1);
    }

    #[test]
    fn a_song_that_is_already_here_is_left_alone() {
        let dir = dir("dupe");
        let source_path = dir.join("src.db");
        let src = library(&source_path);
        add_song(&src, "001", "ヨルシカ", "夜行", "D:/music/a.flac");
        add_song(&src, "002", "ヨルシカ", "花に亡霊", "D:/music/b.flac");
        drop(src);

        let mut target = library(&dir.join("target.db"));
        // 同一个音频文件，id 不同——路径是最硬的证据
        add_song(&target, "001", "ヨルシカ", "夜行（別タイトル）", "D:/music/a.flac");
        // 同歌手同曲名，文件换了位置
        add_song(&target, "002", "ヨルシカ", "花に亡霊", "E:/elsewhere/b.flac");

        let preview = plan(&target, &source_path).unwrap();
        assert_eq!(preview.new_songs, 0);
        assert_eq!(preview.duplicate_songs, 2);

        let report = run(&mut target, &source_path, MigrateOptions::default()).unwrap();
        assert_eq!(report.songs, 0);
        assert_eq!(report.skipped, 2);
        assert_eq!(count(&target, "SELECT COUNT(*) FROM songs").unwrap(), 2);
    }

    /// 搬两次不该出现两份——第二次全部按「音频路径相同」跳过
    #[test]
    fn running_it_twice_changes_nothing_the_second_time() {
        let dir = dir("twice");
        let source_path = dir.join("src.db");
        let src = library(&source_path);
        add_song(&src, "001", "ヨルシカ", "夜行", "D:/music/a.flac");
        drop(src);

        let mut target = library(&dir.join("target.db"));
        let first = run(&mut target, &source_path, MigrateOptions::default()).unwrap();
        assert_eq!(first.songs, 1);
        let second = run(&mut target, &source_path, MigrateOptions::default()).unwrap();
        assert_eq!(second.songs, 0);
        assert_eq!(second.skipped, 1);
        assert_eq!(count(&target, "SELECT COUNT(*) FROM songs").unwrap(), 1);
        assert_eq!(count(&target, "SELECT COUNT(*) FROM utterances").unwrap(), 1);
        assert_eq!(count(&target, "SELECT COUNT(*) FROM tokens").unwrap(), 1);
    }

    /// 词典表是另一回事：那是整张表搬，不跟歌走
    #[test]
    fn the_legacy_dictionary_tables_come_over_when_asked() {
        let dir = dir("dict");
        let source_path = dir.join("src.db");
        let src = library(&source_path);
        src.execute(
            "INSERT INTO dict_terms (term, reading, dict_name, defs_json) VALUES ('夜','よる','D','[]')",
            [],
        )
        .unwrap();
        src.execute("INSERT INTO jlpt_cache (lemma, level) VALUES ('夜','N5')", []).unwrap();
        drop(src);

        let mut target = library(&dir.join("target.db"));
        let preview = plan(&target, &source_path).unwrap();
        assert_eq!(preview.dict_terms, 1);
        assert_eq!(preview.jlpt_rows, 1);
        assert!(!preview.target_has_dictionaries);

        // 不要就不搬
        run(&mut target, &source_path, MigrateOptions { dictionaries: false, history: false }).unwrap();
        assert_eq!(count(&target, "SELECT COUNT(*) FROM dict_terms").unwrap(), 0);

        let report = run(
            &mut target,
            &source_path,
            MigrateOptions { dictionaries: true, history: false },
        )
        .unwrap();
        assert_eq!(report.dict_terms, 1);
        assert_eq!(report.jlpt_rows, 1);
        assert_eq!(count(&target, "SELECT COUNT(*) FROM dict_terms").unwrap(), 1);
    }
}
