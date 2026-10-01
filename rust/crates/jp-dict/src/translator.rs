//! 查词：Yomitan `translator.js` 的 `findTerms` 移植（GPL-3.0-or-later，Copyright (C) 2023-2026 Yomitan Authors）。
//!
//! 支持 simple / split / group / term 四种模式；merge 模式依赖「主词典 + 序号」合并，暂未移植。
//! 逐函数照抄，包括几处不直观、但会影响结果的细节：
//! - 规则链数组在 JS 里按引用共享（同一次还原出来的多个条目共用一个数组，合并时一改都改），
//!   这里用 `Rc<RefCell<_>>` 保持同样的共享关系；
//! - 标签先按「词典 + 标签名」收集，最后统一查库展开、合并同名同类、按 (order, 名字) 排序，
//!   展开时各标签的先后取决于它在**所有**标签列表里第一次出现的位置；
//! - 排序各项都相等时保留输入顺序（JS 的 `Array.sort` 稳定，Rust 的 `sort_by` 也稳定），
//!   所以数据库返回顺序也照 IndexedDB 做了（见 `store`）。
//!
//! 有意不同的地方：
//! - 字符串长度按 Unicode 字符计（JS 是 UTF-16 码元），只有 BMP 以外的字才会不同；
//!   `original_text_length` 因此也是字符数，界面截取原文时要按字符。
//! - JS 用 `Intl.Collator('en-US')` 比较名字，这里没有搬 ICU 排序表：ASCII 不分大小写、
//!   同字母小写在前，其余按码位。只在标签 order 相同、或词条其余排序键全部相等时才用得到。

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use crate::deinflect::{InflectionRuleChainCandidate, InflectionSource, algorithm_deinflections};
use crate::store::{
    DictionaryStore, GLOSSARY_HAS_CONTENT, GLOSSARY_HAS_FORM_OF, GlossaryRef, MatchSource, TagRow, TermRow,
};
use crate::text::japanese_chinese_korean_only_prefix;
use crate::transformer::{LanguageTransformer, conditions_match};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

// ---------------------------------------------------------------- 选项与输出

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindTermsMode {
    Simple,
    Split,
    Group,
    Term,
}

#[derive(Debug, Clone)]
pub struct EnabledDictionary {
    pub title: String,
    /// 排序用的词典顺序（JS enabledDictionaryMap 里的 index）
    pub index: usize,
    pub alias: String,
    pub parts_of_speech_filter: bool,
    pub use_deinflections: bool,
}

#[derive(Debug, Clone)]
pub struct FindTermsOptions {
    pub dictionaries: Vec<EnabledDictionary>,
    pub deinflect: bool,
    pub remove_non_japanese_characters: bool,
    pub primary_reading: String,
    pub sort_frequency_dictionary: Option<String>,
    pub sort_frequency_ascending: bool,
    pub use_all_frequency_dictionaries: bool,
    /// 排序后只保留前 N 条，在读释义之前截断（界面一次看不了几十条，解压释义是大头）。
    /// `None` 为不截断，和 Yomitan `findTerms` 本身一致。
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub name: String,
    pub category: String,
    pub order: f64,
    pub score: f64,
    pub content: Vec<String>,
    pub dictionaries: Vec<String>,
    pub redundant: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermSource {
    pub original_text: String,
    pub transformed_text: String,
    pub deinflected_text: String,
    pub match_type: &'static str,
    pub match_source: MatchSource,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermHeadword {
    pub index: usize,
    pub headword_index: usize,
    pub term: String,
    pub reading: String,
    pub sources: Vec<TermSource>,
    pub tags: Vec<Tag>,
    pub word_classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermDefinition {
    pub index: usize,
    pub headword_indices: Vec<usize>,
    pub dictionary: String,
    pub dictionary_index: usize,
    pub dictionary_alias: String,
    pub id: i64,
    pub score: f64,
    pub frequency_order: f64,
    pub sequences: Vec<i64>,
    pub is_primary: bool,
    pub tags: Vec<Tag>,
    pub entries: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Pronunciation {
    #[serde(rename = "pitch-accent", rename_all = "camelCase")]
    PitchAccent { positions: Value, nasal_positions: Vec<Value>, devoice_positions: Vec<Value>, tags: Vec<Tag> },
    #[serde(rename = "phonetic-transcription")]
    PhoneticTranscription { ipa: Value, tags: Vec<Tag> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermPronunciation {
    pub index: usize,
    pub headword_index: usize,
    pub dictionary: String,
    pub dictionary_index: usize,
    pub dictionary_alias: String,
    pub pronunciations: Vec<Pronunciation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermFrequency {
    pub index: usize,
    pub headword_index: usize,
    pub dictionary: String,
    pub frequency_mode: Option<String>,
    pub dictionary_index: usize,
    pub dictionary_alias: String,
    pub has_reading: bool,
    pub frequency: f64,
    pub display_value: Option<String>,
    pub display_value_parsed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFacingInflectionRule {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFacingInflectionChain {
    pub source: InflectionSource,
    pub inflection_rules: Vec<UserFacingInflectionRule>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermDictionaryEntry {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub is_primary: bool,
    pub text_processor_rule_chain_candidates: Vec<Vec<String>>,
    pub inflection_rule_chain_candidates: Vec<UserFacingInflectionChain>,
    pub score: f64,
    pub frequency_order: f64,
    pub dictionary_index: usize,
    pub dictionary_alias: String,
    pub source_term_exact_match_count: usize,
    pub match_primary_reading: bool,
    pub max_original_text_length: usize,
    pub headwords: Vec<TermHeadword>,
    pub definitions: Vec<TermDefinition>,
    pub pronunciations: Vec<TermPronunciation>,
    pub frequencies: Vec<TermFrequency>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindTermsResult {
    pub dictionary_entries: Vec<TermDictionaryEntry>,
    /// 命中的原文长度（字符数）
    pub original_text_length: usize,
    /// 各阶段用时，给基准和诊断用，不发给界面
    #[serde(skip)]
    pub timings: LookupTimings,
}

/// 一次查词各阶段的用时和规模。
#[derive(Debug, Clone, Default)]
pub struct LookupTimings {
    /// 逐字缩短 + 文本变体 + 活用还原
    pub deinflect: Duration,
    /// 查条目（含「某词的变形」那一轮）
    pub find_terms: Duration,
    /// 去重、合并规则链，建词条
    pub build_entries: Duration,
    pub group: Duration,
    /// 词频、音调
    pub term_meta: Duration,
    pub tags: Duration,
    pub sort: Duration,
    /// 读释义（解压）并组装输出
    pub glossary: Duration,
    pub deinflections: usize,
    pub unique_terms: usize,
    pub rows: usize,
    pub definitions: usize,
}

impl LookupTimings {
    pub fn add(&mut self, other: &LookupTimings) {
        self.deinflect += other.deinflect;
        self.find_terms += other.find_terms;
        self.build_entries += other.build_entries;
        self.group += other.group;
        self.term_meta += other.term_meta;
        self.tags += other.tags;
        self.sort += other.sort;
        self.glossary += other.glossary;
        self.deinflections += other.deinflections;
        self.unique_terms += other.unique_terms;
        self.rows += other.rows;
        self.definitions += other.definitions;
    }
}

// ---------------------------------------------------------------- 内部结构

type Chains = Rc<RefCell<Vec<Vec<String>>>>;
type Inflections = Rc<RefCell<Vec<InflectionRuleChainCandidate>>>;
/// 一个词形下的各读音 → (词条下标, 词头下标)
type ReadingTargets = Vec<(String, Vec<(usize, usize)>)>;

/// JS 里标签数组是按对象身份登记到 TranslatorTagAggregator 的；这里给每个数组一个编号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TagList(usize);

#[derive(Clone)]
struct HeadwordB {
    index: usize,
    term: String,
    reading: String,
    sources: Vec<TermSource>,
    tags: TagList,
    word_classes: Vec<String>,
}

#[derive(Clone)]
struct DefinitionB {
    index: usize,
    headword_indices: Vec<usize>,
    dictionary: String,
    dictionary_index: usize,
    dictionary_alias: String,
    id: i64,
    score: f64,
    frequency_order: f64,
    sequences: Vec<i64>,
    is_primary: bool,
    tags: TagList,
    glossary: GlossaryRef,
}

#[derive(Clone)]
enum PronunciationB {
    Pitch { positions: Value, nasal: Vec<Value>, devoice: Vec<Value>, tags: TagList },
    Ipa { ipa: Value, tags: TagList },
}

struct TermPronunciationB {
    index: usize,
    headword_index: usize,
    dictionary: String,
    dictionary_index: usize,
    dictionary_alias: String,
    pronunciations: Vec<PronunciationB>,
}

struct EntryB {
    is_primary: bool,
    text_processor_chains: Chains,
    inflection_chains: Inflections,
    score: f64,
    frequency_order: f64,
    dictionary_index: usize,
    dictionary_alias: String,
    source_term_exact_match_count: usize,
    match_primary_reading: bool,
    max_original_text_length: usize,
    headwords: Vec<HeadwordB>,
    definitions: Vec<DefinitionB>,
    pronunciations: Vec<TermPronunciationB>,
    frequencies: Vec<TermFrequency>,
}

struct DbEntry {
    row: TermRow,
    dictionary: String,
}

struct DatabaseDeinflection {
    original_text: String,
    transformed_text: String,
    deinflected_text: String,
    conditions: u32,
    text_processor_chains: Chains,
    inflection_chains: Inflections,
    entries: Vec<Rc<DbEntry>>,
}

#[derive(Clone)]
struct TagGroup {
    dictionary: String,
    tag_names: Vec<String>,
}

/// JS `TranslatorTagAggregator`
#[derive(Default)]
struct TagAggregator {
    lists: Vec<Option<Vec<TagGroup>>>,
    /// 首次登记的先后（JS Map 的插入顺序）
    order: Vec<usize>,
}

impl TagAggregator {
    fn new_list(&mut self) -> TagList {
        self.lists.push(None);
        TagList(self.lists.len() - 1)
    }

    fn groups_mut(&mut self, list: TagList) -> &mut Vec<TagGroup> {
        let slot = &mut self.lists[list.0];
        if slot.is_none() {
            self.order.push(list.0);
        }
        slot.get_or_insert_with(Vec::new)
    }

    fn add_unique(groups: &mut Vec<TagGroup>, dictionary: &str, names: &[String]) {
        let i = match groups.iter().position(|g| g.dictionary == dictionary) {
            Some(i) => i,
            None => {
                groups.push(TagGroup { dictionary: dictionary.to_owned(), tag_names: Vec::new() });
                groups.len() - 1
            }
        };
        let group = &mut groups[i];
        for name in names {
            if !group.tag_names.contains(name) {
                group.tag_names.push(name.clone());
            }
        }
    }

    fn add_tags(&mut self, list: TagList, dictionary: &str, names: &[String]) {
        if names.is_empty() {
            return;
        }
        Self::add_unique(self.groups_mut(list), dictionary, names);
    }

    fn merge_tags(&mut self, list: TagList, new_list: TagList) {
        let Some(new_groups) = self.lists[new_list.0].clone() else { return };
        let groups = self.groups_mut(list);
        for group in &new_groups {
            Self::add_unique(groups, &group.dictionary, &group.tag_names);
        }
    }
}

// ---------------------------------------------------------------- 工具

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn cmp_f64(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

/// `Intl.Collator('en-US').compare` 的近似（见模块说明）。
pub(crate) fn collator_compare(a: &str, b: &str) -> Ordering {
    let primary = |s: &str| s.chars().map(|c| c.to_ascii_lowercase()).collect::<Vec<_>>();
    let upper = |s: &str| s.chars().map(|c| c.is_ascii_uppercase()).collect::<Vec<_>>();
    primary(a).cmp(&primary(b)).then_with(|| upper(a).cmp(&upper(b))).then_with(|| a.cmp(b))
}

/// JS `_getNameBase`
fn name_base(name: &str) -> &str {
    name.split(':').next().unwrap_or(name)
}

/// JS `_convertStringToNumber`：`/[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?/` 的第一个匹配。
fn convert_string_to_number(value: &str) -> f64 {
    let b = value.as_bytes();
    let digits = |mut i: usize| {
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        (i, i - start)
    };
    for start in 0..b.len() {
        let mut i = start;
        if matches!(b[i], b'+' | b'-') {
            i += 1;
        }
        let (after_int, int_len) = digits(i);
        let mut end = if int_len > 0 {
            if after_int < b.len() && b[after_int] == b'.' { digits(after_int + 1).0 } else { after_int }
        } else if i < b.len() && b[i] == b'.' {
            let (after_frac, frac_len) = digits(i + 1);
            if frac_len == 0 {
                continue;
            }
            after_frac
        } else {
            continue;
        };
        if end < b.len() && matches!(b[end], b'e' | b'E') {
            let mut j = end + 1;
            if j < b.len() && matches!(b[j], b'+' | b'-') {
                j += 1;
            }
            let (after_exp, exp_len) = digits(j);
            if exp_len > 0 {
                end = after_exp;
            }
        }
        return value[start..end].parse::<f64>().ok().filter(|n| n.is_finite()).unwrap_or(0.0);
    }
    0.0
}

/// JS `_getFrequencyInfo`
fn frequency_info(frequency: &Value) -> (f64, Option<String>, bool) {
    match frequency {
        Value::Object(map) => (
            map.get("value").and_then(Value::as_f64).unwrap_or(0.0),
            map.get("displayValue").and_then(Value::as_str).map(str::to_owned),
            false,
        ),
        Value::Number(n) => (n.as_f64().unwrap_or(0.0), None, false),
        Value::String(s) => (convert_string_to_number(s), Some(s.clone()), true),
        _ => (0.0, None, false),
    }
}

/// JS `_toNumberArray`
fn to_number_array(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(items)) => items.clone(),
        Some(v @ Value::Number(_)) => vec![v.clone()],
        _ => Vec::new(),
    }
}

fn value_to_string(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())
}

/// JS `_createTag`
fn create_tag(db: Option<&TagRow>, name: &str, dictionary: &str) -> Tag {
    let db = db.cloned().unwrap_or_default();
    Tag {
        name: name.to_owned(),
        category: db.category.filter(|c| !c.is_empty()).unwrap_or_else(|| "default".to_owned()),
        order: db.order.unwrap_or(0.0),
        score: db.score.unwrap_or(0.0),
        content: db.notes.filter(|n| !n.is_empty()).into_iter().collect(),
        dictionaries: vec![dictionary.to_owned()],
        redundant: false,
    }
}

/// JS `_areArraysEqualIgnoreOrder`（多重集合相等）
fn equal_ignore_order(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for x in a {
        *counts.entry(x).or_default() += 1;
    }
    for x in b {
        match counts.get_mut(x.as_str()) {
            Some(n) if *n > 0 => *n -= 1,
            _ => return false,
        }
    }
    true
}

fn shortest_text_processing_chain(chains: &[Vec<String>]) -> usize {
    chains.iter().map(Vec::len).min().unwrap_or(0)
}

fn shortest_inflection_chain(chains: &[InflectionRuleChainCandidate]) -> usize {
    chains.iter().map(|c| c.inflection_rules.len()).min().unwrap_or(0)
}

// ---------------------------------------------------------------- 查词

struct Ctx<'a> {
    store: &'a DictionaryStore,
    options: &'a FindTermsOptions,
    title_to_id: HashMap<String, i64>,
    id_to_title: HashMap<i64, String>,
    frequency_modes: HashMap<String, Option<String>>,
    enabled: HashMap<&'a str, &'a EnabledDictionary>,
    enabled_ids: HashSet<i64>,
}

impl<'a> Ctx<'a> {
    fn new(store: &'a DictionaryStore, options: &'a FindTermsOptions) -> Result<Self> {
        let infos = store.dictionaries()?;
        let title_to_id: HashMap<String, i64> = infos.iter().map(|d| (d.title.clone(), d.id)).collect();
        let enabled: HashMap<&str, &EnabledDictionary> =
            options.dictionaries.iter().map(|d| (d.title.as_str(), d)).collect();
        let enabled_ids = enabled.keys().filter_map(|t| title_to_id.get(*t).copied()).collect();
        Ok(Self {
            store,
            options,
            id_to_title: infos.iter().map(|d| (d.id, d.title.clone())).collect(),
            frequency_modes: infos.iter().map(|d| (d.title.clone(), d.frequency_mode.clone())).collect(),
            title_to_id,
            enabled,
            enabled_ids,
        })
    }

    fn dictionary_index(&self, title: &str) -> usize {
        self.enabled.get(title).map_or(self.enabled.len(), |d| d.index)
    }

    fn dictionary_alias(&self, title: &str) -> String {
        self.enabled
            .get(title)
            .map(|d| d.alias.as_str())
            .filter(|a| !a.is_empty())
            .unwrap_or(title)
            .to_owned()
    }

    fn title(&self, id: i64) -> String {
        self.id_to_title.get(&id).cloned().unwrap_or_default()
    }
}

pub struct Translator {
    transformer: LanguageTransformer,
}

impl Default for Translator {
    fn default() -> Self {
        Self::new()
    }
}

impl Translator {
    pub fn new() -> Self {
        Self { transformer: LanguageTransformer::japanese() }
    }

    /// 活用规则（找一首歌里同一个词的其他活用形时用）
    pub fn transformer(&self) -> &LanguageTransformer {
        &self.transformer
    }

    /// 对应 JS `findTerms(mode, text, options)`。
    pub fn find_terms(
        &self,
        store: &DictionaryStore,
        mode: FindTermsMode,
        text: &str,
        options: &FindTermsOptions,
    ) -> Result<FindTermsResult> {
        let mut timings = LookupTimings::default();
        let ctx = Ctx::new(store, options)?;
        let mut tags = TagAggregator::default();

        let text = if options.remove_non_japanese_characters { japanese_chinese_korean_only_prefix(text) } else { text };
        let (mut entries, original_text_length) = if text.is_empty() {
            (Vec::new(), 0)
        } else {
            let deinflections = self.deinflections(&ctx, text, &mut timings)?;
            let started = Instant::now();
            let built = self.dictionary_entries(&ctx, &deinflections, &mut tags);
            timings.build_entries = started.elapsed();
            built
        };

        let started = Instant::now();
        match mode {
            FindTermsMode::Group => entries = group_entries(entries, &mut tags, &options.primary_reading, true),
            FindTermsMode::Term => entries = group_entries(entries, &mut tags, &options.primary_reading, false),
            FindTermsMode::Simple | FindTermsMode::Split => {}
        }
        timings.group = started.elapsed();

        let mut expanded: HashMap<TagList, Vec<Tag>> = HashMap::new();
        if mode != FindTermsMode::Simple || options.use_all_frequency_dictionaries {
            let started = Instant::now();
            add_term_meta(&ctx, &mut entries, &mut tags, None)?;
            timings.term_meta = started.elapsed();
            let started = Instant::now();
            expanded = expand_tags(&ctx, &tags)?;
            timings.tags = started.elapsed();
        } else if let Some(dictionary) = &options.sort_frequency_dictionary {
            let started = Instant::now();
            add_term_meta(&ctx, &mut entries, &mut tags, Some(dictionary))?;
            timings.term_meta = started.elapsed();
        }

        let started = Instant::now();
        if let Some(dictionary) = &options.sort_frequency_dictionary {
            update_sort_frequencies(&mut entries, dictionary, options.sort_frequency_ascending);
        }
        if entries.len() > 1 {
            sort_entries(&mut entries);
        }
        for entry in &mut entries {
            flag_redundant_definition_tags(&entry.definitions, &mut expanded);
            if entry.definitions.len() > 1 {
                sort_definitions(&mut entry.definitions);
            }
            if entry.frequencies.len() > 1 {
                entry.frequencies.sort_by(|a, b| {
                    a.headword_index.cmp(&b.headword_index).then(a.dictionary_index.cmp(&b.dictionary_index)).then(a.index.cmp(&b.index))
                });
            }
            if entry.pronunciations.len() > 1 {
                entry.pronunciations.sort_by(|a, b| {
                    a.headword_index.cmp(&b.headword_index).then(a.dictionary_index.cmp(&b.dictionary_index)).then(a.index.cmp(&b.index))
                });
            }
        }

        timings.sort = started.elapsed();

        if let Some(max) = options.max_results {
            entries.truncate(max);
        }
        let started = Instant::now();
        let dictionary_entries = self.finish(&ctx, entries, &expanded, &mut timings)?;
        timings.glossary = started.elapsed();
        Ok(FindTermsResult { dictionary_entries, original_text_length, timings })
    }

    /// JS `_getDeinflections`
    fn deinflections(
        &self,
        ctx: &Ctx<'_>,
        text: &str,
        timings: &mut LookupTimings,
    ) -> Result<Vec<DatabaseDeinflection>> {
        let started = Instant::now();
        let mut deinflections: Vec<DatabaseDeinflection> = if ctx.options.deinflect {
            algorithm_deinflections(&self.transformer, text)
                .into_iter()
                .map(|d| DatabaseDeinflection {
                    original_text: d.original_text,
                    transformed_text: d.transformed_text,
                    deinflected_text: d.deinflected_text,
                    conditions: d.conditions,
                    text_processor_chains: Rc::new(RefCell::new(
                        d.text_processor_rule_chain_candidates
                            .into_iter()
                            .map(|chain| chain.into_iter().map(str::to_owned).collect())
                            .collect(),
                    )),
                    inflection_chains: Rc::new(RefCell::new(d.inflection_rule_chain_candidates)),
                    entries: Vec::new(),
                })
                .collect()
        } else {
            vec![DatabaseDeinflection {
                original_text: text.to_owned(),
                transformed_text: text.to_owned(),
                deinflected_text: text.to_owned(),
                conditions: 0,
                text_processor_chains: Rc::new(RefCell::new(Vec::new())),
                inflection_chains: Rc::new(RefCell::new(Vec::new())),
                entries: Vec::new(),
            }]
        };
        timings.deinflect = started.elapsed();
        timings.deinflections = deinflections.len();
        if deinflections.is_empty() {
            return Ok(deinflections);
        }

        let started = Instant::now();
        self.add_entries_to_deinflections(ctx, &mut deinflections, timings)?;
        let mut dictionary_deinflections = self.dictionary_deinflections(ctx, &deinflections)?;
        self.add_entries_to_deinflections(ctx, &mut dictionary_deinflections, timings)?;
        deinflections.extend(dictionary_deinflections);
        timings.find_terms = started.elapsed();

        // 只剩「某词的变形」、没有正文释义的条目不显示
        for d in &mut deinflections {
            d.entries.retain(|e| e.row.glossary_flags & GLOSSARY_HAS_CONTENT != 0);
        }
        deinflections.retain(|d| !d.entries.is_empty());
        Ok(deinflections)
    }

    /// JS `_addEntriesToDeinflections` + `_matchEntriesToDeinflections`
    fn add_entries_to_deinflections(
        &self,
        ctx: &Ctx<'_>,
        deinflections: &mut [DatabaseDeinflection],
        timings: &mut LookupTimings,
    ) -> Result<()> {
        let mut keys: Vec<String> = Vec::new();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut slots: HashMap<String, usize> = HashMap::new();
        for (i, d) in deinflections.iter().enumerate() {
            let slot = *slots.entry(d.deinflected_text.clone()).or_insert_with(|| {
                keys.push(d.deinflected_text.clone());
                groups.push(Vec::new());
                keys.len() - 1
            });
            groups[slot].push(i);
        }
        if keys.is_empty() {
            return Ok(());
        }
        timings.unique_terms += keys.len();

        for row in ctx.store.find_terms_bulk(&keys, &ctx.enabled_ids)? {
            timings.rows += 1;
            let dictionary = ctx.title(row.dictionary_id);
            let parts_of_speech_filter = ctx.enabled.get(dictionary.as_str()).is_some_and(|d| d.parts_of_speech_filter);
            let definition_conditions = self.transformer.condition_flags_from_parts_of_speech(&row.rules);
            let group = &groups[row.index];
            let entry = Rc::new(DbEntry { row, dictionary });
            for &i in group {
                if !parts_of_speech_filter || conditions_match(deinflections[i].conditions, definition_conditions) {
                    deinflections[i].entries.push(Rc::clone(&entry));
                }
            }
        }
        Ok(())
    }

    /// JS `_getDictionaryDeinflections`（不含其中最后一步查库，调用方接着做）
    fn dictionary_deinflections(
        &self,
        ctx: &Ctx<'_>,
        deinflections: &[DatabaseDeinflection],
    ) -> Result<Vec<DatabaseDeinflection>> {
        let mut out = Vec::new();
        for d in deinflections {
            for entry in &d.entries {
                let use_deinflections = ctx.enabled.get(entry.dictionary.as_str()).is_none_or(|e| e.use_deinflections);
                if !use_deinflections || entry.row.glossary_flags & GLOSSARY_HAS_FORM_OF == 0 {
                    continue;
                }
                for definition in ctx.store.glossary(entry.row.glossary)? {
                    let Value::Array(parts) = definition else { continue };
                    let form_of = match parts.first() {
                        Some(Value::String(s)) if !s.is_empty() => s.clone(),
                        Some(v @ Value::Number(_)) if v.as_f64() != Some(0.0) => v.to_string(),
                        Some(v @ (Value::Array(_) | Value::Object(_))) => v.to_string(),
                        Some(Value::Bool(true)) => "true".to_owned(),
                        _ => continue,
                    };
                    let dictionary_rules: Vec<String> =
                        parts.get(1).and_then(Value::as_array).map(|a| a.iter().map(value_to_string).collect()).unwrap_or_default();
                    let chains = d
                        .inflection_chains
                        .borrow()
                        .iter()
                        .map(|c| InflectionRuleChainCandidate {
                            source: if c.inflection_rules.is_empty() { InflectionSource::Dictionary } else { InflectionSource::Both },
                            inflection_rules: c.inflection_rules.iter().chain(&dictionary_rules).cloned().collect(),
                        })
                        .collect();
                    out.push(DatabaseDeinflection {
                        original_text: d.original_text.clone(),
                        transformed_text: d.transformed_text.clone(),
                        deinflected_text: form_of,
                        conditions: 0,
                        text_processor_chains: Rc::clone(&d.text_processor_chains),
                        inflection_chains: Rc::new(RefCell::new(chains)),
                        entries: Vec::new(),
                    });
                }
            }
        }
        Ok(out)
    }

    /// JS `_getDictionaryEntries`
    fn dictionary_entries(
        &self,
        ctx: &Ctx<'_>,
        deinflections: &[DatabaseDeinflection],
        tags: &mut TagAggregator,
    ) -> (Vec<EntryB>, usize) {
        let mut original_text_length = 0;
        let mut entries: Vec<EntryB> = Vec::new();
        let mut ids = HashSet::new();
        for d in deinflections {
            if d.entries.is_empty() {
                continue;
            }
            original_text_length = original_text_length.max(char_len(&d.original_text));
            for db in &d.entries {
                if !ids.contains(&db.row.id) {
                    entries.push(create_entry(ctx, tags, db, d));
                    ids.insert(db.row.id);
                    continue;
                }
                let Some(existing_index) =
                    entries.iter().position(|e| e.definitions.iter().any(|def| def.id == db.row.id))
                else {
                    continue;
                };
                let existing_transformed = entries[existing_index].headwords[0].sources[0].transformed_text.clone();
                let existing_len = char_len(&existing_transformed);
                let len = char_len(&d.transformed_text);
                match len.cmp(&existing_len) {
                    Ordering::Less => {}
                    Ordering::Greater => {
                        if d.original_text != existing_transformed {
                            entries[existing_index] = create_entry(ctx, tags, db, d);
                        }
                    }
                    Ordering::Equal => {
                        let existing = &entries[existing_index];
                        // 先拷出新规则链：两边可能是同一个共享数组
                        let new_inflections = d.inflection_chains.borrow().clone();
                        {
                            let mut chains = existing.inflection_chains.borrow_mut();
                            for candidate in new_inflections {
                                match chains.iter_mut().find(|c| equal_ignore_order(&c.inflection_rules, &candidate.inflection_rules)) {
                                    None => chains.push(candidate),
                                    Some(duplicate) => {
                                        if duplicate.source != candidate.source {
                                            duplicate.source = InflectionSource::Both;
                                        }
                                    }
                                }
                            }
                        }
                        let new_processors = d.text_processor_chains.borrow().clone();
                        let mut chains = existing.text_processor_chains.borrow_mut();
                        for rules in new_processors {
                            if !chains.iter().any(|c| equal_ignore_order(c, &rules)) {
                                chains.push(rules);
                            }
                        }
                    }
                }
            }
        }
        (entries, original_text_length)
    }

    /// 标签展开、释义读取、活用规则转成界面文字。
    fn finish(
        &self,
        ctx: &Ctx<'_>,
        entries: Vec<EntryB>,
        expanded: &HashMap<TagList, Vec<Tag>>,
        timings: &mut LookupTimings,
    ) -> Result<Vec<TermDictionaryEntry>> {
        let tags_of = |list: TagList| expanded.get(&list).cloned().unwrap_or_default();
        // 要显示的释义一次批量取（并行解压）；逐条串行解压曾占查词九成的时间
        let mut ids = Vec::new();
        let mut refs = Vec::new();
        let mut seen = HashSet::new();
        for d in entries.iter().flat_map(|e| &e.definitions) {
            if seen.insert(d.id) {
                ids.push(d.id);
                refs.push(d.glossary);
            }
        }
        timings.definitions = refs.len();
        let glossaries: HashMap<i64, Vec<Value>> = ids
            .into_iter()
            .zip(ctx.store.glossaries(&refs)?)
            .map(|(id, glossary)| (id, glossary.into_iter().filter(|g| !g.is_array()).collect()))
            .collect();
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            let mut definitions = Vec::with_capacity(e.definitions.len());
            for d in e.definitions {
                definitions.push(TermDefinition {
                    index: d.index,
                    headword_indices: d.headword_indices,
                    dictionary: d.dictionary,
                    dictionary_index: d.dictionary_index,
                    dictionary_alias: d.dictionary_alias,
                    id: d.id,
                    score: d.score,
                    frequency_order: d.frequency_order,
                    sequences: d.sequences,
                    is_primary: d.is_primary,
                    tags: tags_of(d.tags),
                    entries: glossaries[&d.id].clone(),
                });
            }
            let inflections = e.inflection_chains.borrow();
            out.push(TermDictionaryEntry {
                kind: "term",
                is_primary: e.is_primary,
                text_processor_rule_chain_candidates: e.text_processor_chains.borrow().clone(),
                inflection_rule_chain_candidates: inflections
                    .iter()
                    .map(|c| UserFacingInflectionChain {
                        source: c.source,
                        inflection_rules: c
                            .inflection_rules
                            .iter()
                            .map(|id| match self.transformer.transform_by_id(id) {
                                Some(t) => UserFacingInflectionRule {
                                    name: t.name.clone(),
                                    description: t.description.clone().filter(|d| !d.is_empty()),
                                },
                                None => UserFacingInflectionRule { name: id.clone(), description: None },
                            })
                            .collect(),
                    })
                    .collect(),
                score: e.score,
                frequency_order: e.frequency_order,
                dictionary_index: e.dictionary_index,
                dictionary_alias: e.dictionary_alias.clone(),
                source_term_exact_match_count: e.source_term_exact_match_count,
                match_primary_reading: e.match_primary_reading,
                max_original_text_length: e.max_original_text_length,
                headwords: e
                    .headwords
                    .iter()
                    .map(|h| TermHeadword {
                        index: h.index,
                        headword_index: h.index,
                        term: h.term.clone(),
                        reading: h.reading.clone(),
                        sources: h.sources.clone(),
                        tags: tags_of(h.tags),
                        word_classes: h.word_classes.clone(),
                    })
                    .collect(),
                definitions,
                pronunciations: e
                    .pronunciations
                    .iter()
                    .map(|p| TermPronunciation {
                        index: p.index,
                        headword_index: p.headword_index,
                        dictionary: p.dictionary.clone(),
                        dictionary_index: p.dictionary_index,
                        dictionary_alias: p.dictionary_alias.clone(),
                        pronunciations: p
                            .pronunciations
                            .iter()
                            .map(|x| match x {
                                PronunciationB::Pitch { positions, nasal, devoice, tags } => Pronunciation::PitchAccent {
                                    positions: positions.clone(),
                                    nasal_positions: nasal.clone(),
                                    devoice_positions: devoice.clone(),
                                    tags: tags_of(*tags),
                                },
                                PronunciationB::Ipa { ipa, tags } => {
                                    Pronunciation::PhoneticTranscription { ipa: ipa.clone(), tags: tags_of(*tags) }
                                }
                            })
                            .collect(),
                    })
                    .collect(),
                frequencies: e.frequencies.clone(),
            });
        }
        Ok(out)
    }
}

/// JS `_createTermDictionaryEntryFromDatabaseEntry`（isPrimary 恒为真）
fn create_entry(ctx: &Ctx<'_>, tags: &mut TagAggregator, db: &DbEntry, d: &DatabaseDeinflection) -> EntryB {
    let row = &db.row;
    let reading = if row.reading.is_empty() { row.term.clone() } else { row.reading.clone() };
    let primary = &ctx.options.primary_reading;
    let dictionary_index = ctx.dictionary_index(&db.dictionary);
    let dictionary_alias = ctx.dictionary_alias(&db.dictionary);
    let headword_tags = tags.new_list();
    let definition_tags = tags.new_list();
    tags.add_tags(headword_tags, &db.dictionary, &row.term_tags);
    tags.add_tags(definition_tags, &db.dictionary, &row.definition_tags);
    EntryB {
        is_primary: true,
        text_processor_chains: Rc::clone(&d.text_processor_chains),
        inflection_chains: Rc::clone(&d.inflection_chains),
        score: row.score,
        frequency_order: 0.0,
        dictionary_index,
        dictionary_alias: dictionary_alias.clone(),
        source_term_exact_match_count: usize::from(d.deinflected_text == row.term),
        match_primary_reading: !primary.is_empty() && reading == *primary,
        max_original_text_length: char_len(&d.original_text),
        headwords: vec![HeadwordB {
            index: 0,
            term: row.term.clone(),
            reading,
            sources: vec![TermSource {
                original_text: d.original_text.clone(),
                transformed_text: d.transformed_text.clone(),
                deinflected_text: d.deinflected_text.clone(),
                match_type: "exact",
                match_source: row.match_source,
                is_primary: true,
            }],
            tags: headword_tags,
            word_classes: row.rules.clone(),
        }],
        definitions: vec![DefinitionB {
            index: 0,
            headword_indices: vec![0],
            dictionary: db.dictionary.clone(),
            dictionary_index,
            dictionary_alias,
            id: row.id,
            score: row.score,
            frequency_order: 0.0,
            sequences: vec![if row.sequence >= 0 { row.sequence } else { -1 }],
            is_primary: true,
            tags: definition_tags,
            glossary: row.glossary,
        }],
        pronunciations: Vec::new(),
        frequencies: Vec::new(),
    }
}

/// JS `_groupDictionaryEntriesByHeadword`（`by_reading`）/ `_groupDictionaryEntriesByTerm`
fn group_entries(entries: Vec<EntryB>, tags: &mut TagAggregator, primary_reading: &str, by_reading: bool) -> Vec<EntryB> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<EntryB>> = HashMap::new();
    for entry in entries {
        let key = {
            let hw = &entry.headwords[0];
            let chains = entry.inflection_chains.borrow();
            if by_reading {
                serde_json::to_string(&(&hw.term, &hw.reading, &*chains))
            } else {
                serde_json::to_string(&(&hw.term, &*chains))
            }
            .unwrap_or_default()
        };
        match groups.get_mut(&key) {
            Some(group) => group.push(entry),
            None => {
                order.push(key.clone());
                groups.insert(key, vec![entry]);
            }
        }
    }
    order
        .into_iter()
        .filter_map(|key| groups.remove(&key))
        .map(|group| create_grouped_entry(group, tags, primary_reading))
        .collect()
}

/// JS `_addUniqueSources`
fn add_unique_sources(sources: &mut Vec<TermSource>, new_sources: &[TermSource]) {
    for new in new_sources {
        let existing = sources.iter_mut().find(|s| {
            s.deinflected_text == new.deinflected_text
                && s.transformed_text == new.transformed_text
                && s.original_text == new.original_text
                && s.match_type == new.match_type
                && s.match_source == new.match_source
        });
        match existing {
            Some(s) => {
                if new.is_primary {
                    s.is_primary = true;
                }
            }
            None => sources.push(new.clone()),
        }
    }
}

/// JS `_createGroupedDictionaryEntry(…, checkDuplicateDefinitions = false, …)`
fn create_grouped_entry(entries: Vec<EntryB>, tags: &mut TagAggregator, primary_reading: &str) -> EntryB {
    let mut headwords: Vec<HeadwordB> = Vec::new();
    let mut headword_keys: HashMap<(String, String), usize> = HashMap::new();
    let mut headword_dictionary_indices: HashMap<usize, usize> = HashMap::new();
    let mut definition_entries: Vec<(EntryB, Vec<usize>)> = Vec::new();

    for entry in entries {
        let mut index_map = Vec::with_capacity(entry.headwords.len());
        for hw in &entry.headwords {
            let key = (hw.term.clone(), hw.reading.clone());
            let idx = match headword_keys.get(&key) {
                Some(&i) => i,
                None => {
                    let i = headwords.len();
                    headword_keys.insert(key, i);
                    let list = tags.new_list();
                    headwords.push(HeadwordB {
                        index: i,
                        term: hw.term.clone(),
                        reading: hw.reading.clone(),
                        sources: Vec::new(),
                        tags: list,
                        word_classes: Vec::new(),
                    });
                    i
                }
            };
            let target = &mut headwords[idx];
            add_unique_sources(&mut target.sources, &hw.sources);
            for wc in &hw.word_classes {
                if !target.word_classes.contains(wc) {
                    target.word_classes.push(wc.clone());
                }
            }
            let target_tags = target.tags;
            tags.merge_tags(target_tags, hw.tags);
            index_map.push(idx);
        }
        for &hi in &index_map {
            if headword_dictionary_indices.get(&hi).is_none_or(|&x| entry.dictionary_index < x) {
                headword_dictionary_indices.insert(hi, entry.dictionary_index);
            }
        }
        definition_entries.push((entry, index_map));
    }

    let mut score = -MAX_SAFE_INTEGER;
    let mut dictionary_index = usize::MAX;
    let mut max_original_text_length = 0;
    let mut is_primary = false;
    let mut definitions: Vec<DefinitionB> = Vec::new();
    let mut inflections: Option<Inflections> = None;
    let mut text_processes: Option<Chains> = None;

    for (entry, index_map) in &definition_entries {
        score = score.max(entry.score);
        dictionary_index = dictionary_index.min(entry.dictionary_index);
        if entry.is_primary {
            is_primary = true;
            max_original_text_length = max_original_text_length.max(entry.max_original_text_length);
            if inflections.as_ref().is_none_or(|cur| entry.inflection_chains.borrow().len() < cur.borrow().len()) {
                inflections = Some(Rc::clone(&entry.inflection_chains));
            }
            if text_processes.as_ref().is_none_or(|cur| entry.text_processor_chains.borrow().len() < cur.borrow().len()) {
                text_processes = Some(Rc::clone(&entry.text_processor_chains));
            }
        }
        for def in &entry.definitions {
            definitions.push(DefinitionB {
                index: definitions.len(),
                headword_indices: def.headword_indices.iter().map(|&h| index_map[h]).collect(),
                frequency_order: 0.0,
                ..def.clone()
            });
        }
    }

    // _sortHeadwords
    headwords.sort_by(|a, b| {
        let a_primary = a.sources.iter().any(|s| s.is_primary);
        let b_primary = b.sources.iter().any(|s| s.is_primary);
        if a_primary != b_primary {
            return if a_primary { Ordering::Less } else { Ordering::Greater };
        }
        let a_index = headword_dictionary_indices.get(&a.index).copied().unwrap_or(usize::MAX);
        let b_index = headword_dictionary_indices.get(&b.index).copied().unwrap_or(usize::MAX);
        a_index.cmp(&b_index)
    });
    let mut remap = HashMap::new();
    for (i, hw) in headwords.iter_mut().enumerate() {
        remap.insert(hw.index, i);
        hw.index = i;
    }
    for def in &mut definitions {
        for h in &mut def.headword_indices {
            if let Some(&new) = remap.get(h) {
                *h = new;
            }
        }
    }

    // _getHeadwordMatchCounts
    let mut source_term_exact_match_count = 0;
    let mut match_primary_reading = false;
    for hw in &headwords {
        if !primary_reading.is_empty() && hw.reading == primary_reading {
            match_primary_reading = true;
        }
        if hw.sources.iter().any(|s| s.is_primary && s.match_source == MatchSource::Term) {
            source_term_exact_match_count += 1;
        }
    }

    EntryB {
        is_primary,
        text_processor_chains: text_processes.unwrap_or_default(),
        inflection_chains: inflections.unwrap_or_default(),
        score,
        frequency_order: 0.0,
        dictionary_index,
        dictionary_alias: String::new(),
        source_term_exact_match_count,
        match_primary_reading,
        max_original_text_length,
        headwords,
        definitions,
        pronunciations: Vec::new(),
        frequencies: Vec::new(),
    }
}

/// JS `_addTermMeta`。`only` 是 simple 模式下只为排序取词频时的那一本。
fn add_term_meta(ctx: &Ctx<'_>, entries: &mut [EntryB], tags: &mut TagAggregator, only: Option<&str>) -> Result<()> {
    let mut keys: Vec<String> = Vec::new();
    let mut reading_maps: Vec<ReadingTargets> = Vec::new();
    let mut slots: HashMap<String, usize> = HashMap::new();
    for (ei, entry) in entries.iter().enumerate() {
        for (hi, hw) in entry.headwords.iter().enumerate() {
            let slot = *slots.entry(hw.term.clone()).or_insert_with(|| {
                keys.push(hw.term.clone());
                reading_maps.push(Vec::new());
                keys.len() - 1
            });
            let map = &mut reading_maps[slot];
            match map.iter_mut().find(|(r, _)| *r == hw.reading) {
                Some((_, targets)) => targets.push((ei, hi)),
                None => map.push((hw.reading.clone(), vec![(ei, hi)])),
            }
        }
    }
    if keys.is_empty() {
        return Ok(());
    }
    let enabled_ids: HashSet<i64> = match only {
        Some(title) => ctx.title_to_id.get(title).copied().filter(|id| ctx.enabled_ids.contains(id)).into_iter().collect(),
        None => ctx.enabled_ids.clone(),
    };

    for meta in ctx.store.find_term_meta_bulk(&keys, &enabled_ids)? {
        let dictionary = ctx.title(meta.dictionary_id);
        let dictionary_index = ctx.dictionary_index(&dictionary);
        let dictionary_alias = ctx.dictionary_alias(&dictionary);
        for (reading, targets) in &reading_maps[meta.index] {
            match meta.mode.as_str() {
                "freq" => {
                    let has_reading = meta.data.get("reading").is_some_and(Value::is_string);
                    if has_reading && meta.data.get("reading").and_then(Value::as_str) != Some(reading.as_str()) {
                        continue;
                    }
                    let frequency =
                        if has_reading { meta.data.get("frequency").cloned().unwrap_or(Value::Null) } else { meta.data.clone() };
                    let frequency_mode = ctx.frequency_modes.get(&dictionary).cloned().flatten();
                    let (value, display_value, display_value_parsed) = frequency_info(&frequency);
                    for &(ei, hi) in targets {
                        let frequencies = &mut entries[ei].frequencies;
                        frequencies.push(TermFrequency {
                            index: frequencies.len(),
                            headword_index: hi,
                            dictionary: dictionary.clone(),
                            frequency_mode: frequency_mode.clone(),
                            dictionary_index,
                            dictionary_alias: dictionary_alias.clone(),
                            has_reading,
                            frequency: value,
                            display_value: display_value.clone(),
                            display_value_parsed,
                        });
                    }
                }
                "pitch" | "ipa" => {
                    if meta.data.get("reading").and_then(Value::as_str) != Some(reading.as_str()) {
                        continue;
                    }
                    let (list_key, is_pitch) =
                        if meta.mode == "pitch" { ("pitches", true) } else { ("transcriptions", false) };
                    let mut items = Vec::new();
                    for item in meta.data.get(list_key).and_then(Value::as_array).into_iter().flatten() {
                        let list = tags.new_list();
                        if let Some(names) = item.get("tags").and_then(Value::as_array) {
                            let names: Vec<String> = names.iter().map(value_to_string).collect();
                            tags.add_tags(list, &dictionary, &names);
                        }
                        items.push(if is_pitch {
                            PronunciationB::Pitch {
                                positions: item.get("position").cloned().unwrap_or(Value::Null),
                                nasal: to_number_array(item.get("nasal")),
                                devoice: to_number_array(item.get("devoice")),
                                tags: list,
                            }
                        } else {
                            PronunciationB::Ipa { ipa: item.get("ipa").cloned().unwrap_or(Value::Null), tags: list }
                        });
                    }
                    for &(ei, hi) in targets {
                        let pronunciations = &mut entries[ei].pronunciations;
                        pronunciations.push(TermPronunciationB {
                            index: pronunciations.len(),
                            headword_index: hi,
                            dictionary: dictionary.clone(),
                            dictionary_index,
                            dictionary_alias: dictionary_alias.clone(),
                            pronunciations: items.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// JS `_expandTagGroups` + `_groupTags`
fn expand_tags(ctx: &Ctx<'_>, tags: &TagAggregator) -> Result<HashMap<TagList, Vec<Tag>>> {
    struct Item {
        dictionary: String,
        tag_name: String,
        targets: Vec<TagList>,
    }
    let mut items: Vec<Item> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for &list in &tags.order {
        for group in tags.lists[list].iter().flatten() {
            for name in &group.tag_names {
                let i = *index.entry((group.dictionary.clone(), name.clone())).or_insert_with(|| {
                    items.push(Item { dictionary: group.dictionary.clone(), tag_name: name.clone(), targets: Vec::new() });
                    items.len() - 1
                });
                items[i].targets.push(TagList(list));
            }
        }
    }

    let mut expanded: HashMap<TagList, Vec<Tag>> = HashMap::new();
    for item in &items {
        let db = match ctx.title_to_id.get(&item.dictionary) {
            Some(&id) => ctx.store.find_tag(id, name_base(&item.tag_name))?,
            None => None,
        };
        let tag = create_tag(db.as_ref(), &item.tag_name, &item.dictionary);
        for target in &item.targets {
            expanded.entry(*target).or_default().push(tag.clone());
        }
    }

    for tags in expanded.values_mut() {
        if tags.len() <= 1 {
            continue;
        }
        // _mergeSimilarTags
        let mut i = 0;
        while i < tags.len() {
            let mut j = i + 1;
            while j < tags.len() {
                if tags[j].name == tags[i].name && tags[j].category == tags[i].category {
                    let other = tags.remove(j);
                    let first = &mut tags[i];
                    first.order = first.order.min(other.order);
                    first.score = first.score.max(other.score);
                    first.dictionaries.extend(other.dictionaries);
                    for c in other.content {
                        if !first.content.contains(&c) {
                            first.content.push(c);
                        }
                    }
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        tags.sort_by(|a, b| cmp_f64(a.order, b.order).then_with(|| collator_compare(&a.name, &b.name)));
    }
    Ok(expanded)
}

/// JS `_updateSortFrequencies`
fn update_sort_frequencies(entries: &mut [EntryB], dictionary: &str, ascending: bool) {
    let order_of = |min: f64, max: f64| {
        if min <= max {
            if ascending { min } else { -max }
        } else if ascending {
            MAX_SAFE_INTEGER
        } else {
            0.0
        }
    };
    for entry in entries {
        let mut by_headword: HashMap<usize, f64> = HashMap::new();
        let (mut min, mut max) = (MAX_SAFE_INTEGER, -MAX_SAFE_INTEGER);
        for f in entry.frequencies.iter().filter(|f| f.dictionary == dictionary) {
            by_headword.insert(f.headword_index, f.frequency);
            min = min.min(f.frequency);
            max = max.max(f.frequency);
        }
        entry.frequency_order = order_of(min, max);
        for definition in &mut entry.definitions {
            let (mut min, mut max) = (MAX_SAFE_INTEGER, -MAX_SAFE_INTEGER);
            for h in &definition.headword_indices {
                if let Some(&f) = by_headword.get(h) {
                    min = min.min(f);
                    max = max.max(f);
                }
            }
            definition.frequency_order = order_of(min, max);
        }
    }
}

/// JS `_sortTermDictionaryEntries`
fn sort_entries(entries: &mut [EntryB]) {
    entries.sort_by(|v1, v2| {
        v2.match_primary_reading
            .cmp(&v1.match_primary_reading)
            .then_with(|| v2.max_original_text_length.cmp(&v1.max_original_text_length))
            .then_with(|| {
                shortest_text_processing_chain(&v1.text_processor_chains.borrow())
                    .cmp(&shortest_text_processing_chain(&v2.text_processor_chains.borrow()))
            })
            .then_with(|| {
                shortest_inflection_chain(&v1.inflection_chains.borrow())
                    .cmp(&shortest_inflection_chain(&v2.inflection_chains.borrow()))
            })
            .then_with(|| v2.source_term_exact_match_count.cmp(&v1.source_term_exact_match_count))
            .then_with(|| cmp_f64(v1.frequency_order, v2.frequency_order))
            .then_with(|| v1.dictionary_index.cmp(&v2.dictionary_index))
            .then_with(|| cmp_f64(v2.score, v1.score))
            .then_with(|| {
                for (h1, h2) in v1.headwords.iter().zip(&v2.headwords) {
                    let c = char_len(&h2.term).cmp(&char_len(&h1.term)).then_with(|| collator_compare(&h1.term, &h2.term));
                    if c != Ordering::Equal {
                        return c;
                    }
                }
                Ordering::Equal
            })
            .then_with(|| v2.definitions.len().cmp(&v1.definitions.len()))
    });
}

/// JS `_sortTermDictionaryEntryDefinitions`
fn sort_definitions(definitions: &mut [DefinitionB]) {
    definitions.sort_by(|v1, v2| {
        cmp_f64(v1.frequency_order, v2.frequency_order)
            .then_with(|| v1.dictionary_index.cmp(&v2.dictionary_index))
            .then_with(|| cmp_f64(v2.score, v1.score))
            .then_with(|| v2.headword_indices.len().cmp(&v1.headword_indices.len()))
            .then_with(|| v1.headword_indices.cmp(&v2.headword_indices))
            .then_with(|| v1.index.cmp(&v2.index))
    });
}

/// JS `_flagRedundantDefinitionTags`：同一本词典里连续几条词性相同，后面的词性标签标为冗余。
fn flag_redundant_definition_tags(definitions: &[DefinitionB], expanded: &mut HashMap<TagList, Vec<Tag>>) {
    let mut last_dictionary: Option<&str> = None;
    let mut last_part_of_speech = String::new();
    for definition in definitions {
        let part_of_speech = {
            let mut names: Vec<&str> = expanded
                .get(&definition.tags)
                .into_iter()
                .flatten()
                .filter(|t| t.category == "partOfSpeech")
                .map(|t| t.name.as_str())
                .collect();
            names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            serde_json::to_string(&names).unwrap_or_default()
        };
        if last_dictionary != Some(definition.dictionary.as_str()) {
            last_dictionary = Some(definition.dictionary.as_str());
            last_part_of_speech.clear();
        }
        if last_part_of_speech == part_of_speech {
            if let Some(tags) = expanded.get_mut(&definition.tags) {
                for tag in tags.iter_mut().filter(|t| t.category == "partOfSpeech") {
                    tag.redundant = true;
                }
            }
        } else {
            last_part_of_speech = part_of_speech;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_pulled_out_of_display_strings_like_the_js_regex() {
        assert_eq!(convert_string_to_number("five (5)"), 5.0);
        // 「-」后面跟的不是数字，不算符号；Yomitan 期望结果里这一条也是 22
        assert_eq!(convert_string_to_number("twenty-two (22)"), 22.0);
        assert_eq!(convert_string_to_number("sixteen"), 0.0);
        assert_eq!(convert_string_to_number("㋕ 1234"), 1234.0);
        assert_eq!(convert_string_to_number("x.5e3y"), 500.0);
        assert_eq!(convert_string_to_number("3."), 3.0);
        assert_eq!(convert_string_to_number("+-7"), -7.0);
        assert_eq!(convert_string_to_number("1e"), 1.0);
    }

    #[test]
    fn collation_ignores_ascii_case_first_then_puts_lowercase_first() {
        assert_eq!(collator_compare("abbr", "E1"), Ordering::Less);
        assert_eq!(collator_compare("n", "N"), Ordering::Less);
        assert_eq!(collator_compare("P", "vt"), Ordering::Less);
        assert_eq!(collator_compare("E1", "E2"), Ordering::Less);
    }

    #[test]
    fn arrays_compare_as_multisets() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(equal_ignore_order(&s(&["a", "b", "a"]), &s(&["a", "a", "b"])));
        assert!(!equal_ignore_order(&s(&["a", "b"]), &s(&["a", "a"])));
    }
}
