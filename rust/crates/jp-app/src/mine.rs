//! 一键制卡的应用层：从曲库里点的词和那一行歌词，拼出 `jp_anki::mine` 要的东西。
//!
//! 前端只传「查的哪段文字、选的哪个词条、哪首歌哪一行第几个词」，词条和例句都在这边重新取：
//! 不信任前端送回来的大块 JSON，也保证卡上的释义和词典库当前内容一致。
//!
//! 例句：当前行排第一；这首歌里其他含这个词（含活用形）的行接在后面，文字相同的行只放一次。

use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use jp_anki::mine::{
    DictionaryAssets, LYRICS, LapisCardKind, MineLine, MineOptions, MineOutcome, MineSentence,
    MineSource,
};
use jp_anki::{AnkiConnect, AnkiError};
use jp_dict::occurrence::find_occurrences;
use jp_dict::store::{DictionaryStore, MatchSource};
use jp_dict::translator::TermDictionaryEntry;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MineRequest {
    /// 前端查词用的那段文字
    pub lookup_text: String,
    /// 选中词条的第一个词头
    pub term: String,
    pub reading: String,
    pub song_id: Option<String>,
    pub utterance_id: Option<i64>,
    /// 点的是这一行的第几个词
    pub token_index: Option<usize>,
    pub deck: String,
    /// 放进「牌组::歌手」子牌组，没有就建
    #[serde(default)]
    pub artist_subdeck: bool,
    pub model: String,
    pub main_dictionary: Option<String>,
    #[serde(default)]
    pub kind: LapisCardKind,
}

/// 每个词在原文里从第几个字开始（按字符）。
///
/// 分词结果拼起来不一定等于原文——歌词里常有空格、全角空格，分词时被丢掉了。
/// 逐个词往后找，词和词之间只允许隔着空白；对不上时返回 None，调用方退回用词拼成的句子。
pub fn align_tokens<S: AsRef<str>>(text: &str, surfaces: &[S]) -> Option<Vec<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let mut pos = 0;
    let mut offsets = Vec::with_capacity(surfaces.len());
    for surface in surfaces {
        let token: Vec<char> = surface.as_ref().chars().collect();
        if token.is_empty() {
            offsets.push(pos);
            continue;
        }
        if token.len() > chars.len() {
            return None;
        }
        let found =
            (pos..=chars.len() - token.len()).find(|&p| chars[p..p + token.len()] == token[..])?;
        if chars[pos..found].iter().any(|c| !c.is_whitespace()) {
            return None;
        }
        offsets.push(found);
        pos = found + token.len();
    }
    Some(offsets)
}

/// 第 `index` 个词在原文里从第几个字开始
pub fn align_token_offset<S: AsRef<str>>(
    text: &str,
    surfaces: &[S],
    index: usize,
) -> Option<usize> {
    align_tokens(text, surfaces)?.get(index).copied()
}

/// 比较两行是不是同一句：去掉所有空白再比
fn sentence_key(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

struct Line {
    text: String,
    offset: usize,
    utterance_id: i64,
    time_sec: Option<f64>,
    end_sec: Option<f64>,
}

struct OtherLine {
    text: String,
    highlights: Vec<(usize, usize)>,
    utterance_id: i64,
    time_sec: Option<f64>,
    end_sec: Option<f64>,
}

/// 当前行，以及这首歌里其他含这个词的行（按歌里的顺序，文字相同的只留第一次出现的）
fn load_lines(
    state: &AppState,
    entry: &TermDictionaryEntry,
    song_id: &str,
    utterance_id: i64,
    token_index: usize,
) -> Result<Option<(Line, Vec<OtherLine>)>> {
    let lyrics = state.corpus().lyrics(song_id)?;
    let Some(position) = lyrics.iter().position(|l| l.utterance_id == utterance_id) else {
        return Ok(None);
    };
    // 下一句开始的时间就是这一句结束的时间（和导出例句音频的算法一致）
    let end_of = |i: usize| {
        lyrics[i].time_sec.and_then(|t| {
            lyrics[i + 1..]
                .iter()
                .filter_map(|l| l.time_sec)
                .find(|&next| next > t)
        })
    };

    let line = &lyrics[position];
    let surfaces: Vec<&str> = line.tokens.iter().map(|t| t.surface.as_str()).collect();
    let (text, offset) = match align_token_offset(&line.text, &surfaces, token_index) {
        Some(offset) => (line.text.clone(), offset),
        None => (
            surfaces.concat(),
            surfaces[..token_index.min(surfaces.len())]
                .concat()
                .chars()
                .count(),
        ),
    };
    let current = Line {
        text,
        offset,
        utterance_id,
        time_sec: line.time_sec,
        end_sec: end_of(position),
    };

    // 找其他行：词头（点的是假名写法时也认读音），活用条件和词条的词性相容
    let mut targets: Vec<&str> = entry.headwords.iter().map(|h| h.term.as_str()).collect();
    let clicked_reading = entry
        .headwords
        .iter()
        .flat_map(|h| &h.sources)
        .find(|s| s.is_primary)
        .is_some_and(|s| s.match_source == MatchSource::Reading);
    if clicked_reading {
        targets.extend(entry.headwords.iter().map(|h| h.reading.as_str()));
    }
    let word_classes: Vec<&str> = entry
        .headwords
        .iter()
        .flat_map(|h| h.word_classes.iter().map(String::as_str))
        .collect();
    let transformer = state.translator().transformer();
    let conditions = transformer.condition_flags_from_parts_of_speech(&word_classes);

    let mut seen = vec![sentence_key(&line.text)];
    let mut others = Vec::new();
    for (i, other) in lyrics.iter().enumerate() {
        if i == position || other.tokens.is_empty() {
            continue;
        }
        let key = sentence_key(&other.text);
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        let surfaces: Vec<&str> = other.tokens.iter().map(|t| t.surface.as_str()).collect();
        let occurrences = find_occurrences(transformer, &surfaces, &targets, conditions);
        if occurrences.is_empty() {
            continue;
        }
        seen.push(key);
        let (text, offsets) = match align_tokens(&other.text, &surfaces) {
            Some(offsets) => (other.text.clone(), offsets),
            None => {
                let mut pos = 0;
                let offsets = surfaces
                    .iter()
                    .map(|s| {
                        let start = pos;
                        pos += s.chars().count();
                        start
                    })
                    .collect();
                (surfaces.concat(), offsets)
            }
        };
        let highlights = occurrences
            .iter()
            .map(|o| {
                (
                    offsets[o.start],
                    offsets[o.end - 1] + surfaces[o.end - 1].chars().count(),
                )
            })
            .collect();
        others.push(OtherLine {
            text,
            highlights,
            utterance_id: other.utterance_id,
            time_sec: other.time_sec,
            end_sec: end_of(i),
        });
    }
    Ok(Some((current, others)))
}

fn find_entry(state: &AppState, request: &MineRequest) -> Result<TermDictionaryEntry> {
    let result = crate::dict::lookup(state, &request.lookup_text, Some("group"))?;
    result
        .dictionary_entries
        .into_iter()
        .find(|e| {
            e.headwords
                .first()
                .is_some_and(|h| h.term == request.term && h.reading == request.reading)
        })
        .ok_or_else(|| {
            anyhow!(
                "词典里没找到「{}【{}】」，可能刚改过词典设置，重新查一次再制卡",
                request.term,
                request.reading
            )
        })
}

pub fn mine(state: &AppState, request: &MineRequest) -> Result<MineOutcome> {
    let entry = find_entry(state, request)?;

    let (line, others) = match (&request.song_id, request.utterance_id, request.token_index) {
        (Some(song), Some(utterance), Some(index)) => {
            match load_lines(state, &entry, song, utterance, index)? {
                Some((line, others)) => (Some(line), others),
                None => (None, Vec::new()),
            }
        }
        _ => (None, Vec::new()),
    };
    let track = match &request.song_id {
        Some(song) => state.corpus().track(song)?,
        None => None,
    };

    // 单开一条连接读图片和样式：切音频、传文件要一两秒，不占着界面查词用的那条
    let store = DictionaryStore::open(state.dictionaries_path()).context("打不开词典库")?;
    let dictionaries = store.dictionaries()?;
    let mut styles = HashMap::new();
    for d in dictionaries.iter().filter(|d| d.enabled && d.has_styles) {
        styles.insert(d.title.clone(), store.styles(d.id)?);
    }
    let ids: HashMap<String, i64> = dictionaries
        .iter()
        .map(|d| (d.title.clone(), d.id))
        .collect();
    let media = |dictionary: &str, path: &str| -> Option<Vec<u8>> {
        let id = *ids.get(dictionary)?;
        store.media(id, path).ok().flatten().map(|m| m.data)
    };
    let assets = DictionaryAssets {
        styles,
        media: &media,
    };

    let sentence = line.as_ref().map(|l| MineSentence {
        text: &l.text,
        offset: l.offset,
        utterance_id: l.utterance_id,
        time_sec: l.time_sec,
        end_sec: l.end_sec,
    });
    let other_lines: Vec<MineLine<'_>> = others
        .iter()
        .map(|l| MineLine {
            text: &l.text,
            highlights: &l.highlights,
            utterance_id: l.utterance_id,
            time_sec: l.time_sec,
            end_sec: l.end_sec,
        })
        .collect();
    let source = track.as_ref().map(|t| MineSource {
        artist: &t.artist,
        title: &t.title,
        album: &t.album,
        audio_path: &t.audio_path,
        cover_path: &t.cover_path,
    });
    let options = MineOptions {
        deck: request.deck.clone(),
        artist_subdeck: request.artist_subdeck,
        model: request.model.clone(),
        main_dictionary: request.main_dictionary.clone(),
        kind: request.kind,
        tags: vec!["jpop-corpus".to_owned()],
        audio: crate::anki::audio_options(&state.db_path),
    };
    let anki = crate::anki::client();
    jp_anki::mine::mine(
        &anki,
        &entry,
        sentence.as_ref(),
        &other_lines,
        source.as_ref(),
        &assets,
        &options,
    )
    .map_err(|err| anyhow!(crate::anki::describe(&err)))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MineCheck {
    pub connected: bool,
    /// 连不上、没有笔记类型时的提示
    pub message: String,
    /// 和传入的词头一一对应：已有卡片的笔记 id
    pub notes: Vec<Vec<i64>>,
}

/// 这几个词在 Anki 里有没有卡了。Anki 没开不算错误。
pub fn check(anki: &AnkiConnect, model: &str, expressions: &[String]) -> MineCheck {
    let unavailable = |message: String| MineCheck {
        connected: false,
        message,
        notes: vec![Vec::new(); expressions.len()],
    };
    let fields = match anki.model_field_names(model) {
        Ok(fields) if !fields.is_empty() => fields,
        // 自己的 Lyrics 笔记类型第一次制卡时才建，还没建说明还没有卡
        Ok(_) if model == LYRICS => {
            return MineCheck {
                connected: true,
                message: String::new(),
                notes: vec![Vec::new(); expressions.len()],
            };
        }
        Err(AnkiError::Rejected(message)) if model == LYRICS && message.contains("not found") => {
            return MineCheck {
                connected: true,
                message: String::new(),
                notes: vec![Vec::new(); expressions.len()],
            };
        }
        Ok(_) => return unavailable(format!("Anki 里没有「{model}」笔记类型")),
        Err(AnkiError::Rejected(message)) if message.contains("not found") => {
            return unavailable(format!("Anki 里没有「{model}」笔记类型"));
        }
        Err(err) => return unavailable(crate::anki::describe(&err)),
    };
    let first = &fields[0];
    let mut notes = Vec::with_capacity(expressions.len());
    for expression in expressions {
        // 卡上存的是转义过的 HTML，按同样的形式查
        let value = jp_dict::anki::escape(expression);
        match jp_anki::mine::existing_notes(anki, model, first, &value) {
            Ok(ids) => notes.push(ids),
            Err(err) => return unavailable(crate::anki::describe(&err)),
        }
    }
    MineCheck {
        connected: true,
        message: String::new(),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_align_across_the_spaces_that_tokenization_dropped() {
        let surfaces = ["今更", "寂しく", "なっ", "て", "も"];
        assert_eq!(
            align_token_offset("今更　寂しくなっても", &surfaces, 0),
            Some(0)
        );
        assert_eq!(
            align_token_offset("今更　寂しくなっても", &surfaces, 1),
            Some(3)
        );
        assert_eq!(
            align_token_offset("今更　寂しくなっても", &surfaces, 4),
            Some(9)
        );
    }

    #[test]
    fn all_token_offsets_are_aligned_at_once() {
        assert_eq!(
            align_tokens("今更　寂しく", &["今更", "寂しく"]),
            Some(vec![0, 3])
        );
        assert_eq!(align_tokens("短", &["長い"]), None);
        assert_eq!(sentence_key(" 夜が　明ける "), "夜が明ける");
    }

    #[test]
    fn the_subdeck_switch_arrives_under_its_camel_case_name_and_defaults_to_off() {
        let base = serde_json::json!({
            "lookupText": "夜", "term": "夜", "reading": "よる", "songId": "001", "utteranceId": 1, "tokenIndex": 0,
            "deck": "JPOP", "model": "Lyrics", "mainDictionary": null,
        });
        let off: MineRequest = serde_json::from_value(base.clone()).unwrap();
        assert!(!off.artist_subdeck, "旧版界面不传这个字段：默认不开");
        let mut on = base;
        on["artistSubdeck"] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<MineRequest>(on)
                .unwrap()
                .artist_subdeck
        );
    }

    #[test]
    fn tokens_that_do_not_match_the_text_give_up_instead_of_guessing() {
        assert_eq!(align_token_offset("夜が明ける", &["朝", "が"], 1), None);
        // 中间夹着非空白字符：分词和原文对不上
        assert_eq!(align_token_offset("夜Xが", &["夜", "が"], 1), None);
        assert_eq!(align_token_offset("夜が", &["夜", "が"], 5), None);
    }
}
