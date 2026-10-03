//! 一键制卡：查词结果 + 当前歌词行 → 一张 Lyrics（或 Lapis）卡。
//!
//! 默认笔记类型是 `lyrics_model` 里照 [Lapis](https://github.com/donkuri/lapis)（GPL-3.0）做的「Lyrics」，
//! 第一次制卡时自动建；也可以在设置里换成用户自己的 Lapis。字段按 Lapis README 推荐的 Yomitan 标记填：
//!
//! | 字段 | 内容 |
//! |---|---|
//! | Expression / ExpressionReading | `{expression}` / `{reading}` |
//! | ExpressionFurigana | `{furigana-plain}` |
//! | MainDefinition | 首选词典的 `{single-glossary-…}`；这个词首选词典里没有时，退到排在最前、有释义的那本 |
//! | Sentence | 当前歌词行，查的那个词加粗（`{cloze-prefix}<b>{cloze-body}</b>{cloze-suffix}`）；这首歌里其他含这个词的句子接在后面，文字相同的只放一次 |
//! | SentenceAudio | 每一句各切一段，顺序和 Sentence 一致；当前句的音频切不出来时在结果里给出原因 |
//! | Picture | 专辑封面 |
//! | Glossary | `{glossary}`（所有词典） |
//! | PitchPosition / PitchCategories | `{pitch-accent-positions}` / `{pitch-accent-categories}` |
//! | Frequency / FreqSort | `{frequencies}` / `{frequency-harmonic-rank}` |
//! | MiscInfo | 歌手「歌名」 时间 |
//! | SongTitle / Artist / Album | 歌名、歌手、专辑（只有 Lyrics 有这几个字段，显示在封面旁边） |
//!
//! 字段的 HTML 由 `jp_dict::anki` 生成，已和 Yomitan 自己的制卡期望结果逐字对账。
//! ExpressionAudio 留空：单词读音要联网取，本项目不用远程服务。SentenceFurigana 按 Lapis 的建议留空。
//!
//! 和导出一样，**先查重再切音频、传图片**，重复时不在 Anki 媒体库里留孤儿文件。

use std::collections::HashMap;
use std::path::Path;

use jp_dict::anki::{EntryKind, MediaResolver, NoteContext, NoteRenderer};
use jp_dict::translator::TermDictionaryEntry;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::audio;
use crate::card::md5;
use crate::connect::{AnkiConnect, AnkiError};

pub const LAPIS: &str = "Lapis";
pub use crate::lyrics_model::NOTE_TYPE as LYRICS;

/// Lapis 用几个开关字段决定出哪种卡；都空是单词卡。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LapisCardKind {
    Vocab,
    /// 用户现有的 Lapis 卡全是这一种
    #[default]
    WordAndSentence,
    Click,
    Sentence,
    Audio,
}

impl LapisCardKind {
    fn flag_field(self) -> Option<&'static str> {
        match self {
            Self::Vocab => None,
            Self::WordAndSentence => Some("IsWordAndSentenceCard"),
            Self::Click => Some("IsClickCard"),
            Self::Sentence => Some("IsSentenceCard"),
            Self::Audio => Some("IsAudioCard"),
        }
    }
}

/// 例句：一行歌词，以及查词起点在行里的位置（按字符）。
pub struct MineSentence<'a> {
    pub text: &'a str,
    pub offset: usize,
    pub utterance_id: i64,
    pub time_sec: Option<f64>,
    pub end_sec: Option<f64>,
}

/// 同一首歌里其他含这个词的句子
pub struct MineLine<'a> {
    pub text: &'a str,
    /// 要加粗的范围（按字符，左闭右开）
    pub highlights: &'a [(usize, usize)],
    pub utterance_id: i64,
    pub time_sec: Option<f64>,
    pub end_sec: Option<f64>,
}

pub struct MineSource<'a> {
    pub artist: &'a str,
    pub title: &'a str,
    pub album: &'a str,
    pub audio_path: &'a str,
    pub cover_path: &'a str,
}

/// 词典给的东西：自带样式，以及按（词典名, 路径）取图片字节
pub struct DictionaryAssets<'a> {
    pub styles: HashMap<String, String>,
    pub media: &'a dyn Fn(&str, &str) -> Option<Vec<u8>>,
}

pub struct MineOptions {
    /// 牌组。开了 `artist_subdeck` 时是父牌组
    pub deck: String,
    /// 放进「牌组::歌手」子牌组，没有就建（见 [`artist_subdeck`]）。卡不是从歌里来的就还放在 `deck`
    pub artist_subdeck: bool,
    pub model: String,
    /// 首选释义词典（MainDefinition）
    pub main_dictionary: Option<String>,
    pub kind: LapisCardKind,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MineOutcome {
    #[serde(rename_all = "camelCase")]
    Added {
        note_id: i64,
        /// 实际放进的牌组
        deck: String,
        /// 这个子牌组是这次新建的
        deck_created: bool,
        /// Lapis 该有、这个笔记类型却没有的字段（旧版 Lapis），这些内容没写进去
        skipped_fields: Vec<String>,
        /// 卡加上了，但有东西没做成（比如当前句的音频没切出来）
        warnings: Vec<String>,
    },
    /// 已经有这个词的卡了，没有再加
    #[serde(rename_all = "camelCase")]
    Duplicate { note_ids: Vec<i64> },
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// 文字按字符范围加粗，其余转义
fn highlighted(text: &str, ranges: &[(usize, usize)]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut ranges: Vec<(usize, usize)> =
        ranges.iter().map(|&(a, b)| (a.min(chars.len()), b.min(chars.len()))).filter(|(a, b)| a < b).collect();
    ranges.sort_unstable();
    let mut out = String::new();
    let mut pos = 0;
    for (start, end) in ranges {
        if start < pos {
            continue;
        }
        out.push_str(&escape_text(&String::from_iter(&chars[pos..start])));
        out.push_str("<b>");
        out.push_str(&escape_text(&String::from_iter(&chars[start..end])));
        out.push_str("</b>");
        pos = end;
    }
    out.push_str(&escape_text(&String::from_iter(&chars[pos..])));
    out
}

fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_default()
}

/// 词典图片在 Anki 媒体库里的名字：按（词典, 路径）固定，同一张图不会存出一堆副本
pub fn dictionary_media_name(dictionary: &str, path: &str) -> String {
    let key = format!("{dictionary}\u{0}{path}");
    format!("jpop_dict_{}{}", hex(&md5(key.as_bytes())), extension(path))
}

fn cover_media_name(cover_path: &str) -> String {
    format!("jpop_cover_{}{}", hex(&md5(cover_path.as_bytes())), extension(cover_path))
}

/// 首选词典里有释义就用它，否则用排在最前的那本
pub fn main_dictionary_for<'e>(entry: &'e TermDictionaryEntry, preferred: Option<&str>) -> Option<&'e str> {
    preferred
        .and_then(|p| entry.definitions.iter().find(|d| d.dictionary == p))
        .or_else(|| entry.definitions.first())
        .map(|d| d.dictionary.as_str())
}

/// （字段名, 内容），按 Lapis 的字段顺序
pub type NoteFields = Vec<(String, String)>;
/// （词典名, 词典内路径）
pub type MediaKeys = Vec<(String, String)>;

/// 纯文本字段（不含音频、图片引用）。返回（字段, 还没存进 Anki 的词典图片）。
pub fn lapis_fields(
    entry: &TermDictionaryEntry,
    sentence: Option<&MineSentence<'_>>,
    other_lines: &[MineLine<'_>],
    source: Option<&MineSource<'_>>,
    options: &MineOptions,
    styles: &HashMap<String, String>,
    media_files: &HashMap<(String, String), String>,
) -> (NoteFields, MediaKeys) {
    let ctx = NoteContext {
        sentence: sentence.map_or("", |s| s.text),
        offset: sentence.map_or(0, |s| s.offset),
        document_title: "",
        dictionary_styles: styles,
    };
    let renderer = NoteRenderer::new(entry, EntryKind::TermGrouped, &ctx);
    let mut media = MediaResolver { files: media_files, missing: Vec::new() };
    let mut marker = |m: &str| renderer.render(m, &mut media).unwrap_or_default();

    let expression = marker("expression");
    let furigana = marker("furigana-plain");
    let reading = marker("reading");
    let glossary = marker("glossary");
    let pitch_position = marker("pitch-accent-positions");
    let pitch_categories = marker("pitch-accent-categories");
    let frequency = marker("frequencies");
    let freq_sort = marker("frequency-harmonic-rank");
    let mut sentences: Vec<String> = Vec::new();
    if sentence.is_some() {
        sentences.push(format!(
            "{}<b>{}</b>{}",
            escape_text(&marker("cloze-prefix")),
            escape_text(&marker("cloze-body")),
            escape_text(&marker("cloze-suffix"))
        ));
    }
    sentences.extend(other_lines.iter().map(|l| highlighted(l.text, l.highlights)));
    let sentence_html = sentences.join("<br>");
    let main_definition = match main_dictionary_for(entry, options.main_dictionary.as_deref()) {
        Some(dictionary) => renderer.single_glossary(dictionary, false, false, &mut media),
        None => String::new(),
    };
    let misc = source
        .map(|s| {
            let time = sentence.and_then(|l| l.time_sec).map(|t| format!(" {}", clock(t))).unwrap_or_default();
            escape_text(&format!("{}「{}」{time}", s.artist, s.title))
        })
        .unwrap_or_default();

    let mut fields = vec![
        ("Expression", expression),
        ("ExpressionFurigana", furigana),
        ("ExpressionReading", reading),
        ("ExpressionAudio", String::new()),
        ("SelectionText", String::new()),
        ("MainDefinition", main_definition),
        ("DefinitionPicture", String::new()),
        ("Sentence", sentence_html),
        ("SentenceFurigana", String::new()),
        ("SentenceAudio", String::new()),
        ("Picture", String::new()),
        ("Glossary", glossary),
        ("Hint", String::new()),
        ("IsWordAndSentenceCard", String::new()),
        ("IsClickCard", String::new()),
        ("IsSentenceCard", String::new()),
        ("IsAudioCard", String::new()),
        ("PitchPosition", pitch_position),
        ("PitchCategories", pitch_categories),
        ("Frequency", frequency),
        ("FreqSort", freq_sort),
        ("MiscInfo", misc),
        ("SongTitle", source.map(|s| escape_text(s.title)).unwrap_or_default()),
        ("Artist", source.map(|s| escape_text(s.artist)).unwrap_or_default()),
        ("Album", source.map(|s| escape_text(s.album)).unwrap_or_default()),
    ];
    if let Some(flag) = options.kind.flag_field()
        && let Some((_, value)) = fields.iter_mut().find(|(name, _)| *name == flag)
    {
        *value = "x".to_owned();
    }
    let fields = fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    (fields, media.missing)
}

fn note_json(deck: &str, options: &MineOptions, fields: &[(String, String)], available: &[String]) -> Value {
    let map: Map<String, Value> = fields
        .iter()
        .filter(|(name, _)| available.contains(name))
        .map(|(name, value)| (name.clone(), Value::String(value.clone())))
        .collect();
    json!({
        "deckName": deck,
        "modelName": options.model,
        "fields": map,
        "tags": options.tags,
        // 和 Yomitan 默认一致：整个收藏里同一笔记类型、首字段相同算重复
        "options": { "allowDuplicate": false, "duplicateScope": "collection" },
    })
}

/// 按歌手放的子牌组名：「父牌组::歌手」。
///
/// 合作曲（`ずっと真夜中でいいのに。/森カリオペ`）取第一位——曲库里的歌手串都是主唱在前，
/// 导入时演唱署名也是按 `/` 拆的。`::` 在 Anki 里是层级分隔符，歌手名里有的话换成单个冒号，
/// 免得多出一层。歌手是空的返回 None（卡还放在父牌组）。
pub fn artist_subdeck(parent: &str, artist: &str) -> Option<String> {
    let mut name = artist.split('/').next().unwrap_or("").trim().to_owned();
    while name.contains("::") {
        name = name.replace("::", ":");
    }
    let name = name.trim();
    if name.is_empty() || parent.trim().is_empty() {
        return None;
    }
    Some(format!("{}::{name}", parent.trim()))
}

/// 牌组有就用（Anki 的牌组名不分大小写，用 Anki 里原来的写法），没有就建。返回（牌组名, 是不是新建的）
fn ensure_deck(anki: &AnkiConnect, deck: &str) -> Result<(String, bool), AnkiError> {
    let wanted = deck.to_lowercase();
    if let Some(existing) = anki.deck_names()?.into_iter().find(|d| d.to_lowercase() == wanted) {
        return Ok((existing, false));
    }
    anki.call("createDeck", json!({ "deck": deck }))?;
    Ok((deck.to_owned(), true))
}

/// Anki 搜索语法里要转义的字符
fn search_escape(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if matches!(c, '\\' | '"' | '*' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 按首字段找已有的卡（笔记类型内）
pub fn existing_notes(anki: &AnkiConnect, model: &str, first_field: &str, value: &str) -> Result<Vec<i64>, AnkiError> {
    let query = format!("\"note:{}\" \"{}:{}\"", search_escape(model), search_escape(first_field), search_escape(value));
    anki.find_notes(&query)
}

fn set_field(fields: &mut [(String, String)], name: &str, value: String) {
    if let Some((_, v)) = fields.iter_mut().find(|(n, _)| n == name) {
        *v = value;
    }
}

/// 切一句的音频存进 Anki，返回文件名；切不出来时返回原因
fn clip_line(
    anki: &AnkiConnect,
    song: Option<&MineSource<'_>>,
    time_sec: Option<f64>,
    end_sec: Option<f64>,
) -> Result<Result<String, String>, AnkiError> {
    let Some(song) = song.filter(|s| !s.audio_path.is_empty()) else {
        return Ok(Err("这首歌没有音频文件".into()));
    };
    let Some(start) = time_sec else { return Ok(Err("这一行歌词没有时间轴".into())) };
    let clip = match audio::clip(Path::new(song.audio_path), start, end_sec) {
        Ok(clip) => clip,
        Err(reason) => return Ok(Err(reason)),
    };
    // 用 Anki 实际存下的名字，见 `audio::clip_name` 和 `store_media_file`
    Ok(Ok(anki.store_media_file(&clip.name, &clip.mp3)?))
}

pub fn mine(
    anki: &AnkiConnect,
    entry: &TermDictionaryEntry,
    sentence: Option<&MineSentence<'_>>,
    other_lines: &[MineLine<'_>],
    source: Option<&MineSource<'_>>,
    assets: &DictionaryAssets<'_>,
    options: &MineOptions,
) -> Result<MineOutcome, AnkiError> {
    // 自己的 Lyrics 笔记类型：没有就建，缺字段就补
    if options.model == LYRICS {
        crate::lyrics_model::ensure(anki)?;
    }
    let available = match anki.model_field_names(&options.model) {
        Ok(fields) if !fields.is_empty() => fields,
        Ok(_) => return Err(AnkiError::Rejected(format!("Anki 里没有「{}」笔记类型", options.model))),
        Err(AnkiError::Rejected(message)) if message.contains("not found") => {
            return Err(AnkiError::Rejected(format!("Anki 里没有「{}」笔记类型", options.model)));
        }
        Err(err) => return Err(err),
    };
    if let Some(message) = crate::export::missing_deck_message(anki, &options.deck)? {
        return Err(AnkiError::Rejected(message));
    }

    let no_media = HashMap::new();
    let (mut fields, missing) = lapis_fields(entry, sentence, other_lines, source, options, &assets.styles, &no_media);
    let first_field = available.first().cloned().unwrap_or_else(|| "Expression".to_owned());
    let first_value = fields.iter().find(|(n, _)| *n == first_field).map(|(_, v)| v.clone()).unwrap_or_default();

    // 查重用父牌组：范围是整个收藏，和牌组无关；而子牌组可能还没建，canAddNotes 找不到牌组会直接回 false，
    // 被当成重复。重复的词也就不会留下一个空的子牌组
    if !anki.can_add_note(&note_json(&options.deck, options, &fields, &available))? {
        let note_ids = existing_notes(anki, &options.model, &first_field, &first_value)?;
        return Ok(MineOutcome::Duplicate { note_ids });
    }

    // 释义里的词典图片：先存进 Anki 媒体库，再带上文件名重新渲染
    let mut files = HashMap::new();
    for (dictionary, path) in missing {
        if let Some(bytes) = (assets.media)(&dictionary, &path) {
            // 同一本词典换了版本、同一路径的图变了时，Anki 会改名而不是覆盖旧卡的图
            let name = anki.store_media_file(&dictionary_media_name(&dictionary, &path), &bytes)?;
            files.insert((dictionary, path), name);
        }
    }
    if !files.is_empty() {
        fields = lapis_fields(entry, sentence, other_lines, source, options, &assets.styles, &files).0;
    }

    // 例句音频：当前句一定要有，切不出来要说清楚；其他句子能切几句算几句
    let mut warnings = Vec::new();
    let mut sounds = Vec::new();
    if let Some(line) = sentence {
        match clip_line(anki, source, line.time_sec, line.end_sec)? {
            Ok(name) => sounds.push(format!("[sound:{name}]")),
            Err(reason) => warnings.push(format!("当前句没有音频：{reason}")),
        }
    }
    let mut failed_others = 0;
    for line in other_lines {
        match clip_line(anki, source, line.time_sec, line.end_sec)? {
            Ok(name) => sounds.push(format!("[sound:{name}]")),
            Err(_) => failed_others += 1,
        }
    }
    if failed_others > 0 {
        warnings.push(format!("另有 {failed_others} 句没切出音频"));
    }
    set_field(&mut fields, "SentenceAudio", sounds.concat());

    if let Some(song) = source
        && !song.cover_path.is_empty()
        && let Ok(bytes) = std::fs::read(song.cover_path)
    {
        // 封面重新刮削过、同一路径换了图时，Anki 会改名而不是覆盖旧卡的图
        let name = anki.store_media_file(&cover_media_name(song.cover_path), &bytes)?;
        set_field(&mut fields, "Picture", format!("<img src=\"{name}\">"));
    }

    let skipped_fields: Vec<String> = fields
        .iter()
        .filter(|(name, value)| {
            // 歌曲信息那几个是 Lyrics 额外加的，Lapis 没有很正常，不算漏写
            !value.is_empty() && !available.contains(name) && !crate::lyrics_model::SONG_FIELDS.contains(&name.as_str())
        })
        .map(|(name, _)| name.clone())
        .collect();
    // 子牌组放到最后才建：前面哪一步失败都不会留下空牌组
    let (deck, deck_created) = match source.filter(|_| options.artist_subdeck).and_then(|s| artist_subdeck(&options.deck, s.artist)) {
        Some(target) => ensure_deck(anki, &target)?,
        None => (options.deck.clone(), false),
    };
    match anki.call("addNote", json!({ "note": note_json(&deck, options, &fields, &available) })) {
        Ok(id) => Ok(MineOutcome::Added { note_id: id.as_i64().unwrap_or_default(), deck, deck_created, skipped_fields, warnings }),
        // 查重和加卡之间用户可能在 Anki 里手动加了同一个词
        Err(AnkiError::Rejected(message)) if message.to_lowercase().contains("duplicate") => {
            Ok(MineOutcome::Duplicate { note_ids: existing_notes(anki, &options.model, &first_field, &first_value)? })
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use jp_dict::import::{DirArchive, ImportOptions, import_dictionary};
    use jp_dict::store::DictionaryStore;
    use jp_dict::translator::{EnabledDictionary, FindTermsMode, FindTermsOptions, Translator};

    use super::*;
    use crate::connect::testing::{FakeAnki, client};

    fn lookup(text: &str) -> (DictionaryStore, TermDictionaryEntry) {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../jp-dict/tests/fixtures/yomitan-translator/valid-dictionary1");
        let mut store = DictionaryStore::open_in_memory().unwrap();
        let import = ImportOptions { image_size: |_, _| Some((100, 100)), ..ImportOptions::default() };
        import_dictionary(&mut store, &mut DirArchive::open(&dir).unwrap(), &import, &mut |_| {}).unwrap();
        let title = store.dictionaries().unwrap()[0].title.clone();
        let options = FindTermsOptions {
            dictionaries: vec![EnabledDictionary {
                alias: title.clone(),
                title,
                index: 0,
                parts_of_speech_filter: true,
                use_deinflections: true,
            }],
            deinflect: true,
            remove_non_japanese_characters: true,
            primary_reading: String::new(),
            sort_frequency_dictionary: None,
            sort_frequency_ascending: true,
            use_all_frequency_dictionaries: false,
            max_results: Some(32),
        };
        let result = Translator::new().find_terms(&store, FindTermsMode::Group, text, &options).unwrap();
        let entry = result.dictionary_entries.into_iter().next().expect("测试词典里应当查得到");
        (store, entry)
    }

    fn options() -> MineOptions {
        MineOptions {
            deck: "Lapis".into(),
            artist_subdeck: false,
            model: LAPIS.into(),
            main_dictionary: None,
            kind: LapisCardKind::WordAndSentence,
            tags: vec!["jpop-corpus".into()],
        }
    }

    fn lapis_fields_list() -> Value {
        json!([
            "Expression", "ExpressionFurigana", "ExpressionReading", "ExpressionAudio", "SelectionText",
            "MainDefinition", "DefinitionPicture", "Sentence", "SentenceFurigana", "SentenceAudio", "Picture",
            "Glossary", "Hint", "IsWordAndSentenceCard", "IsClickCard", "IsSentenceCard", "IsAudioCard",
            "PitchPosition", "PitchCategories", "Frequency", "FreqSort", "MiscInfo"
        ])
    }

    fn no_assets() -> impl Fn(&str, &str) -> Option<Vec<u8>> {
        |_: &str, _: &str| None
    }

    #[test]
    fn the_sentence_bolds_the_inflected_form_and_the_card_is_word_and_sentence() {
        let (_store, entry) = lookup("打ち込んでいませんでした");
        let line = MineSentence { text: "もう打ち込んでいませんでした<ね>", offset: 2, utterance_id: 7, time_sec: Some(64.5), end_sec: None };
        let song = MineSource { artist: "ヨルシカ", title: "夜行", album: "負け犬にアンコールはいらない", audio_path: "", cover_path: "" };
        let others = [MineLine { text: "打ち込んで　<また>", highlights: &[(0, 5)], utterance_id: 9, time_sec: None, end_sec: None }];
        let (fields, _) =
            lapis_fields(&entry, Some(&line), &others, Some(&song), &options(), &HashMap::new(), &HashMap::new());
        let get = |name: &str| fields.iter().find(|(n, _)| n == name).unwrap().1.clone();
        assert_eq!(get("Expression"), "打ち込む");
        assert_eq!(get("ExpressionFurigana"), "打[う]ち 込[こ]む");
        assert_eq!(get("Sentence"), "もう<b>打ち込んでいませんでした</b>&lt;ね&gt;<br><b>打ち込んで</b>　&lt;また&gt;");
        assert_eq!(get("IsWordAndSentenceCard"), "x");
        assert_eq!(get("MiscInfo"), "ヨルシカ「夜行」 1:04");
        assert_eq!(get("SongTitle"), "夜行");
        assert_eq!(get("Artist"), "ヨルシカ");
        assert_eq!(get("Album"), "負け犬にアンコールはいらない");
        assert_eq!(get("FreqSort"), "3");
        assert!(get("MainDefinition").contains("<li data-dictionary="), "{}", get("MainDefinition"));
    }

    #[test]
    fn a_duplicate_is_reported_without_uploading_anything() {
        let (_store, entry) = lookup("打ち込む");
        let fake = Arc::new(FakeAnki::new(&[
            ("modelFieldNames", lapis_fields_list()),
            ("deckNames", json!(["Lapis"])),
            ("canAddNotes", json!([false])),
            ("findNotes", json!([111, 222])),
        ]));
        let media = no_assets();
        let assets = DictionaryAssets { styles: HashMap::new(), media: &media };
        let outcome = mine(&client(fake.clone()), &entry, None, &[], None, &assets, &options()).unwrap();
        assert_eq!(outcome, MineOutcome::Duplicate { note_ids: vec![111, 222] });
        assert!(!fake.actions().iter().any(|a| a == "storeMediaFile" || a == "addNote"), "{:?}", fake.actions());
        assert_eq!(fake.params_of("findNotes").unwrap()["query"], r#""note:Lapis" "Expression:打ち込む""#);
    }

    #[test]
    fn dictionary_images_and_the_cover_are_stored_before_the_note_is_added() {
        let (_store, entry) = lookup("画像");
        let cover = std::env::temp_dir().join(format!("jp_anki_mine_cover_{}.PNG", std::process::id()));
        std::fs::write(&cover, b"png").unwrap();
        let fake = Arc::new(FakeAnki::new(&[
            ("modelFieldNames", lapis_fields_list()),
            ("deckNames", json!(["Lapis"])),
            ("canAddNotes", json!([true])),
            ("addNote", json!(4242)),
        ]));
        let media = |dictionary: &str, path: &str| (path == "image.gif").then(|| format!("{dictionary}:{path}").into_bytes());
        let assets = DictionaryAssets { styles: HashMap::new(), media: &media };
        let cover_path = cover.to_string_lossy().to_string();
        let song = MineSource { artist: "A", title: "B", album: "", audio_path: "", cover_path: &cover_path };
        let outcome = mine(&client(fake.clone()), &entry, None, &[], Some(&song), &assets, &options());
        let _ = std::fs::remove_file(&cover);
        assert_eq!(
            outcome.unwrap(),
            MineOutcome::Added { note_id: 4242, deck: "Lapis".into(), deck_created: false, skipped_fields: vec![], warnings: vec![] }
        );

        let stored: Vec<String> =
            fake.all_params_of("storeMediaFile").iter().map(|p| p["filename"].as_str().unwrap().to_owned()).collect();
        assert_eq!(stored.len(), 2, "{stored:?}");
        assert!(stored[0].starts_with("jpop_dict_") && stored[0].ends_with(".gif"));
        assert!(stored[1].starts_with("jpop_cover_") && stored[1].ends_with(".png"));
        let actions = fake.actions();
        assert!(actions.iter().rposition(|a| a == "storeMediaFile") < actions.iter().position(|a| a == "addNote"));

        let note = &fake.params_of("addNote").unwrap()["note"];
        let fields = &note["fields"];
        assert!(fields["Glossary"].as_str().unwrap().contains(&format!("src=\"{}\"", stored[0])));
        assert_eq!(fields["Picture"], format!("<img src=\"{}\">", stored[1]));
        assert_eq!(note["deckName"], "Lapis");
        assert_eq!(note["tags"], json!(["jpop-corpus"]));
    }

    #[test]
    fn a_missing_note_type_is_a_clear_error() {
        let (_store, entry) = lookup("打ち込む");
        let fake = Arc::new(FakeAnki::new(&[]).with_error("modelFieldNames", "model was not found: Lapis"));
        let media = no_assets();
        let assets = DictionaryAssets { styles: HashMap::new(), media: &media };
        let err = mine(&client(fake), &entry, None, &[], None, &assets, &options()).unwrap_err();
        assert!(matches!(&err, AnkiError::Rejected(m) if m == "Anki 里没有「Lapis」笔记类型"), "{err:?}");
    }

    #[test]
    fn a_missing_current_sentence_audio_is_reported_not_silently_dropped() {
        let (_store, entry) = lookup("打ち込む");
        let fake = Arc::new(FakeAnki::new(&[
            ("modelFieldNames", lapis_fields_list()),
            ("deckNames", json!(["Lapis"])),
            ("canAddNotes", json!([true])),
            ("addNote", json!(7)),
        ]));
        let media = no_assets();
        let assets = DictionaryAssets { styles: HashMap::new(), media: &media };
        let line = MineSentence { text: "打ち込む", offset: 0, utterance_id: 1, time_sec: Some(3.0), end_sec: None };
        let others = [MineLine { text: "打ち込む日々", highlights: &[(0, 4)], utterance_id: 2, time_sec: None, end_sec: None }];
        let song = MineSource { artist: "A", title: "B", album: "", audio_path: "", cover_path: "" };
        let outcome =
            mine(&client(fake.clone()), &entry, Some(&line), &others, Some(&song), &assets, &options()).unwrap();
        let MineOutcome::Added { warnings, .. } = outcome else { panic!("应当加上卡") };
        assert_eq!(warnings, ["当前句没有音频：这首歌没有音频文件", "另有 1 句没切出音频"]);
        let note = &fake.params_of("addNote").unwrap()["note"];
        assert_eq!(note["fields"]["Sentence"], "<b>打ち込む</b><br><b>打ち込む</b>日々");
        assert_eq!(note["fields"]["SentenceAudio"], "");
    }

    #[test]
    fn the_lyrics_note_type_is_created_on_first_use() {
        let (_store, entry) = lookup("打ち込む");
        let mut fields: Vec<&str> = crate::lyrics_model::FIELDS.to_vec();
        fields.dedup();
        let fake = Arc::new(FakeAnki::new(&[
            ("modelNames", json!(["Lapis"])),
            ("createModel", json!({})),
            ("modelFieldNames", json!(fields)),
            ("deckNames", json!(["JPOP"])),
            ("canAddNotes", json!([true])),
            ("addNote", json!(1)),
        ]));
        let media = no_assets();
        let assets = DictionaryAssets { styles: HashMap::new(), media: &media };
        let options = MineOptions { deck: "JPOP".into(), model: LYRICS.into(), ..options() };
        let song = MineSource { artist: "ヨルシカ", title: "夜行", album: "", audio_path: "", cover_path: "" };
        mine(&client(fake.clone()), &entry, None, &[], Some(&song), &assets, &options).unwrap();
        let actions = fake.actions();
        let created = actions.iter().position(|a| a == "createModel").expect("应当建笔记类型");
        assert!(created < actions.iter().position(|a| a == "addNote").unwrap());
        let note = &fake.params_of("addNote").unwrap()["note"];
        assert_eq!(note["modelName"], "Lyrics");
        assert_eq!(note["fields"]["Artist"], "ヨルシカ");
    }

    #[test]
    fn subdeck_names_take_the_first_performer_and_never_add_a_level() {
        assert_eq!(artist_subdeck("JPOP", "ヨルシカ").as_deref(), Some("JPOP::ヨルシカ"));
        assert_eq!(artist_subdeck("JPOP", "ずっと真夜中でいいのに。/森カリオペ").as_deref(), Some("JPOP::ずっと真夜中でいいのに。"));
        assert_eq!(artist_subdeck("日本語::歌詞", " Mrs. GREEN APPLE ").as_deref(), Some("日本語::歌詞::Mrs. GREEN APPLE"));
        assert_eq!(artist_subdeck("JPOP", "A::B:::C").as_deref(), Some("JPOP::A:B:C"));
        assert_eq!(artist_subdeck("JPOP", " / feat"), None);
        assert_eq!(artist_subdeck("", "ヨルシカ"), None);
    }

    fn lyrics_fake(decks: Value, can_add: bool) -> Arc<FakeAnki> {
        let mut fields: Vec<&str> = crate::lyrics_model::FIELDS.to_vec();
        fields.extend(crate::lyrics_model::SONG_FIELDS);
        Arc::new(FakeAnki::new(&[
            ("modelNames", json!(["Lyrics"])),
            ("modelFieldNames", json!(fields)),
            ("modelStyling", json!({ "css": crate::lyrics_model::CSS })),
            ("deckNames", decks),
            ("canAddNotes", json!([can_add])),
            ("findNotes", json!([5])),
            ("createDeck", json!(99)),
            ("addNote", json!(1)),
        ]))
    }

    fn mine_into_subdeck(fake: &Arc<FakeAnki>, artist: &str) -> MineOutcome {
        let (_store, entry) = lookup("打ち込む");
        let media = no_assets();
        let assets = DictionaryAssets { styles: HashMap::new(), media: &media };
        let options = MineOptions { deck: "JPOP".into(), artist_subdeck: true, model: LYRICS.into(), ..options() };
        let song = MineSource { artist, title: "夜行", album: "", audio_path: "", cover_path: "" };
        mine(&client(fake.clone()), &entry, None, &[], Some(&song), &assets, &options).unwrap()
    }

    #[test]
    fn a_missing_artist_subdeck_is_created_after_the_duplicate_check() {
        let fake = lyrics_fake(json!(["Default", "JPOP"]), true);
        let outcome = mine_into_subdeck(&fake, "ヨルシカ");
        let MineOutcome::Added { deck, deck_created, .. } = outcome else { panic!("应当加上卡") };
        assert_eq!((deck.as_str(), deck_created), ("JPOP::ヨルシカ", true));
        assert_eq!(fake.params_of("createDeck").unwrap()["deck"], "JPOP::ヨルシカ");
        // 查重按父牌组问（子牌组那时还不存在），加卡进子牌组
        assert_eq!(fake.params_of("canAddNotes").unwrap()["notes"][0]["deckName"], "JPOP");
        assert_eq!(fake.params_of("addNote").unwrap()["note"]["deckName"], "JPOP::ヨルシカ");
        let actions = fake.actions();
        assert!(actions.iter().position(|a| a == "createDeck") < actions.iter().position(|a| a == "addNote"));
    }

    #[test]
    fn an_existing_subdeck_is_reused_whatever_its_case() {
        let fake = lyrics_fake(json!(["JPOP", "JPOP::Vaundy"]), true);
        let MineOutcome::Added { deck, deck_created, .. } = mine_into_subdeck(&fake, "vaundy") else { panic!("应当加上卡") };
        assert_eq!((deck.as_str(), deck_created), ("JPOP::Vaundy", false));
        assert!(!fake.actions().iter().any(|a| a == "createDeck"));
        assert_eq!(fake.params_of("addNote").unwrap()["note"]["deckName"], "JPOP::Vaundy");
    }

    #[test]
    fn a_duplicate_never_leaves_an_empty_subdeck_behind() {
        let fake = lyrics_fake(json!(["JPOP"]), false);
        assert_eq!(mine_into_subdeck(&fake, "ヨルシカ"), MineOutcome::Duplicate { note_ids: vec![5] });
        assert!(!fake.actions().iter().any(|a| a == "createDeck" || a == "addNote"), "{:?}", fake.actions());
    }

    #[test]
    fn cards_not_from_a_song_stay_in_the_parent_deck() {
        let (_store, entry) = lookup("打ち込む");
        let fake = lyrics_fake(json!(["JPOP"]), true);
        let media = no_assets();
        let assets = DictionaryAssets { styles: HashMap::new(), media: &media };
        let options = MineOptions { deck: "JPOP".into(), artist_subdeck: true, model: LYRICS.into(), ..options() };
        let MineOutcome::Added { deck, deck_created, .. } =
            mine(&client(fake.clone()), &entry, None, &[], None, &assets, &options).unwrap()
        else {
            panic!("应当加上卡")
        };
        assert_eq!((deck.as_str(), deck_created), ("JPOP", false));
        assert!(!fake.actions().iter().any(|a| a == "createDeck"));
    }

    #[test]
    fn highlights_escape_text_and_ignore_overlaps() {
        assert_eq!(highlighted("a<b夜c夜", &[(3, 4), (5, 6), (3, 5)]), "a&lt;b<b>夜</b>c<b>夜</b>");
        assert_eq!(highlighted("夜", &[(0, 9)]), "<b>夜</b>");
    }

    #[test]
    fn media_names_are_stable_and_keep_the_extension() {
        assert_eq!(dictionary_media_name("D", "gaiji/一.SVG"), dictionary_media_name("D", "gaiji/一.SVG"));
        assert_ne!(dictionary_media_name("D", "a.svg"), dictionary_media_name("E", "a.svg"));
        assert!(dictionary_media_name("D", "gaiji/一.SVG").ends_with(".svg"));
        assert_eq!(search_escape(r#"a_b*"c\"#), r#"a\_b\*\"c\\"#);
    }
}
