//! 选词、组卡、推给 Anki。
//!
//! 重复卡片**不是错误**：同一个词以前导过，这次在别的歌里又遇到了，
//! 该做的是把新例句追加到已有的那张卡上，而不是报错或者建第二张。
//! 复习历史长在卡片上，建第二张等于把它劈成两半。

use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::card::{Card, CardOptions, build, word_fields};
use crate::connect::{AnkiConnect, AnkiError};
use crate::learning::LearningState;
use crate::model::{self, NOTE_TYPE};

/// 语料里的一个候选词。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordCandidate {
    pub lemma: String,
    /// UPOS
    pub pos: String,
    /// 在语料里出现几次
    pub count: i64,
    /// 出现在几首歌里
    pub song_count: i64,
    /// Anki 里已经有了
    pub in_anki: bool,
    /// 已经复习过
    pub studied: bool,
}

/// 选词的条件。
#[derive(Debug, Clone)]
pub struct PickOptions {
    /// 只要这些词性。空表示不限。默认只要实词——助词做成卡片没有意义。
    pub pos: Vec<String>,
    /// 至少出现几次
    pub min_count: i64,
    /// 只从这些歌里选。空表示全库。
    pub song_ids: Vec<String>,
    pub limit: i64,
}

impl Default for PickOptions {
    fn default() -> Self {
        Self {
            // 和曲库页「可点的词」一致：助词、符号点开也没有研究价值
            pos: ["NOUN", "PROPN", "VERB", "ADJ", "ADV"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            min_count: 1,
            song_ids: Vec::new(),
            limit: 500,
        }
    }
}

/// 按出现频次列出候选词，并标出 Anki 里已有的。
pub fn pick_words(
    conn: &Connection,
    options: &PickOptions,
    learning: Option<&LearningState>,
) -> Result<Vec<WordCandidate>> {
    let mut sql = String::from(
        "SELECT t.lemma, t.pos, COUNT(*) AS n, COUNT(DISTINCT u.song_id) AS songs
         FROM tokens t
         JOIN utterances u ON u.id = t.utterance_id
         WHERE COALESCE(t.lemma,'') <> ''",
    );
    // **数字必须按数字绑定。** 全当字符串塞进去的话 `n >= '1'` 恒假——
    // SQLite 里 INTEGER 永远小于 TEXT，比较不会隐式转型，
    // 结果是一个候选词都选不出来而且不报错。
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if !options.pos.is_empty() {
        let marks: Vec<String> = (0..options.pos.len())
            .map(|i| format!("?{}", i + 1))
            .collect();
        sql.push_str(&format!(" AND t.pos IN ({})", marks.join(",")));
        values.extend(
            options
                .pos
                .iter()
                .map(|p| Box::new(p.clone()) as Box<dyn rusqlite::ToSql>),
        );
    }
    if !options.song_ids.is_empty() {
        let start = values.len() + 1;
        let marks: Vec<String> = (0..options.song_ids.len())
            .map(|i| format!("?{}", start + i))
            .collect();
        sql.push_str(&format!(" AND u.song_id IN ({})", marks.join(",")));
        values.extend(
            options
                .song_ids
                .iter()
                .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::ToSql>),
        );
    }
    sql.push_str(&format!(
        " GROUP BY t.lemma, t.pos HAVING n >= ?{} ORDER BY n DESC, t.lemma LIMIT ?{}",
        values.len() + 1,
        values.len() + 2
    ));
    values.push(Box::new(options.min_count));
    values.push(Box::new(options.limit));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), |r| {
        Ok(WordCandidate {
            lemma: r.get(0)?,
            pos: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            count: r.get(2)?,
            song_count: r.get(3)?,
            in_anki: false,
            studied: false,
        })
    })?;

    let mut out: Vec<WordCandidate> = rows.collect::<Result<Vec<_>, _>>()?;
    if let Some(state) = learning {
        for word in &mut out {
            if let Some(status) = state.words.get(&word.lemma) {
                word.in_anki = status.exists;
                word.studied = status.studied;
            }
        }
    }
    Ok(out)
}

/// 一个词的导出结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExportOutcome {
    /// 新建了一张卡
    Added,
    /// 已有卡片，追加了 N 段新例句
    #[serde(rename_all = "camelCase")]
    Updated { new_sentences: usize },
    /// 已有卡片，例句也都在了，什么都没做
    AlreadyComplete,
    /// 已有卡片，按设置跳过
    Skipped,
    /// 没有例句可放，没导
    NoExamples,
    Failed { error: String },
}

/// Anki 里已经有这个词时怎么办。
///
/// Python 版还有「手动选择」（逐个弹窗勾例句），这里先没做——批量导出时逐个弹窗
/// 等于把几百个词变成几百次点击。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DupMode {
    /// 把新例句追加到已有的卡上（Python 版的「自动追加」）
    #[default]
    Append,
    /// 已有的卡不动
    Skip,
}

/// 查重看多大范围。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DupScope {
    /// 只看要导进的这个牌组
    #[default]
    Deck,
    /// 看主牌组及其所有子牌组——防止同一个词在「日本語::歌詞」「日本語::小说」里各有一张
    Root,
}

/// 切音频要的东西。`ExportOptions.audio` 为 `None` 表示不切。
#[derive(Debug, Clone)]
pub struct AudioOptions {
    pub ffmpeg: std::path::PathBuf,
    /// 切出来的 mp3 先放这里，传给 Anki 后删掉
    pub work_dir: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub deck: String,
    pub card: CardOptions,
    pub song_ids: Vec<String>,
    /// 一条例句都没有的词直接跳过。做成卡片也没法用。
    pub skip_without_examples: bool,
    pub dup_mode: DupMode,
    pub dup_scope: DupScope,
    pub audio: Option<AudioOptions>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            deck: "Default".into(),
            card: CardOptions::default(),
            song_ids: Vec::new(),
            skip_without_examples: true,
            dup_mode: DupMode::default(),
            dup_scope: DupScope::default(),
            audio: None,
        }
    }
}

/// 查重和找已有卡用的牌组。和 Python 一致：「主牌组」取 `::` 前的第一段。
fn check_deck(deck: &str, scope: DupScope) -> &str {
    match scope {
        DupScope::Root => deck.split("::").next().unwrap_or(deck),
        DupScope::Deck => deck,
    }
}

/// Anki 搜索语法里的引号转义。和 Python `_anki_query_quote` 一致。
fn quote(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// 牌组不存在时的一句提示，列出现有的牌组；存在时返回 `None`。和 Python 导出前的检查一致。
pub fn missing_deck_message(anki: &AnkiConnect, deck: &str) -> Result<Option<String>, AnkiError> {
    let mut decks = anki.deck_names()?;
    if decks.iter().any(|d| d == deck) {
        return Ok(None);
    }
    decks.sort();
    Ok(Some(format!(
        "牌组「{deck}」在 Anki 里不存在。现有牌组：{}",
        decks.join("、")
    )))
}

/// 导一个词。
pub fn export_word(
    anki: &AnkiConnect,
    conn: &Connection,
    lemma: &str,
    upos: &str,
    options: &ExportOptions,
) -> Result<ExportOutcome, AnkiError> {
    let mut card = match build(conn, lemma, upos, &options.song_ids, options.card) {
        Ok(card) => card,
        Err(err) => {
            return Ok(ExportOutcome::Failed {
                error: format!("{err:#}"),
            });
        }
    };
    if options.skip_without_examples && card.no_examples {
        return Ok(ExportOutcome::NoExamples);
    }

    let check = check_deck(&options.deck, options.dup_scope);
    let mut dup_options = json!({ "allowDuplicate": false, "duplicateScope": "deck" });
    if check != options.deck {
        dup_options["duplicateScopeOptions"] = json!({ "deckName": check, "checkChildren": true });
    }
    let note = |card: &Card| {
        json!({
            "deckName": options.deck,
            "modelName": NOTE_TYPE,
            "fields": card.fields(),
            "options": dup_options,
            "tags": ["jpop-corpus"],
        })
    };

    // 先问能不能加。重复的词不切音频——切了也用不上，还会在 Anki 的媒体文件夹里
    // 留下没人引用的文件（Python 版是先切先传再加，重复时那些文件就成了孤儿）。
    if !anki.can_add_note(&note(&card))? {
        return on_duplicate(anki, &card, options);
    }
    if let Some(audio) = &options.audio {
        card.sentence_audio = crate::audio::attach(anki, audio, &card.examples)?;
    }

    match anki.call("addNote", json!({ "note": note(&card) })) {
        Ok(_) => Ok(ExportOutcome::Added),
        // Anki 用一句话说「重复了」。那不是错误，是「该合并」。
        Err(AnkiError::Rejected(message)) if is_duplicate(&message) => {
            on_duplicate(anki, &card, options)
        }
        Err(err) => Err(err),
    }
}

fn on_duplicate(
    anki: &AnkiConnect,
    card: &Card,
    options: &ExportOptions,
) -> Result<ExportOutcome, AnkiError> {
    match options.dup_mode {
        DupMode::Skip => Ok(ExportOutcome::Skipped),
        DupMode::Append => merge_into_existing(anki, card, options),
    }
}

fn is_duplicate(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("duplicate")
}

/// 把新例句追加到已有的卡上。
///
/// **只加不删。** 已有的例句是用户复习过的语境，换掉等于把记忆的
/// 锚点抽走。新句子接在后面。音频不追加——和 Python 一致。
fn merge_into_existing(
    anki: &AnkiConnect,
    card: &Card,
    options: &ExportOptions,
) -> Result<ExportOutcome, AnkiError> {
    let check = check_deck(&options.deck, options.dup_scope);
    let query = format!(
        r#"deck:"{}" note:"{}" Expression:"{}""#,
        quote(check),
        NOTE_TYPE,
        quote(&card.expression)
    );
    let ids = anki.find_notes(&query)?;
    let Some(note_id) = ids.first().copied() else {
        // Anki 说重复但查不到——多半是查重范围比牌组宽（子牌组、
        // 或者别的牌组里有）。这种情况不猜，如实报出来。
        return Ok(ExportOutcome::Failed {
            error: format!("Anki 说「{}」重复，但在牌组 {check} 里找不到它", card.expression),
        });
    };
    let existing = anki.notes_info(&[note_id])?;
    let Some((_, mut fields)) = existing.into_iter().next() else {
        return Ok(ExportOutcome::Failed {
            error: format!("取不到笔记 {note_id} 的内容"),
        });
    };

    let old_sentence = fields.get("Sentence").cloned().unwrap_or_default();
    let new_blocks: Vec<&str> = split_sentence_blocks(&card.sentence)
        .into_iter()
        .filter(|block| !old_sentence.contains(*block))
        .collect();
    if new_blocks.is_empty() {
        return Ok(ExportOutcome::AlreadyComplete);
    }

    let merged = format!("{old_sentence}{}", new_blocks.concat());
    let mut update = std::collections::BTreeMap::new();
    update.insert("Sentence".to_string(), merged);
    // 出处也补上，否则新加的句子看不出来自哪
    if !card.source.is_empty() {
        let old_source = fields.remove("Source").unwrap_or_default();
        let mut parts: Vec<&str> = old_source.split('、').filter(|s| !s.is_empty()).collect();
        for source in card.source.split('、') {
            if !source.is_empty() && !parts.contains(&source) {
                parts.push(source);
            }
        }
        update.insert("Source".to_string(), parts.join("、"));
    }
    anki.update_note_fields(note_id, &update)?;
    Ok(ExportOutcome::Updated {
        new_sentences: new_blocks.len(),
    })
}

/// 把 Sentence 字段拆回一段段 `<div class="sent">…</div>`。
///
/// 按整段比对而不是按整个字段——已有的卡上可能已经有两句，
/// 这次的两句里只有一句是新的。
fn split_sentence_blocks(html: &str) -> Vec<&str> {
    const OPEN: &str = r#"<div class="sent">"#;
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start..];
        let end = match after[OPEN.len()..].find(OPEN) {
            Some(next) => OPEN.len() + next,
            None => after.len(),
        };
        out.push(&after[..end]);
        rest = &after[end..];
    }
    out
}

/// 确保 note type 就位（顺带更新模板和 CSS）。导出、更新、刷新前各调一次。
pub fn prepare(anki: &AnkiConnect) -> Result<bool, AnkiError> {
    model::ensure(anki)
}

// ────────────────────────── 更新已有卡 / 刷新旧牌组 ──────────────────────────

/// 刷新一张已有卡的结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RefreshOutcome {
    /// 词典字段重新写了一遍
    Refreshed,
    /// Anki 里没有这个词的卡
    NotFound,
    Failed { error: String },
}

/// 刷新旧牌组的范围。和 Python 的三个选项一致。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshScope {
    /// 仅当前牌组
    #[default]
    Deck,
    /// 当前牌组 + 子牌组
    DeckAndChildren,
    /// 主牌组 + 子牌组
    RootAndChildren,
}

fn deck_in_scope(card_deck: &str, selected: &str, scope: RefreshScope) -> bool {
    match scope {
        RefreshScope::RootAndChildren => {
            let root = selected.split("::").next().unwrap_or(selected);
            card_deck == root || card_deck.starts_with(&format!("{root}::"))
        }
        RefreshScope::DeckAndChildren => {
            card_deck == selected || card_deck.starts_with(&format!("{selected}::"))
        }
        RefreshScope::Deck => card_deck == selected,
    }
}

/// 一张要刷新的旧卡。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTarget {
    pub note_id: i64,
    pub expression: String,
}

/// 更新选中的词：重新查读音、释义、JLPT、音高、词频、词性，**不动例句、音频、出处**。
///
/// 和 Python `AnkiUpdateWorker` 一致，只是查询多带了 `note:` 条件——别的笔记类型
/// 碰巧也有 Expression 字段时不该被改。释义**不截断词典数**：已有的卡是按全量做的，
/// 截断的话刷新一次所有卡片都会变短。
pub fn update_word(
    anki: &AnkiConnect,
    conn: &Connection,
    lemma: &str,
    upos: &str,
    deck: &str,
    scope: DupScope,
) -> Result<RefreshOutcome, AnkiError> {
    let query = format!(
        r#"deck:"{}" note:"{}" Expression:"{}""#,
        quote(check_deck(deck, scope)),
        NOTE_TYPE,
        quote(lemma)
    );
    let ids = anki.find_notes(&query)?;
    let Some(note_id) = ids.first().copied() else {
        return Ok(RefreshOutcome::NotFound);
    };
    refresh_note(anki, conn, note_id, lemma, upos)
}

fn refresh_note(
    anki: &AnkiConnect,
    conn: &Connection,
    note_id: i64,
    lemma: &str,
    upos: &str,
) -> Result<RefreshOutcome, AnkiError> {
    let fields = match word_fields(conn, lemma, upos, None) {
        Ok(fields) => fields,
        Err(err) => {
            return Ok(RefreshOutcome::Failed {
                error: format!("{err:#}"),
            });
        }
    };
    anki.update_note_fields(note_id, &fields.update_map())?;
    Ok(RefreshOutcome::Refreshed)
}

/// 找出范围内所有 JPOP Corpus 旧卡。**只读**，界面上确认之前先用它报个数。
///
/// 和 Python `_find_target_notes` 一致：按笔记类型、按标签各查一遍取并集，
/// 再按卡片所在牌组过滤。一张笔记有多张卡时只算一次。
pub fn refresh_targets(
    anki: &AnkiConnect,
    deck: &str,
    scope: RefreshScope,
) -> Result<Vec<RefreshTarget>, AnkiError> {
    let mut card_ids: Vec<i64> = Vec::new();
    let mut seen_cards: HashSet<i64> = HashSet::new();
    for query in [format!(r#"note:"{}""#, quote(NOTE_TYPE)), "tag:jpop-corpus".to_string()] {
        // Python 查询失败时当成没查到；Anki 关了则要停下来
        let found = match anki.find_cards(&query) {
            Ok(found) => found,
            Err(err) if err.is_not_running() => return Err(err),
            Err(_) => Vec::new(),
        };
        for id in found {
            if seen_cards.insert(id) {
                card_ids.push(id);
            }
        }
    }

    let mut note_ids: Vec<i64> = Vec::new();
    let mut seen_notes: HashSet<i64> = HashSet::new();
    for chunk in card_ids.chunks(100) {
        for (_, note_id, card_deck) in anki.cards_info(chunk)? {
            if note_id != 0 && deck_in_scope(&card_deck, deck, scope) && seen_notes.insert(note_id) {
                note_ids.push(note_id);
            }
        }
    }

    let mut out = Vec::new();
    for chunk in note_ids.chunks(100) {
        for (note_id, fields) in anki.notes_info(chunk)? {
            // 只收这一批要的：别的笔记混进来就刷到范围外去了
            if !chunk.contains(&note_id) {
                continue;
            }
            let expression =
                crate::learning::plain_field(fields.get("Expression").map(String::as_str).unwrap_or(""));
            if !expression.is_empty() {
                out.push(RefreshTarget {
                    note_id,
                    expression,
                });
            }
        }
    }
    Ok(out)
}

/// 刷新一张旧卡。词性取这个词在语料里最常见的那个（Python `_lookup_common_pos_map`）。
pub fn refresh_target(
    anki: &AnkiConnect,
    conn: &Connection,
    target: &RefreshTarget,
) -> Result<RefreshOutcome, AnkiError> {
    let pos = match common_pos(conn, &target.expression) {
        Ok(pos) => pos,
        Err(err) => {
            return Ok(RefreshOutcome::Failed {
                error: format!("{err:#}"),
            });
        }
    };
    refresh_note(anki, conn, target.note_id, &target.expression, &pos)
}

fn common_pos(conn: &Connection, lemma: &str) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT pos FROM tokens WHERE lemma=?1 GROUP BY pos ORDER BY COUNT(*) DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([lemma], |r| r.get::<_, Option<String>>(0))?;
    Ok(rows.next().transpose()?.flatten().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::testing::FakeAnki;
    use jp_scraper::http::HttpClient;
    use std::sync::Arc;

    fn client(fake: Arc<FakeAnki>) -> AnkiConnect {
        struct Shared(Arc<FakeAnki>);
        impl jp_scraper::http::Transport for Shared {
            fn get(
                &self,
                u: &str,
                h: &[(String, String)],
                t: jp_scraper::http::Timeouts,
            ) -> std::result::Result<jp_scraper::http::HttpResponse, jp_scraper::ProviderError>
            {
                self.0.get(u, h, t)
            }
            fn post(
                &self,
                u: &str,
                b: &[u8],
                h: &[(String, String)],
                t: jp_scraper::http::Timeouts,
            ) -> std::result::Result<jp_scraper::http::HttpResponse, jp_scraper::ProviderError>
            {
                self.0.post(u, b, h, t)
            }
        }
        AnkiConnect::new(HttpClient::new(Box::new(Shared(fake))))
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE songs (id TEXT PRIMARY KEY, artist TEXT, title TEXT, audio_path TEXT);
             CREATE TABLE utterances (id INTEGER PRIMARY KEY, song_id TEXT, line_idx INTEGER,
                time_sec REAL, text TEXT);
             CREATE TABLE tokens (id INTEGER PRIMARY KEY, utterance_id INTEGER,
                token_idx INTEGER, surface TEXT, lemma TEXT, pos TEXT);
             CREATE TABLE dict_registry (name TEXT, zip_path TEXT, dict_type TEXT,
                enabled INTEGER, sort_order INTEGER);
             CREATE TABLE dict_terms (id INTEGER PRIMARY KEY, term TEXT, reading TEXT,
                dict_name TEXT, defs_json TEXT);
             CREATE TABLE yomitan_zh (term TEXT, reading TEXT, zh_defs TEXT);
             CREATE TABLE yomitan_pitch (term TEXT, reading TEXT, positions TEXT);
             CREATE TABLE yomitan_freq (term TEXT, freq INTEGER);
             CREATE TABLE jlpt_cache (lemma TEXT, level TEXT);

             INSERT INTO songs VALUES ('001','ヨルシカ','夜行',''),('002','YOASOBI','夜に駆ける','');
             INSERT INTO utterances VALUES (1,'001',0,10.0,'夜が明ける'),
                                           (2,'002',0,20.0,'夜に駆ける');
             INSERT INTO tokens VALUES (1,1,0,'夜','夜','NOUN'),
                                       (2,2,0,'夜','夜','NOUN'),
                                       (3,1,1,'明ける','明ける','VERB'),
                                       (4,1,2,'が','が','ADP');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn function_words_are_not_offered_as_cards() {
        // 助词做成卡片没有意义
        let conn = db();
        let words = pick_words(&conn, &PickOptions::default(), None).unwrap();
        let lemmas: Vec<&str> = words.iter().map(|w| w.lemma.as_str()).collect();
        assert!(lemmas.contains(&"夜"));
        assert!(lemmas.contains(&"明ける"));
        assert!(!lemmas.contains(&"が"), "助词不该出现：{lemmas:?}");
    }

    #[test]
    fn candidates_are_ranked_by_frequency() {
        let conn = db();
        let words = pick_words(&conn, &PickOptions::default(), None).unwrap();
        assert_eq!(words[0].lemma, "夜");
        assert_eq!(words[0].count, 2);
        assert_eq!(words[0].song_count, 2, "出现在两首歌里");
    }

    #[test]
    fn learning_state_marks_what_is_already_known() {
        let conn = db();
        let mut state = LearningState::default();
        state.words.insert(
            "夜".into(),
            crate::learning::WordStatus {
                exists: true,
                studied: true,
                ..Default::default()
            },
        );
        let words = pick_words(&conn, &PickOptions::default(), Some(&state)).unwrap();
        let yoru = words.iter().find(|w| w.lemma == "夜").unwrap();
        assert!(yoru.in_anki && yoru.studied);
        let other = words.iter().find(|w| w.lemma == "明ける").unwrap();
        assert!(!other.in_anki);
    }

    #[test]
    fn a_new_word_is_added_with_the_right_deck_and_model() {
        let conn = db();
        let fake = Arc::new(FakeAnki::new(&[("addNote", json!(1234))]));
        let options = ExportOptions {
            deck: "日本語::歌詞".into(),
            ..Default::default()
        };
        let out = export_word(&client(fake.clone()), &conn, "夜", "NOUN", &options).unwrap();
        assert_eq!(out, ExportOutcome::Added);

        let params = fake.params_of("addNote").unwrap();
        assert_eq!(params["note"]["deckName"], "日本語::歌詞");
        assert_eq!(params["note"]["modelName"], NOTE_TYPE);
        assert_eq!(params["note"]["tags"][0], "jpop-corpus");
        // 例句真的进去了
        // 目标词被高亮标签裹着，整句不是连续子串
        let sentence = params["note"]["fields"]["Sentence"].as_str().unwrap();
        assert!(
            sentence.contains(r#"<b style="color:#c0392b">夜</b>が明ける"#),
            "{sentence}"
        );
        assert!(sentence.contains("ヨルシカ「夜行」"), "{sentence}");
    }

    #[test]
    fn a_duplicate_appends_new_sentences_instead_of_failing() {
        // 复习历史长在卡片上，建第二张等于把它劈成两半
        let conn = db();
        let fake = Arc::new(
            FakeAnki::new(&[
                ("findNotes", json!([555])),
                (
                    "notesInfo",
                    json!([{
                        "noteId": 555,
                        "fields": {
                            "Sentence": {"value": r#"<div class="sent">旧的句子<span class="sent-src">A「B」</span></div>"#},
                            "Source": {"value": "A「B」"}
                        }
                    }]),
                ),
            ])
            .with_error("addNote", "cannot create note because it is a duplicate"),
        );
        let out = export_word(&client(fake.clone()), &conn, "夜", "NOUN", &Default::default())
            .unwrap();
        assert_eq!(out, ExportOutcome::Updated { new_sentences: 2 });

        let params = fake.params_of("updateNoteFields").unwrap();
        let merged = params["note"]["fields"]["Sentence"].as_str().unwrap();
        // 旧句子必须还在
        assert!(merged.contains("旧的句子"), "{merged}");
        assert!(
            merged.contains(r#"<b style="color:#c0392b">夜</b>が明ける"#),
            "{merged}"
        );
        // 出处也要合并，否则新句子看不出来自哪
        let source = params["note"]["fields"]["Source"].as_str().unwrap();
        assert!(source.starts_with("A「B」"), "{source}");
        assert!(source.contains("ヨルシカ「夜行」"), "{source}");
    }

    #[test]
    fn a_duplicate_with_nothing_new_does_not_touch_the_note() {
        let conn = db();
        // 已有的卡上就是这次会生成的那两句
        let card = build(&conn, "夜", "NOUN", &[], CardOptions::default()).unwrap();
        let fake = Arc::new(
            FakeAnki::new(&[
                ("findNotes", json!([555])),
                (
                    "notesInfo",
                    json!([{ "noteId": 555, "fields": { "Sentence": {"value": card.sentence} } }]),
                ),
            ])
            .with_error("addNote", "duplicate"),
        );
        let out = export_word(&client(fake.clone()), &conn, "夜", "NOUN", &Default::default())
            .unwrap();
        assert_eq!(out, ExportOutcome::AlreadyComplete);
        assert!(
            !fake.actions().iter().any(|a| a == "updateNoteFields"),
            "没有新东西就不该写"
        );
    }

    #[test]
    fn a_duplicate_that_cannot_be_found_is_reported_not_guessed() {
        // Anki 的查重范围可能比这个牌组宽。猜不出来就如实说。
        let conn = db();
        let fake = Arc::new(
            FakeAnki::new(&[("findNotes", json!([]))]).with_error("addNote", "duplicate"),
        );
        let out = export_word(&client(fake), &conn, "夜", "NOUN", &Default::default()).unwrap();
        match out {
            ExportOutcome::Failed { error } => assert!(error.contains("找不到"), "{error}"),
            other => panic!("该报失败：{other:?}"),
        }
    }

    #[test]
    fn a_word_with_no_examples_is_skipped() {
        let conn = db();
        let fake = Arc::new(FakeAnki::new(&[("addNote", json!(1))]));
        let out = export_word(&client(fake.clone()), &conn, "查无此词", "NOUN", &Default::default())
            .unwrap();
        assert_eq!(out, ExportOutcome::NoExamples);
        assert!(fake.actions().is_empty(), "不该发请求");
    }

    #[test]
    fn anki_being_down_is_an_error_not_a_per_word_failure() {
        // 整个作业该停下来让用户去开 Anki，而不是 500 个词各报一次
        let conn = db();
        let err = export_word(
            &client(Arc::new(FakeAnki::offline())),
            &conn,
            "夜",
            "NOUN",
            &Default::default(),
        )
        .unwrap_err();
        assert!(err.is_not_running());
    }

    #[test]
    fn sentence_blocks_are_split_one_per_div() {
        let html = r#"<div class="sent">A</div><div class="sent">B</div>"#;
        let blocks = split_sentence_blocks(html);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], r#"<div class="sent">A</div>"#);
        assert!(split_sentence_blocks("").is_empty());
    }

    #[test]
    fn a_duplicate_is_left_alone_when_the_setting_says_skip() {
        let conn = db();
        let fake = Arc::new(FakeAnki::new(&[("canAddNotes", json!([false]))]));
        let options = ExportOptions {
            dup_mode: DupMode::Skip,
            ..Default::default()
        };
        let out = export_word(&client(fake.clone()), &conn, "夜", "NOUN", &options).unwrap();
        assert_eq!(out, ExportOutcome::Skipped);
        let actions = fake.actions();
        assert!(
            !actions.iter().any(|a| a == "addNote" || a == "updateNoteFields"),
            "{actions:?}"
        );
    }

    #[test]
    fn root_deck_scope_checks_duplicates_across_sibling_decks() {
        let conn = db();
        let fake = Arc::new(FakeAnki::new(&[("addNote", json!(1))]));
        let options = ExportOptions {
            deck: "日本語::歌詞".into(),
            dup_scope: DupScope::Root,
            ..Default::default()
        };
        export_word(&client(fake.clone()), &conn, "夜", "NOUN", &options).unwrap();
        let note = fake.params_of("addNote").unwrap()["note"].clone();
        assert_eq!(note["options"]["duplicateScopeOptions"]["deckName"], "日本語");
        assert_eq!(note["options"]["duplicateScopeOptions"]["checkChildren"], true);
        // 卡片本身还是进选定的那个子牌组
        assert_eq!(note["deckName"], "日本語::歌詞");
    }

    #[test]
    fn appending_to_a_duplicate_searches_the_root_deck_when_asked() {
        let conn = db();
        let fake = Arc::new(FakeAnki::new(&[
            ("canAddNotes", json!([false])),
            ("findNotes", json!([])),
        ]));
        let options = ExportOptions {
            deck: "日本語::歌詞".into(),
            dup_scope: DupScope::Root,
            ..Default::default()
        };
        let _ = export_word(&client(fake.clone()), &conn, "夜", "NOUN", &options).unwrap();
        let query = fake.params_of("findNotes").unwrap()["query"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(query.starts_with(r#"deck:"日本語" "#), "{query}");
    }

    #[test]
    fn a_word_anki_cannot_add_is_neither_sent_nor_clipped() {
        // 先问能不能加：重复的词不切音频，免得在 Anki 媒体文件夹里留下没人用的文件
        let conn = db();
        let fake = Arc::new(FakeAnki::new(&[
            ("canAddNotes", json!([false])),
            ("findNotes", json!([])),
        ]));
        let options = ExportOptions {
            audio: Some(AudioOptions {
                ffmpeg: "ffmpeg-that-does-not-exist".into(),
                work_dir: std::env::temp_dir(),
            }),
            ..Default::default()
        };
        let _ = export_word(&client(fake.clone()), &conn, "夜", "NOUN", &options).unwrap();
        let actions = fake.actions();
        assert!(
            !actions.iter().any(|a| a == "addNote" || a == "storeMediaFile"),
            "{actions:?}"
        );
    }

    #[test]
    fn quotes_in_a_word_do_not_break_the_search() {
        assert_eq!(quote(r#"say "hi" \o/"#), r#"say \"hi\" \\o/"#);
    }

    #[test]
    fn updating_a_word_rewrites_only_the_dictionary_fields() {
        // 例句、音频、出处是用户复习过的语境，更新不碰
        let conn = db();
        let fake = Arc::new(FakeAnki::new(&[("findNotes", json!([777]))]));
        let out = update_word(&client(fake.clone()), &conn, "夜", "NOUN", "日本語", DupScope::Deck)
            .unwrap();
        assert_eq!(out, RefreshOutcome::Refreshed);
        let params = fake.params_of("updateNoteFields").unwrap();
        assert_eq!(params["note"]["id"], 777);
        let mut keys: Vec<String> = params["note"]["fields"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        assert_eq!(keys, ["Freq", "JLPT", "Meaning", "PartOfSpeech", "Pitch", "Reading"]);
        let query = fake.params_of("findNotes").unwrap()["query"].clone();
        assert_eq!(query, r#"deck:"日本語" note:"JPOP Corpus" Expression:"夜""#);
    }

    #[test]
    fn updating_a_word_that_is_not_in_anki_says_so() {
        let conn = db();
        let fake = Arc::new(FakeAnki::new(&[("findNotes", json!([]))]));
        let out = update_word(&client(fake.clone()), &conn, "夜", "NOUN", "日本語", DupScope::Deck)
            .unwrap();
        assert_eq!(out, RefreshOutcome::NotFound);
        assert!(!fake.actions().iter().any(|a| a == "updateNoteFields"));
    }

    fn refresh_fake() -> Arc<FakeAnki> {
        Arc::new(FakeAnki::new(&[
            ("findCards", json!([1, 2, 3, 4])),
            (
                "cardsInfo",
                json!([
                    {"cardId": 1, "note": 11, "deckName": "JPOP"},
                    {"cardId": 2, "note": 12, "deckName": "JPOP::ヨルシカ"},
                    {"cardId": 3, "note": 13, "deckName": "JPOPX"},
                    {"cardId": 4, "note": 12, "deckName": "JPOP::ヨルシカ"},
                ]),
            ),
            (
                "notesInfo",
                json!([
                    {"noteId": 11, "fields": {"Expression": {"value": "夜"}}},
                    {"noteId": 12, "fields": {"Expression": {"value": "<b>君</b>"}}},
                    {"noteId": 13, "fields": {"Expression": {"value": "花"}}},
                ]),
            ),
        ]))
    }

    #[test]
    fn refresh_targets_follow_the_deck_scope() {
        let pick = |deck: &str, scope| -> Vec<(i64, String)> {
            refresh_targets(&client(refresh_fake()), deck, scope)
                .unwrap()
                .into_iter()
                .map(|t| (t.note_id, t.expression))
                .collect()
        };
        assert_eq!(pick("JPOP", RefreshScope::Deck), vec![(11, "夜".to_string())]);
        // 「JPOPX」只是名字开头一样，不是子牌组
        assert_eq!(
            pick("JPOP", RefreshScope::DeckAndChildren),
            vec![(11, "夜".to_string()), (12, "君".to_string())]
        );
        assert_eq!(
            pick("JPOP::ヨルシカ", RefreshScope::RootAndChildren),
            vec![(11, "夜".to_string()), (12, "君".to_string())]
        );
        // 一张笔记两张卡只算一次
        assert_eq!(pick("JPOP::ヨルシカ", RefreshScope::Deck), vec![(12, "君".to_string())]);
    }

    #[test]
    fn old_cards_are_found_by_note_type_and_by_tag_like_python() {
        let fake = refresh_fake();
        let _ = refresh_targets(&client(fake.clone()), "JPOP", RefreshScope::Deck).unwrap();
        let queries: Vec<String> = fake
            .all_params_of("findCards")
            .iter()
            .map(|p| p["query"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(queries, [r#"note:"JPOP Corpus""#, "tag:jpop-corpus"]);
    }

    #[test]
    fn refreshing_an_old_card_uses_the_most_common_part_of_speech() {
        let conn = db();
        let fake = Arc::new(FakeAnki::new(&[]));
        let target = RefreshTarget {
            note_id: 9,
            expression: "明ける".into(),
        };
        let out = refresh_target(&client(fake.clone()), &conn, &target).unwrap();
        assert_eq!(out, RefreshOutcome::Refreshed);
        let params = fake.params_of("updateNoteFields").unwrap();
        assert_eq!(params["note"]["id"], 9);
        assert_eq!(params["note"]["fields"]["PartOfSpeech"], "動詞");
    }

    #[test]
    fn a_missing_deck_is_named_along_with_the_decks_that_exist() {
        let fake = Arc::new(FakeAnki::new(&[("deckNames", json!(["Default", "日本語"]))]));
        let message = missing_deck_message(&client(fake), "日本語::歌詞").unwrap().unwrap();
        assert!(message.contains("日本語::歌詞") && message.contains("Default"), "{message}");
        let fake = Arc::new(FakeAnki::new(&[("deckNames", json!(["日本語::歌詞"]))]));
        assert!(missing_deck_message(&client(fake), "日本語::歌詞").unwrap().is_none());
    }

    #[test]
    fn duplicate_detection_matches_ankis_wording() {
        assert!(is_duplicate("cannot create note because it is a duplicate"));
        assert!(is_duplicate("Duplicate"));
        assert!(!is_duplicate("deck was not found"));
    }
}
