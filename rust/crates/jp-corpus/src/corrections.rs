//! 分词校正。
//!
//! 用户在 KWIC 里右键一行，把分错的词合并、拆开，或者改词元和词性。校正存进
//! `token_corrections`，同时直接改写 `tokens`，下次检索立刻生效。
//!
//! ## 和 Python 版共用一张表
//!
//! 表结构和 `legacy/scripts/migrate_db.py::_ensure_token_corrections` 逐字一致，JSON 形状和
//! `legacy/gui.py::_apply_token_correction` 一致（`surface` / `lemma` / `pos`）。
//! 任何一边存的校正，另一边都读得出来。
//!
//! ## 删歌再导入时恢复
//!
//! 删歌不会删校正：Python 的 `legacy/dialogs/song_manager.py::_delete` 只删 utterances、
//! tokens、songs。校正行会留下来，`utterance_id` 指向已经不存在的行。重新导入同一首歌时，
//! 按 `(歌手, 曲名)` 取出这些校正，逐行比对文本：先精确匹配，再用 NFKC + 去首尾空白兜底。
//! 对上了就把校正套回去，并把校正行改指向新的 `utterance_id`。
//! 规则照抄 `legacy/gui.py` 导入线程里的那段（`corr_exact` / `corr_norm`）。
//!
//! ## 和 Python 刻意不同的一处：撤销
//!
//! Python 的「重置为 GiNZA 原始」只是把原始分词填回表格，保存后仍是一条校正——
//! 下次导入会把**旧分词器**的结果强行套回去，哪怕新分词器已经分对了。
//! 这里的 [`revert`] 写回原始分词并**删掉校正行**：撤销就等于没校正过。

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

/// 校正里的一个词。JSON 键名和 Python 存的一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenEdit {
    pub surface: String,
    /// Python 读的时候是 `t.get('lemma', t['surface'])`，所以允许缺省
    #[serde(default)]
    pub lemma: String,
    #[serde(default)]
    pub pos: String,
}

/// 编辑器打开一行时要的全部东西。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionView {
    pub utterance_id: i64,
    pub song_id: String,
    pub artist: String,
    pub title: String,
    pub text: String,
    /// 当前生效的分词（校正过就是校正后的）
    pub tokens: Vec<TokenEdit>,
    /// 第一次校正之前的分词。没校正过时和 `tokens` 相同。
    pub original: Vec<TokenEdit>,
    pub corrected: bool,
}

/// 和 `legacy/scripts/migrate_db.py::_ensure_token_corrections` 逐字一致。
/// 只在第一次保存时用得上——没校正过的库不需要这张表。
const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS token_corrections (
        utterance_id INTEGER PRIMARY KEY,
        tokens_json  TEXT NOT NULL,
        orig_json    TEXT NOT NULL DEFAULT '[]',
        text         TEXT NOT NULL DEFAULT '',
        song_artist  TEXT NOT NULL DEFAULT '',
        song_title   TEXT NOT NULL DEFAULT '',
        updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE INDEX IF NOT EXISTS idx_token_corr_lookup
        ON token_corrections(song_artist, song_title, text);
";

/// 表在不在。`check_schema` 不要求它：从来没校正过的库照样能用。
pub(crate) fn table_exists(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master \
         WHERE type='table' AND name='token_corrections')",
        [],
        |r| r.get(0),
    )?)
}

/// 只经过 `legacy/gui.py::_ensure_token_corrections_table` 建的表没有 `updated_at`。
fn has_updated_at(conn: &Connection) -> Result<bool> {
    let mut stmt = conn.prepare("PRAGMA table_info(token_corrections)")?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    for name in names {
        if name? == "updated_at" {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 和 Python 编辑器 `_current_tokens` 同样的清理：去首尾空白，丢掉空词，
/// 词元缺省用表层形，词性缺省 NOUN。
fn normalize(tokens: &[TokenEdit]) -> Vec<TokenEdit> {
    tokens
        .iter()
        .filter_map(|t| {
            let surface = t.surface.trim();
            if surface.is_empty() {
                return None;
            }
            let lemma = t.lemma.trim();
            let pos = t.pos.trim();
            Some(TokenEdit {
                surface: surface.to_string(),
                lemma: if lemma.is_empty() { surface } else { lemma }.to_string(),
                pos: if pos.is_empty() { "NOUN" } else { pos }.to_string(),
            })
        })
        .collect()
}

fn parse(json: &str) -> Result<Vec<TokenEdit>> {
    let tokens: Vec<TokenEdit> =
        serde_json::from_str(json).context("校正数据不是合法的 JSON")?;
    Ok(normalize(&tokens))
}

fn tokens_in_db(conn: &Connection, utterance_id: i64) -> Result<Vec<TokenEdit>> {
    let mut stmt = conn.prepare(
        "SELECT surface, lemma, COALESCE(pos,'') FROM tokens \
         WHERE utterance_id=?1 ORDER BY token_idx",
    )?;
    let rows = stmt.query_map(params![utterance_id], |r| {
        Ok(TokenEdit {
            surface: r.get(0)?,
            lemma: r.get(1)?,
            pos: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn write_tokens(conn: &Connection, utterance_id: i64, tokens: &[TokenEdit]) -> Result<()> {
    conn.execute("DELETE FROM tokens WHERE utterance_id=?1", params![utterance_id])?;
    let mut stmt = conn.prepare(
        "INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) \
         VALUES (?1,?2,?3,?4,?5)",
    )?;
    for (idx, t) in tokens.iter().enumerate() {
        stmt.execute(params![utterance_id, idx as i64, t.surface, t.lemma, t.pos])?;
    }
    Ok(())
}

/// 取一行的分词给编辑器。行不存在返回 `None`。
pub fn load(conn: &Connection, utterance_id: i64) -> Result<Option<CorrectionView>> {
    let line = conn
        .query_row(
            "SELECT u.song_id, s.artist, s.title, u.text \
             FROM utterances u JOIN songs s ON s.id = u.song_id WHERE u.id = ?1",
            params![utterance_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((song_id, artist, title, text)) = line else {
        return Ok(None);
    };

    let in_db = tokens_in_db(conn, utterance_id)?;
    let stored = if table_exists(conn)? {
        conn.query_row(
            "SELECT tokens_json, orig_json FROM token_corrections WHERE utterance_id=?1",
            params![utterance_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    } else {
        None
    };

    let (tokens, original, corrected) = match stored {
        Some((tokens_json, orig_json)) => {
            // orig_json 为 '[]' 是老数据：Python 早期没存原始分词
            let original = match orig_json.trim() {
                "" | "[]" => in_db.clone(),
                raw => parse(raw)?,
            };
            (parse(&tokens_json)?, original, true)
        }
        None => (in_db.clone(), in_db, false),
    };

    Ok(Some(CorrectionView {
        utterance_id,
        song_id,
        artist,
        title,
        text,
        tokens,
        original,
        corrected,
    }))
}

/// 保存校正：改写 `tokens`，记下校正。
///
/// 原始分词只在**第一次**校正时记下；之后再改，「原始」仍是最早那份，
/// 撤销永远回到分词器给的结果，而不是上一次手改的结果。
pub fn save(conn: &Connection, utterance_id: i64, tokens: &[TokenEdit]) -> Result<()> {
    let tokens = normalize(tokens);
    ensure!(!tokens.is_empty(), "至少需要一个词");
    let view = load(conn, utterance_id)?
        .with_context(|| format!("找不到歌词行 {utterance_id}"))?;

    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(SCHEMA)?;
    let tokens_json = serde_json::to_string(&tokens)?;
    let orig_json = serde_json::to_string(&view.original)?;
    if has_updated_at(&tx)? {
        tx.execute(
            "INSERT OR REPLACE INTO token_corrections \
             (utterance_id, tokens_json, orig_json, text, song_artist, song_title, updated_at) \
             VALUES (?1,?2,?3,?4,?5,?6,CURRENT_TIMESTAMP)",
            params![utterance_id, tokens_json, orig_json, view.text, view.artist, view.title],
        )?;
    } else {
        tx.execute(
            "INSERT OR REPLACE INTO token_corrections \
             (utterance_id, tokens_json, orig_json, text, song_artist, song_title) \
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![utterance_id, tokens_json, orig_json, view.text, view.artist, view.title],
        )?;
    }
    write_tokens(&tx, utterance_id, &tokens)?;
    tx.commit()?;
    Ok(())
}

/// 撤销校正：写回第一次校正前的分词，删掉校正行。
/// 返回是否真的撤销了——没校正过的行什么都不做，返回 `false`。
pub fn revert(conn: &Connection, utterance_id: i64) -> Result<bool> {
    if !table_exists(conn)? {
        return Ok(false);
    }
    let Some(view) = load(conn, utterance_id)? else {
        return Ok(false);
    };
    if !view.corrected {
        return Ok(false);
    }
    let tx = conn.unchecked_transaction()?;
    write_tokens(&tx, utterance_id, &view.original)?;
    tx.execute(
        "DELETE FROM token_corrections WHERE utterance_id=?1",
        params![utterance_id],
    )?;
    tx.commit()?;
    Ok(true)
}

/// 这些行里哪些做过校正。KWIC 用来画 ✏。
pub(crate) fn corrected_among(conn: &Connection, utterance_ids: &[i64]) -> Result<HashSet<i64>> {
    let mut out = HashSet::new();
    if utterance_ids.is_empty() || !table_exists(conn)? {
        return Ok(out);
    }
    for chunk in utterance_ids.chunks(900) {
        let marks = vec!["?"; chunk.len()].join(",");
        let sql = format!("SELECT utterance_id FROM token_corrections WHERE utterance_id IN ({marks})");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk.iter()), |r| r.get::<_, i64>(0))?;
        for id in rows {
            out.insert(id?);
        }
    }
    Ok(out)
}

/// Python 的 `_norm`：NFKC 之后去首尾空白。
fn fold(text: &str) -> String {
    jp_normalize::nfkc(text).trim().to_string()
}

/// 一首歌重新导入之后，把留下来的校正套回去。返回套回去的行数。
///
/// 必须在歌词行和分词都写完之后调用，并且和它们在同一个事务里——恢复失败就整首回滚，
/// 不会留下一半校正、一半没校正的歌。**不自己开事务**，调用方手上已经有一个。
///
/// 和 Python 一致的几个细节，都有测试钉着：
/// - 同一句有好几条校正时后面的覆盖前面的。Python 用 dict 推导式建表，行的顺序是
///   `(song_artist, song_title, text)` 索引的顺序，同文本内按主键；这里用同样的 `ORDER BY`。
/// - 副歌重复的每一行都会套上校正，但校正行只挪到第一次出现的那一行。
/// - 校正行已经指向这一行时跳过，所以重复调用不会重复计数。
pub fn restore_for_song(conn: &Connection, song_id: &str, artist: &str, title: &str) -> Result<usize> {
    if !table_exists(conn)? {
        return Ok(0);
    }
    let rows: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT utterance_id, tokens_json, text FROM token_corrections \
             WHERE song_artist=?1 AND song_title=?2 ORDER BY text, utterance_id",
        )?;
        let mapped = stmt.query_map(params![artist, title], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let mut exact: HashMap<String, (i64, String)> = HashMap::new();
    let mut folded: HashMap<String, (i64, String)> = HashMap::new();
    for (old_id, tokens_json, text) in rows {
        folded.insert(fold(&text), (old_id, tokens_json.clone()));
        exact.insert(text, (old_id, tokens_json));
    }

    let lines: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, text FROM utterances WHERE song_id=?1 ORDER BY line_idx, id",
        )?;
        let mapped = stmt.query_map(params![song_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut restored = 0;
    for (new_id, text) in lines {
        let Some((old_id, tokens_json)) = exact.get(&text).or_else(|| folded.get(&fold(&text)))
        else {
            continue;
        };
        if *old_id == new_id {
            continue;
        }
        let tokens = parse(tokens_json)?;
        // 两边的编辑器都不让存空校正；万一库里有，也不能拿它把整行分词清空
        if tokens.is_empty() {
            continue;
        }
        write_tokens(conn, new_id, &tokens)?;
        conn.execute(
            "UPDATE token_corrections SET utterance_id=?1, text=?2 WHERE utterance_id=?3",
            params![new_id, text, old_id],
        )?;
        restored += 1;
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::KwicQuery;

    /// **走 `schema.sql`，不要手抄建表语句。** 原来这里抄的是一个三张表的子集，
    /// 连 `token_corrections` 都没有——于是 `save` 走的是「表不存在、现建一张」
    /// 那条分支，和生产库里的情况正好相反。
    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::Corpus::create_tables_on(&conn).unwrap();
        conn
    }

    fn tok(surface: &str, lemma: &str, pos: &str) -> TokenEdit {
        TokenEdit {
            surface: surface.into(),
            lemma: lemma.into(),
            pos: pos.into(),
        }
    }

    fn add_song(conn: &Connection, id: &str, title: &str, artist: &str) {
        conn.execute(
            "INSERT INTO songs (id, title, artist) VALUES (?1,?2,?3)",
            params![id, title, artist],
        )
        .unwrap();
    }

    fn add_line(conn: &Connection, id: i64, song: &str, line_idx: i64, text: &str, tokens: &[TokenEdit]) {
        conn.execute(
            "INSERT INTO utterances (id, song_id, line_idx, time_sec, text) VALUES (?1,?2,?3,0,?4)",
            params![id, song, line_idx, text],
        )
        .unwrap();
        write_tokens(conn, id, tokens).unwrap();
    }

    /// 分词器给的：夜 / が / 明ける
    fn yoru() -> Vec<TokenEdit> {
        vec![tok("夜", "夜", "NOUN"), tok("が", "が", "ADP"), tok("明ける", "明ける", "VERB")]
    }

    /// 用户合并成：夜が / 明ける
    fn merged() -> Vec<TokenEdit> {
        vec![tok("夜が", "夜", "NOUN"), tok("明ける", "明ける", "VERB")]
    }

    fn kimi() -> Vec<TokenEdit> {
        vec![tok("君", "君", "PRON"), tok("を", "を", "ADP"), tok("待つ", "待つ", "VERB")]
    }

    /// Python 删歌只删这几张表（legacy/dialogs/song_manager.py::_delete），校正行留着。
    fn python_style_delete(conn: &Connection) {
        conn.execute_batch("DELETE FROM tokens; DELETE FROM utterances; DELETE FROM songs;")
            .unwrap();
    }

    #[test]
    fn a_library_that_never_corrected_anything_works_without_the_table() {
        let conn = fixture();
        add_song(&conn, "001", "夜行", "ヨルシカ");
        add_line(&conn, 1, "001", 0, "夜が明ける", &yoru());

        let view = load(&conn, 1).unwrap().unwrap();
        assert!(!view.corrected);
        assert_eq!(view.tokens, yoru());
        assert_eq!(view.original, yoru());
        assert!(!revert(&conn, 1).unwrap());
        assert_eq!(restore_for_song(&conn, "001", "ヨルシカ", "夜行").unwrap(), 0);
        assert!(corrected_among(&conn, &[1]).unwrap().is_empty());
        assert!(load(&conn, 999).unwrap().is_none());
    }

    #[test]
    fn saving_rewrites_tokens_and_keeps_the_very_first_original() {
        let conn = fixture();
        add_song(&conn, "001", "夜行", "ヨルシカ");
        add_line(&conn, 1, "001", 0, "夜が明ける", &yoru());

        save(&conn, 1, &merged()).unwrap();
        assert_eq!(tokens_in_db(&conn, 1).unwrap(), merged());
        let view = load(&conn, 1).unwrap().unwrap();
        assert!(view.corrected);
        assert_eq!(view.tokens, merged());
        assert_eq!(view.original, yoru());

        // 再改一次，「原始」仍然是分词器给的那份，不是上一次手改的
        let again = vec![tok("夜", "夜", "NOUN"), tok("が明ける", "が明ける", "VERB")];
        save(&conn, 1, &again).unwrap();
        let view = load(&conn, 1).unwrap().unwrap();
        assert_eq!(view.tokens, again);
        assert_eq!(view.original, yoru());

        // 恢复要用的键都记下了
        let (text, artist, title): (String, String, String) = conn
            .query_row(
                "SELECT text, song_artist, song_title FROM token_corrections WHERE utterance_id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((text.as_str(), artist.as_str(), title.as_str()), ("夜が明ける", "ヨルシカ", "夜行"));
    }

    #[test]
    fn saving_cleans_tokens_the_way_the_python_editor_does() {
        let conn = fixture();
        add_song(&conn, "001", "夜行", "ヨルシカ");
        add_line(&conn, 1, "001", 0, "夜が明ける", &yoru());

        save(
            &conn,
            1,
            &[tok(" 夜 ", "", ""), tok("   ", "x", "NOUN"), tok("が明ける", "が明ける", "VERB")],
        )
        .unwrap();
        assert_eq!(
            tokens_in_db(&conn, 1).unwrap(),
            vec![tok("夜", "夜", "NOUN"), tok("が明ける", "が明ける", "VERB")]
        );

        let err = save(&conn, 1, &[tok(" ", "", "")]).unwrap_err();
        assert!(err.to_string().contains("至少"), "{err}");
        // 被拒绝的保存不能动到已有的分词
        assert_eq!(tokens_in_db(&conn, 1).unwrap().len(), 2);
    }

    #[test]
    fn json_is_readable_by_python_and_python_json_is_readable_here() {
        let conn = fixture();
        add_song(&conn, "001", "夜行", "ヨルシカ");
        add_line(&conn, 1, "001", 0, "夜が明ける", &yoru());
        save(&conn, 1, &merged()).unwrap();

        let raw: String = conn
            .query_row("SELECT tokens_json FROM token_corrections", [], |r| r.get(0))
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let first = value[0].as_object().unwrap();
        let mut keys: Vec<&str> = first.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["lemma", "pos", "surface"]);

        // Python json.dumps 的默认分隔符带空格，而且 lemma 可能缺省
        conn.execute(
            "UPDATE token_corrections SET tokens_json = ?1",
            params![r#"[{"surface": "夜が明ける", "pos": "VERB"}]"#],
        )
        .unwrap();
        let view = load(&conn, 1).unwrap().unwrap();
        assert_eq!(view.tokens, vec![tok("夜が明ける", "夜が明ける", "VERB")]);
    }

    #[test]
    fn revert_goes_back_to_the_tokenizer_and_forgets_the_correction() {
        let conn = fixture();
        add_song(&conn, "001", "夜行", "ヨルシカ");
        add_line(&conn, 1, "001", 0, "夜が明ける", &yoru());
        save(&conn, 1, &merged()).unwrap();

        assert!(revert(&conn, 1).unwrap());
        assert_eq!(tokens_in_db(&conn, 1).unwrap(), yoru());
        let view = load(&conn, 1).unwrap().unwrap();
        assert!(!view.corrected);
        // 行没了，所以之后删歌重导也不会把旧分词套回来
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM token_corrections", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
        assert!(!revert(&conn, 1).unwrap());
    }

    #[test]
    fn a_deleted_and_reimported_song_gets_its_corrections_back() {
        let conn = fixture();
        add_song(&conn, "001", "夜行", "ヨルシカ");
        add_line(&conn, 1, "001", 0, "夜が明ける", &yoru());
        add_line(&conn, 2, "001", 1, "君を待つ", &kimi());
        save(&conn, 1, &merged()).unwrap();

        python_style_delete(&conn);
        // 重新导入：新 song id、新 utterance id，分词是分词器重新给的
        add_song(&conn, "002", "夜行", "ヨルシカ");
        add_line(&conn, 10, "002", 0, "夜が明ける", &yoru());
        add_line(&conn, 11, "002", 1, "君を待つ", &kimi());

        assert_eq!(restore_for_song(&conn, "002", "ヨルシカ", "夜行").unwrap(), 1);
        assert_eq!(tokens_in_db(&conn, 10).unwrap(), merged());
        assert_eq!(tokens_in_db(&conn, 11).unwrap(), kimi(), "没校正过的行不能动");
        let moved: i64 = conn
            .query_row("SELECT utterance_id FROM token_corrections", [], |r| r.get(0))
            .unwrap();
        assert_eq!(moved, 10);
        assert!(load(&conn, 10).unwrap().unwrap().corrected);

        // 已经指向这一行了，再跑一遍不会重复计数
        assert_eq!(restore_for_song(&conn, "002", "ヨルシカ", "夜行").unwrap(), 0);
    }

    #[test]
    fn a_line_that_only_differs_in_width_or_spacing_still_matches() {
        // 旧 LRC 是半角片假名 + 全角空格 + 行尾空格，新 LRC 规整过。
        // 精确匹配不上，靠 NFKC + 去首尾空白兜底。
        let conn = fixture();
        add_song(&conn, "001", "さよならはエモーション", "サカナクション");
        let old_text = "ｻﾖﾅﾗ\u{3000}ｴﾓｰｼｮﾝ ";
        add_line(&conn, 1, "001", 0, old_text, &[tok(old_text.trim(), old_text.trim(), "NOUN")]);
        let fixed = vec![tok("サヨナラ", "サヨナラ", "INTJ"), tok("エモーション", "エモーション", "NOUN")];
        save(&conn, 1, &fixed).unwrap();

        python_style_delete(&conn);
        add_song(&conn, "002", "さよならはエモーション", "サカナクション");
        add_line(&conn, 10, "002", 0, "サヨナラ エモーション", &[tok("サヨナラエモーション", "x", "NOUN")]);

        assert_eq!(
            restore_for_song(&conn, "002", "サカナクション", "さよならはエモーション").unwrap(),
            1
        );
        assert_eq!(tokens_in_db(&conn, 10).unwrap(), fixed);
    }

    #[test]
    fn a_repeated_line_is_corrected_everywhere_but_the_row_follows_the_first() {
        // 副歌重复。Python 对每一行都套校正，但校正行只挪到第一次出现的那行
        // （后面几行的 UPDATE 找不到旧 id 了）。照抄；KWIC 折叠重复行取的也是 MIN(u.id)。
        let conn = fixture();
        add_song(&conn, "001", "夜行", "ヨルシカ");
        add_line(&conn, 1, "001", 0, "夜が明ける", &yoru());
        add_line(&conn, 2, "001", 1, "夜が明ける", &yoru());
        save(&conn, 1, &merged()).unwrap();

        python_style_delete(&conn);
        add_song(&conn, "002", "夜行", "ヨルシカ");
        add_line(&conn, 10, "002", 0, "夜が明ける", &yoru());
        add_line(&conn, 11, "002", 1, "夜が明ける", &yoru());

        assert_eq!(restore_for_song(&conn, "002", "ヨルシカ", "夜行").unwrap(), 2);
        assert_eq!(tokens_in_db(&conn, 10).unwrap(), merged());
        assert_eq!(tokens_in_db(&conn, 11).unwrap(), merged());
        let rows: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT utterance_id FROM token_corrections").unwrap();
            stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(rows, vec![10]);
    }

    #[test]
    fn corrections_never_leak_into_another_song_with_the_same_line() {
        let conn = fixture();
        add_song(&conn, "001", "夜行", "ヨルシカ");
        add_line(&conn, 1, "001", 0, "夜が明ける", &yoru());
        save(&conn, 1, &merged()).unwrap();

        add_song(&conn, "002", "花に亡霊", "ヨルシカ");
        add_line(&conn, 10, "002", 0, "夜が明ける", &yoru());
        assert_eq!(restore_for_song(&conn, "002", "ヨルシカ", "花に亡霊").unwrap(), 0);
        assert_eq!(tokens_in_db(&conn, 10).unwrap(), yoru());
    }

    #[test]
    fn kwic_marks_corrected_lines_and_finds_the_new_words_right_away() {
        let conn = fixture();
        add_song(&conn, "001", "夜行", "ヨルシカ");
        add_line(&conn, 1, "001", 0, "夜が明ける", &yoru());
        add_line(&conn, 2, "001", 1, "君を待つ", &kimi());
        save(&conn, 1, &merged()).unwrap();

        let hits = crate::search::kwic(
            &conn,
            &KwicQuery {
                keywords: vec!["夜が".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(hits.len(), 1, "校正后的新词应该立刻能检索到");
        assert!(hits[0].corrected);

        let hits = crate::search::kwic(
            &conn,
            &KwicQuery {
                keywords: vec!["君".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].corrected);
    }

    #[test]
    fn kwic_japanese_only_filters_on_the_keyword_before_the_limit() {
        let conn = fixture();
        // 排序是歌手、曲名：A 在前。只取 1 条时，先截断再筛就会一条不剩
        add_song(&conn, "001", "A", "X");
        add_line(&conn, 1, "001", 0, "Love と 愛", &[tok("Love", "love", "NOUN"), tok("と", "と", "ADP"), tok("愛", "愛", "NOUN")]);
        add_song(&conn, "002", "B", "X");
        add_line(&conn, 2, "002", 0, "愛してる", &[tok("愛", "愛する", "VERB"), tok("してる", "する", "AUX")]);
        let query = |jp_only: bool, limit: Option<i64>| KwicQuery {
            keywords: vec!["Love".into(), "愛".into()],
            jp_only,
            limit,
            ..Default::default()
        };

        let all = crate::search::kwic(&conn, &query(false, None)).unwrap();
        // 折叠后一行一条，取行内第一个命中：001 那行关键词是 Love（Python 也是 MIN(token_idx)）
        assert_eq!(all.iter().map(|h| h.keyword.as_str()).collect::<Vec<_>>(), ["Love", "愛"]);

        let jp = crate::search::kwic(&conn, &query(true, Some(1))).unwrap();
        assert_eq!(jp.iter().map(|h| (h.song_id.as_str(), h.keyword.as_str())).collect::<Vec<_>>(), [("002", "愛")]);
        assert_eq!(crate::search::kwic(&conn, &query(false, Some(1))).unwrap()[0].keyword, "Love");
    }
}
