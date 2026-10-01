//! 活用还原（deinflection）：Yomitan `LanguageTransformer` 的逐行移植。
//!
//! 移植自 Yomitan `ext/js/language/language-transformer.js`
//! （Copyright (C) 2024-2026 Yomitan Authors，GPL-3.0-or-later）。
//! 规则数据 `data/japanese-transforms.json` 由 `tools/dump-yomitan-transforms.mjs`
//! 从 Yomitan 源码直接导出（commit d34832d7），不是手抄的。
//!
//! 和 JS 版保持一致、改了就会和 Yomitan 对不上的几处：
//! - 条件是位标志。叶子条件按声明顺序分配位，带 `subConditions` 的是子条件的并集。
//! - `conditions_match(cur, next) = cur == 0 || cur & next != 0`。
//! - 广度优先展开，结果顺序就是 JS 数组的 push 顺序（下游排序会用到先后）。
//! - trace 新帧在前：`食べました` 的 trace 是 `[-ます, -た]`。
//! - 同一条规则对同一段文字再次命中视为循环，跳过（JS 版会打一条警告）。
//!
//! JS 版每个 transform 还有一个把所有规则正则拼起来的 heuristic，只是为了少跑几条规则，
//! 结果和逐条检查完全相同，这里不需要。

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// 位标志条件是否相容。`current` 为 0 表示「还没有约束」，什么都能接。
pub fn conditions_match(current: u32, next: u32) -> bool {
    current == 0 || (current & next) != 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleKind {
    /// 词尾替换：`食べました` → `食べます`
    Suffix,
    /// 整词替换：Yomitan 只用在 `いらっしゃいます` 这类特殊敬语上
    WholeWord,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub kind: RuleKind,
    pub inflected: String,
    pub deinflected: String,
    pub conditions_in: u32,
    pub conditions_out: u32,
}

impl Rule {
    fn is_inflected(&self, text: &str) -> bool {
        match self.kind {
            RuleKind::Suffix => text.ends_with(&self.inflected),
            RuleKind::WholeWord => text == self.inflected,
        }
    }

    /// 只在 `is_inflected` 为真时调用。JS 版按 UTF-16 长度切，
    /// 这里按字节切；因为已确认以词尾结尾，两者切出的是同一段。
    fn deinflect(&self, text: &str) -> String {
        match self.kind {
            RuleKind::Suffix => {
                let stem = &text[..text.len() - self.inflected.len()];
                let mut out = String::with_capacity(stem.len() + self.deinflected.len());
                out.push_str(stem);
                out.push_str(&self.deinflected);
                out
            }
            RuleKind::WholeWord => self.deinflected.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Transform {
    /// 规则 id，例如 `-ます`、`potential or passive`。测试和 trace 都用它。
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceFrame {
    /// `LanguageTransformer::transforms()` 里的下标
    pub transform: usize,
    pub rule_index: usize,
    /// 应用这条规则之前的文字
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TransformedText {
    pub text: String,
    pub conditions: u32,
    /// 新帧在前
    pub trace: Vec<TraceFrame>,
}

pub struct LanguageTransformer {
    language: String,
    transforms: Vec<Transform>,
    condition_type_flags: HashMap<String, u32>,
    part_of_speech_flags: HashMap<String, u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DescriptorJson {
    language: String,
    conditions: Vec<ConditionJson>,
    transforms: Vec<TransformJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConditionJson {
    id: String,
    is_dictionary_form: bool,
    sub_conditions: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct TransformJson {
    id: String,
    name: String,
    description: Option<String>,
    rules: Vec<RuleJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleJson {
    #[serde(rename = "type")]
    kind: RuleKind,
    inflected: String,
    deinflected: String,
    conditions_in: Vec<String>,
    conditions_out: Vec<String>,
}

impl LanguageTransformer {
    /// 内置的日语规则（Yomitan `japaneseTransforms`）。
    pub fn japanese() -> Self {
        Self::from_json(include_str!("../data/japanese-transforms.json"))
            .expect("内置的日语活用规则应当能解析")
    }

    /// 对应 JS 的 `addDescriptor`（本项目只加载一份描述，所以没有累加）。
    pub fn from_json(json: &str) -> Result<Self> {
        let descriptor: DescriptorJson =
            serde_json::from_str(json).context("活用规则 JSON 解析失败")?;
        let flags = condition_flags_map(&descriptor.conditions)?;

        let mut transforms = Vec::with_capacity(descriptor.transforms.len());
        for transform in descriptor.transforms {
            let mut rules = Vec::with_capacity(transform.rules.len());
            for (j, rule) in transform.rules.into_iter().enumerate() {
                let conditions_in = flags_strict(&flags, &rule.conditions_in).with_context(|| {
                    format!("transform {}.rules[{j}] 的 conditionsIn 无效", transform.id)
                })?;
                let conditions_out =
                    flags_strict(&flags, &rule.conditions_out).with_context(|| {
                        format!("transform {}.rules[{j}] 的 conditionsOut 无效", transform.id)
                    })?;
                if rule.kind == RuleKind::Suffix && rule.inflected.is_empty() {
                    // JS 的 slice(0, -0) 会把整段文字吃掉，这种规则两边行为对不上
                    bail!("transform {}.rules[{j}] 的词尾为空", transform.id);
                }
                rules.push(Rule {
                    kind: rule.kind,
                    inflected: rule.inflected,
                    deinflected: rule.deinflected,
                    conditions_in,
                    conditions_out,
                });
            }
            transforms.push(Transform {
                id: transform.id,
                name: transform.name,
                description: transform.description,
                rules,
            });
        }

        let mut part_of_speech_flags = HashMap::new();
        for condition in &descriptor.conditions {
            if condition.is_dictionary_form
                && let Some(&f) = flags.get(&condition.id)
            {
                part_of_speech_flags.insert(condition.id.clone(), f);
            }
        }

        Ok(Self {
            language: descriptor.language,
            transforms,
            condition_type_flags: flags,
            part_of_speech_flags,
        })
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn transforms(&self) -> &[Transform] {
        &self.transforms
    }

    /// 词典条目的 `rules`（`v5 vt` 这种）→ 条件位。只认 `isDictionaryForm` 的条件，
    /// 不认识的词性记 0。对应 JS `getConditionFlagsFromPartsOfSpeech`。
    pub fn condition_flags_from_parts_of_speech<S: AsRef<str>>(&self, parts_of_speech: &[S]) -> u32 {
        flags_lenient(&self.part_of_speech_flags, parts_of_speech)
    }

    pub fn condition_flags_from_condition_types<S: AsRef<str>>(&self, types: &[S]) -> u32 {
        flags_lenient(&self.condition_type_flags, types)
    }

    pub fn condition_flags_from_condition_type(&self, condition_type: &str) -> u32 {
        self.condition_flags_from_condition_types(&[condition_type])
    }

    /// 对应 JS `transform(sourceText)`。第一项永远是原文本身（条件 0、trace 空）。
    pub fn transform(&self, source: &str) -> Vec<TransformedText> {
        let mut results = vec![TransformedText {
            text: source.to_owned(),
            conditions: 0,
            trace: Vec::new(),
        }];
        let mut i = 0;
        while i < results.len() {
            let text = results[i].text.clone();
            let conditions = results[i].conditions;
            for (t, transform) in self.transforms.iter().enumerate() {
                for (j, rule) in transform.rules.iter().enumerate() {
                    if !conditions_match(conditions, rule.conditions_in) || !rule.is_inflected(&text)
                    {
                        continue;
                    }
                    let is_cycle = results[i]
                        .trace
                        .iter()
                        .any(|f| f.transform == t && f.rule_index == j && f.text == text);
                    if is_cycle {
                        continue;
                    }
                    let mut trace = Vec::with_capacity(results[i].trace.len() + 1);
                    trace.push(TraceFrame { transform: t, rule_index: j, text: text.clone() });
                    trace.extend(results[i].trace.iter().cloned());
                    results.push(TransformedText {
                        text: rule.deinflect(&text),
                        conditions: rule.conditions_out,
                        trace,
                    });
                }
            }
            i += 1;
        }
        results
    }

    /// trace → 规则 id 链（新帧在前），给测试和界面用。
    pub fn trace_ids<'a>(&'a self, trace: &[TraceFrame]) -> Vec<&'a str> {
        trace.iter().map(|f| self.transforms[f.transform].id.as_str()).collect()
    }

    pub fn transform_by_id(&self, id: &str) -> Option<&Transform> {
        self.transforms.iter().find(|t| t.id == id)
    }
}

/// JS `_getConditionFlagsMap`：多轮解析，子条件还没分配到位的留到下一轮。
fn condition_flags_map(conditions: &[ConditionJson]) -> Result<HashMap<String, u32>> {
    let mut map = HashMap::new();
    let mut next_flag_index = 0u32;
    let mut targets: Vec<&ConditionJson> = conditions.iter().collect();
    while !targets.is_empty() {
        let mut next_targets = Vec::new();
        for &condition in &targets {
            let flags = match &condition.sub_conditions {
                None => {
                    if next_flag_index >= 32 {
                        bail!("条件数超过 32 个位的上限");
                    }
                    let f = 1u32 << next_flag_index;
                    next_flag_index += 1;
                    f
                }
                Some(subs) => match flags_strict(&map, subs) {
                    Some(f) => f,
                    None => {
                        next_targets.push(condition);
                        continue;
                    }
                },
            };
            map.insert(condition.id.clone(), flags);
        }
        if next_targets.len() == targets.len() {
            bail!("subConditions 存在循环引用");
        }
        targets = next_targets;
    }
    Ok(map)
}

fn flags_strict(map: &HashMap<String, u32>, types: &[String]) -> Option<u32> {
    let mut flags = 0;
    for t in types {
        flags |= *map.get(t)?;
    }
    Some(flags)
}

fn flags_lenient<S: AsRef<str>>(map: &HashMap<String, u32>, types: &[S]) -> u32 {
    types.iter().fold(0, |f, t| f | map.get(t.as_ref()).copied().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_itself_is_always_the_first_candidate() {
        let lt = LanguageTransformer::japanese();
        let out = lt.transform("食べました");
        assert_eq!(out[0].text, "食べました");
        assert_eq!(out[0].conditions, 0);
        assert!(out[0].trace.is_empty());
    }

    #[test]
    fn traces_list_the_newest_rule_first() {
        let lt = LanguageTransformer::japanese();
        let hit = lt
            .transform("食べました")
            .into_iter()
            .find(|r| r.text == "食べる" && r.trace.len() == 2)
            .expect("应当还原到 食べる");
        assert_eq!(lt.trace_ids(&hit.trace), ["-ます", "-た"]);
        assert_eq!(hit.trace[0].text, "食べます");
        assert_eq!(hit.trace[1].text, "食べました");
    }

    #[test]
    fn unknown_parts_of_speech_contribute_no_flags() {
        let lt = LanguageTransformer::japanese();
        assert_eq!(lt.condition_flags_from_parts_of_speech(&["n", "vt"]), 0);
        assert_ne!(lt.condition_flags_from_parts_of_speech(&["v5"]), 0);
    }

    #[test]
    fn zero_conditions_accept_anything() {
        assert!(conditions_match(0, 0));
        assert!(conditions_match(0, 0b100));
        assert!(!conditions_match(0b010, 0b100));
        assert!(conditions_match(0b110, 0b100));
    }

    #[test]
    fn a_sub_condition_cycle_is_rejected_instead_of_looping() {
        let json = r#"{"language":"x","conditions":[
            {"id":"a","isDictionaryForm":true,"subConditions":["b"]},
            {"id":"b","isDictionaryForm":true,"subConditions":["a"]}],"transforms":[]}"#;
        assert!(LanguageTransformer::from_json(json).is_err());
    }

    #[test]
    fn a_sub_condition_declared_before_its_leaves_still_resolves() {
        let json = r#"{"language":"x","conditions":[
            {"id":"v","isDictionaryForm":false,"subConditions":["v1","v5"]},
            {"id":"v1","isDictionaryForm":true,"subConditions":null},
            {"id":"v5","isDictionaryForm":true,"subConditions":null}],"transforms":[]}"#;
        let lt = LanguageTransformer::from_json(json).unwrap();
        assert_eq!(lt.condition_flags_from_condition_type("v1"), 0b01);
        assert_eq!(lt.condition_flags_from_condition_type("v5"), 0b10);
        assert_eq!(lt.condition_flags_from_condition_type("v"), 0b11);
        // 非辞书形的条件不参与词性映射
        assert_eq!(lt.condition_flags_from_parts_of_speech(&["v"]), 0);
    }
}
