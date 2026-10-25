//! 两端共用的状态与选项。
use crate::{AnkiError, CardOptions, ExportOptions};

pub fn validate_deck(deck: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!deck.trim().is_empty(), "要先选一个牌组");
    Ok(())
}

pub fn validate_selection(items: &[crate::batch::ExportItem], deck: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!items.is_empty(), "没有选中任何词");
    validate_deck(deck)
}

pub fn words(
    conn: &rusqlite::Connection,
    options: &crate::PickOptions,
    skip_studied: bool,
) -> anyhow::Result<Vec<crate::WordCandidate>> {
    let learning = crate::learning::load(None, "").ok();
    let mut words = crate::pick_words(conn, options, learning.as_ref())?;
    if skip_studied {
        words.retain(|w| !w.studied);
    }
    Ok(words)
}

pub fn preview(
    conn: &rusqlite::Connection,
    lemma: &str,
    pos: &str,
    max_examples: usize,
    max_dicts: Option<usize>,
) -> anyhow::Result<crate::Card> {
    crate::card::build(
        conn,
        lemma,
        pos,
        &[],
        CardOptions {
            max_examples: max_examples.clamp(1, 8),
            max_dicts,
        },
    )
}

pub fn notes_since(since_ms: i64) -> anyhow::Result<Option<usize>> {
    if crate::learning::find_collection().is_none() {
        return Ok(None);
    }
    Ok(Some(crate::learning::notes_created_since(None, since_ms)?))
}

pub fn browse_notes(anki: &crate::AnkiConnect, note_ids: &[i64]) -> anyhow::Result<()> {
    let ids: Vec<String> = note_ids.iter().map(i64::to_string).collect();
    anki.call(
        "guiBrowse",
        serde_json::json!({"query": format!("nid:{}", ids.join(","))}),
    )
    .map(|_| ())
    .map_err(|e| anyhow::anyhow!(e.advice()))
}
/// 界面一进来就问的那几件事。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnkiStatus {
    /// Anki 开着而且装了 AnkiConnect
    pub connected: bool,
    /// 连不上时的处置建议
    pub message: String,
    pub version: u32,
    pub decks: Vec<String>,
    /// 「JPOP Corpus」这个笔记类型在不在
    pub note_type_ready: bool,
    /// 读到的 collection 路径。空表示没找到。
    pub collection_path: String,
    /// 已经进过 Anki 的词数
    pub known_words: usize,
    /// 其中复习过的
    pub studied_words: usize,
}

/// 问一遍 Anki 的状态。**连不上不是错误**——用户可能还没开 Anki，
/// 界面该显示指引而不是红色报错。
pub fn status(client: &crate::AnkiConnect) -> AnkiStatus {
    let anki = client;
    let mut out = AnkiStatus::default();
    match anki.ping() {
        Ok(version) => {
            out.connected = true;
            out.version = version;
        }
        Err(err) => {
            out.message = err.advice();
            // 连不上也把学习状态读出来：那是直接读文件，不需要 Anki 开着
            fill_learning(&mut out);
            return out;
        }
    }
    out.decks = anki.deck_names().unwrap_or_default();
    out.note_type_ready = anki
        .model_names()
        .map(|names| names.iter().any(|n| n == crate::NOTE_TYPE))
        .unwrap_or(false);
    fill_learning(&mut out);
    out
}

fn fill_learning(out: &mut AnkiStatus) {
    // 找不到 collection 很正常（没装 Anki、或者装在别处），不当错误
    if let Ok(state) = crate::learning::load(None, "") {
        out.collection_path = state.collection_path.clone();
        out.known_words = state.words.len();
        out.studied_words = state.studied_count();
    }
}

/// 组装导出选项。
pub fn options(deck: String, max_examples: usize, max_dicts: Option<usize>) -> ExportOptions {
    ExportOptions {
        deck,
        card: CardOptions {
            max_examples: max_examples.clamp(1, 8),
            max_dicts,
        },
        ..Default::default()
    }
}

/// 把 AnkiConnect 的错误转成一句给用户的话。
pub fn describe(err: &AnkiError) -> String {
    err.advice()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_count_is_clamped_to_something_sane() {
        // 0 条例句的卡片没有意义；几十条会让卡片没法看
        assert_eq!(options("D".into(), 0, None).card.max_examples, 1);
        assert_eq!(options("D".into(), 99, None).card.max_examples, 8);
        assert_eq!(options("D".into(), 3, None).card.max_examples, 3);
    }
}
