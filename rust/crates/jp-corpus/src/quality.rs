//! 歌词校验：一份歌词靠不靠得住。
//!
//! 歌词会错：漏行、重复贴了一遍副歌、时间轴漂移、乱码、混进署名或逐字时间标记、
//! 甚至整份是另一首歌的翻译。这些错会直接污染词频、共现和播放器同步，
//! 而且一旦进了库，统计结果里是看不出来的。
//!
//! 这里**只检查、不改**：原文一个字不动，结论写进 `lyric_versions.validation_json`，
//! 每行一个时间戳置信度写进 `utterances.time_confidence`。要不要排除一首歌，
//! 由分析时的筛选条件（`StatsFilter::min_completeness`）决定，不在这里替人做主。
//!
//! 全是纯函数，不碰数据库，所以能拿构造出来的歌词逐条测。

use serde::{Deserialize, Serialize};

/// 一行待检查的歌词。
#[derive(Debug, Clone, Copy)]
pub struct LineInput<'a> {
    pub time_sec: Option<f64>,
    pub text: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// 值得知道，但多半不是错（间奏很长、副歌连唱两遍）
    Info,
    /// 可能是错，统计和同步会受影响
    Warning,
    /// 几乎一定是错（乱码、一行都没有）
    Error,
}

/// 校验发现的一个问题。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// 稳定的机器代码，界面按它翻译、分析按它统计。见 [`ISSUE_CODES`]
    pub code: String,
    pub severity: Severity,
    /// 第几行（从 0 起，和 `utterances.line_idx` 一致）；整份歌词的问题是 `None`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_idx: Option<usize>,
    pub message: String,
}

/// 所有问题代码和一句中文说明。界面和文档都从这里取，免得两处说法不一。
pub const ISSUE_CODES: &[(&str, &str)] = &[
    ("empty", "一行歌词都没有"),
    ("replacement_char", "有乱码（U+FFFD），多半是编码认错了"),
    ("not_japanese", "看起来不是日文歌词（可能是翻译或别的歌）"),
    ("too_few_lines", "行数太少，可能只有一部分"),
    ("ends_early", "歌词在歌曲一半之前就结束了，可能被截断"),
    ("untimed", "没有时间轴，播放器无法同步"),
    ("mixed_timing", "有的行有时间、有的行没有"),
    ("non_monotonic", "时间倒退：这一行比上一行早"),
    ("beyond_duration", "时间超过了歌曲长度（时间轴漂移或不是这一版录音）"),
    ("duplicate_timestamp", "和上一行同一个时间"),
    ("stacked_duplicate", "和上一行同时间同文本：重复贴了一遍"),
    ("long_gap", "和上一行隔得很久：间奏，或者漏了几行"),
    ("leftover_markup", "混进了时间标记或署名"),
    ("very_long_line", "一行特别长，可能是几行粘在一起"),
];

/// 歌词的主要语言。按字符种类粗分，给「按语言建子语料」用，不是严格的语种识别。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageGuess {
    /// ja / en / ko / zh / mixed / unknown
    pub code: String,
    /// 0~1：主导那一类字符占全部文字字符的比例
    pub confidence: f64,
    /// 各类字符占比，留给需要自己定阈值的人
    pub kana_share: f64,
    pub han_share: f64,
    pub latin_share: f64,
    pub hangul_share: f64,
}

/// 一份歌词的校验结果。`line_confidence` 不进 JSON（它逐行写在 `utterances` 上）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Validation {
    pub line_count: usize,
    pub timed_line_count: usize,
    /// 0~1。从 1 开始按问题扣分，规则见 [`validate`]。是估计，不是判决
    pub completeness: f64,
    /// 文本在这首歌里出现不止一次的行占比（副歌）。统计时可以选「同一首歌里相同的行只算一次」
    pub repeated_line_ratio: f64,
    pub language: LanguageGuess,
    pub issues: Vec<Issue>,
    #[serde(skip)]
    pub line_confidence: Vec<Option<f64>>,
}

impl Validation {
    /// 某个级别以上的问题有几个
    pub fn count_at_least(&self, severity: Severity) -> usize {
        self.issues.iter().filter(|i| i.severity >= severity).count()
    }
}

/// 歌曲长度之外容许的余量（秒）。LRC 的最后一行常常标在尾奏淡出的那一刻。
const DURATION_SLACK_SEC: f64 = 2.0;
/// 相邻两行隔这么久就提一句。J-pop 的间奏很少超过半分钟。
const LONG_GAP_SEC: f64 = 30.0;
const VERY_LONG_LINE_CHARS: usize = 80;

/// 检查一份歌词。`duration_sec` 是歌曲长度，不知道就传 `None`（相关检查跳过）。
///
/// 完整度扣分规则（下限 0）：
///
/// | 问题 | 扣分 |
/// |---|---|
/// | 一行都没有 | 归零 |
/// | 乱码 | 0.5 |
/// | 行数太少 | 0.4 |
/// | 提前结束 | 0.3 |
/// | 不是日文 | 0.2 |
/// | 每处重复贴行 / 长空档 / 时间倒退 | 各 0.05，每类最多 0.2 |
///
/// 每行时间戳置信度：没时间是 `None`；有时间从 1.0 起，倒退 0.2、超出歌曲长度 0.1、
/// 和上一行同时间 0.5、在长空档之后 0.7、文件里有的行没时间再 × 0.8。取最低的那一条。
pub fn validate(lines: &[LineInput<'_>], duration_sec: Option<f64>) -> Validation {
    let mut issues = Vec::new();
    let line_count = lines.len();
    let timed_line_count = lines.iter().filter(|l| l.time_sec.is_some()).count();
    let language = guess_language(lines.iter().map(|l| l.text));
    let duration = duration_sec.filter(|d| *d > 0.0);

    if line_count == 0 {
        issues.push(whole("empty", Severity::Error, "一行歌词都没有".into()));
        return Validation {
            line_count,
            timed_line_count,
            completeness: 0.0,
            repeated_line_ratio: 0.0,
            language,
            issues,
            line_confidence: Vec::new(),
        };
    }

    let mut penalty = 0.0f64;
    let mut stacked = 0usize;
    let mut gaps = 0usize;
    let mut backwards = 0usize;
    let mut confidence: Vec<Option<f64>> = lines.iter().map(|l| l.time_sec.map(|_| 1.0)).collect();
    fn lower(confidence: &mut [Option<f64>], idx: usize, value: f64) {
        if let Some(c) = confidence[idx].as_mut() {
            *c = c.min(value);
        }
    }

    // ── 整份的问题 ──
    let garbled = lines.iter().filter(|l| l.text.contains('\u{FFFD}')).count();
    if garbled > 0 {
        issues.push(whole("replacement_char", Severity::Error, format!("{garbled} 行有乱码字符")));
        penalty += 0.5;
    }
    if language.code != "ja" && language.code != "unknown" {
        issues.push(whole(
            "not_japanese",
            Severity::Warning,
            format!("主要语言判断为 {}（{:.0}%）", language.code, language.confidence * 100.0),
        ));
        penalty += 0.2;
    }
    if let Some(d) = duration
        && d >= 120.0
        && line_count < 8
    {
        issues.push(whole("too_few_lines", Severity::Warning, format!("{d:.0} 秒的歌只有 {line_count} 行")));
        penalty += 0.4;
    }
    if timed_line_count == 0 {
        issues.push(whole("untimed", Severity::Info, "整份没有时间轴".into()));
    } else if timed_line_count < line_count {
        issues.push(whole(
            "mixed_timing",
            Severity::Warning,
            format!("{} 行里有 {} 行没时间", line_count, line_count - timed_line_count),
        ));
        for c in confidence.iter_mut().flatten() {
            *c *= 0.8;
        }
    }
    if let Some(d) = duration
        && d >= 60.0
        && let Some(last) = lines.iter().filter_map(|l| l.time_sec).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))))
        && last < d * 0.5
    {
        issues.push(whole(
            "ends_early",
            Severity::Warning,
            format!("最后一行在 {last:.0} 秒，歌长 {d:.0} 秒"),
        ));
        penalty += 0.3;
    }

    // ── 逐行的问题 ──
    let mut prev: Option<(usize, f64)> = None;
    for (idx, line) in lines.iter().enumerate() {
        let text = line.text.trim();
        if looks_like_markup(text) {
            issues.push(at(idx, "leftover_markup", Severity::Warning, format!("「{}」", clip(text))));
        }
        if text.chars().count() > VERY_LONG_LINE_CHARS {
            issues.push(at(idx, "very_long_line", Severity::Info, format!("{} 个字", text.chars().count())));
        }
        let Some(t) = line.time_sec else { continue };
        if let Some(d) = duration
            && t > d + DURATION_SLACK_SEC
        {
            issues.push(at(idx, "beyond_duration", Severity::Warning, format!("{t:.1} 秒 > 歌长 {d:.1} 秒")));
            lower(&mut confidence, idx, 0.1);
        }
        if let Some((p_idx, p)) = prev {
            if t + 0.01 < p {
                issues.push(at(idx, "non_monotonic", Severity::Warning, format!("{t:.2} 秒早于上一行的 {p:.2} 秒")));
                lower(&mut confidence, idx, 0.2);
                backwards += 1;
            } else if (t - p).abs() <= 0.01 {
                if lines[p_idx].text.trim() == text {
                    issues.push(at(idx, "stacked_duplicate", Severity::Warning, format!("「{}」", clip(text))));
                    stacked += 1;
                } else {
                    issues.push(at(idx, "duplicate_timestamp", Severity::Info, format!("{t:.2} 秒")));
                }
                lower(&mut confidence, idx, 0.5);
            } else if t - p > LONG_GAP_SEC {
                issues.push(at(idx, "long_gap", Severity::Info, format!("隔了 {:.0} 秒", t - p)));
                lower(&mut confidence, idx, 0.7);
                gaps += 1;
            }
        }
        // 倒退的那一行不当新的基准：后面的行跟它比会连着报一串假的倒退
        if prev.is_none_or(|(_, p)| t >= p) {
            prev = Some((idx, t));
        }
    }
    penalty += (stacked as f64 * 0.05).min(0.2);
    penalty += (gaps as f64 * 0.05).min(0.2);
    penalty += (backwards as f64 * 0.05).min(0.2);

    // 副歌：同一首歌里文本出现不止一次的行
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for line in lines {
        *seen.entry(line.text.trim()).or_default() += 1;
    }
    let repeated = lines.iter().filter(|l| seen[l.text.trim()] > 1).count();

    Validation {
        line_count,
        timed_line_count,
        completeness: round3((1.0 - penalty).clamp(0.0, 1.0)),
        repeated_line_ratio: round3(repeated as f64 / line_count as f64),
        language,
        issues,
        line_confidence: confidence.into_iter().map(|c| c.map(round3)).collect(),
    }
}

/// 按字符种类猜主要语言。
///
/// 只要假名占到文字字符的 5% 以上就当日文：日文歌词夹大量英文很平常，
/// 但不含假名的「日文歌词」几乎不存在（纯汉字的多半是中文翻译）。
pub fn guess_language<'a>(texts: impl IntoIterator<Item = &'a str>) -> LanguageGuess {
    let (mut kana, mut han, mut latin, mut hangul) = (0usize, 0usize, 0usize, 0usize);
    for text in texts {
        for c in text.chars() {
            match c {
                'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー' | 'ｦ'..='ﾟ' => kana += 1,
                '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々' => han += 1,
                'a'..='z' | 'A'..='Z' | 'ａ'..='ｚ' | 'Ａ'..='Ｚ' => latin += 1,
                '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' => hangul += 1,
                _ => {}
            }
        }
    }
    let letters = (kana + han + latin + hangul) as f64;
    let share = |n: usize| if letters > 0.0 { round3(n as f64 / letters) } else { 0.0 };
    let (kana_share, han_share, latin_share, hangul_share) = (share(kana), share(han), share(latin), share(hangul));
    let (code, confidence) = if letters == 0.0 {
        ("unknown", 0.0)
    } else if hangul_share >= 0.5 {
        ("ko", hangul_share)
    } else if kana_share >= 0.05 {
        ("ja", round3(kana_share + han_share))
    } else if latin_share >= 0.8 {
        ("en", latin_share)
    } else if kana == 0 && han_share >= 0.5 {
        ("zh", han_share)
    } else {
        ("mixed", round3(kana_share.max(han_share).max(latin_share).max(hangul_share)))
    };
    LanguageGuess {
        code: code.into(),
        confidence,
        kana_share,
        han_share,
        latin_share,
        hangul_share,
    }
}

/// 解析之后还留在正文里的标记：`[00:12.34]`、逐字时间 `<00:12.34>`、署名行。
fn looks_like_markup(text: &str) -> bool {
    let has_stamp = |open: char, close: char| {
        text.split(open).skip(1).any(|rest| {
            rest.split(close)
                .next()
                .is_some_and(|inner| inner.contains(':') && inner.chars().all(|c| c.is_ascii_digit() || c == ':' || c == '.'))
        })
    };
    if has_stamp('[', ']') || has_stamp('<', '>') {
        return true;
    }
    // 「作詞：xxx」这种署名本该被 LRC 解析器收走；还在正文里说明格式不认识
    const LABELS: [&str; 6] = ["作詞", "作词", "作曲", "編曲", "编曲", "歌词"];
    LABELS.iter().any(|label| {
        text.strip_prefix(label)
            .is_some_and(|rest| rest.trim_start().starts_with([':', '：']))
    })
}

fn whole(code: &str, severity: Severity, message: String) -> Issue {
    Issue { code: code.into(), severity, line_idx: None, message }
}

fn at(idx: usize, code: &str, severity: Severity, message: String) -> Issue {
    Issue { code: code.into(), severity, line_idx: Some(idx), message }
}

fn clip(text: &str) -> String {
    let mut out: String = text.chars().take(24).collect();
    if text.chars().count() > 24 {
        out.push('…');
    }
    out
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(rows: &[(Option<f64>, &'static str)]) -> Vec<LineInput<'static>> {
        rows.iter().map(|(t, text)| LineInput { time_sec: *t, text }).collect()
    }

    fn codes(v: &Validation) -> Vec<&str> {
        v.issues.iter().map(|i| i.code.as_str()).collect()
    }

    /// 二十行、等间隔、覆盖整首歌的正常歌词
    fn clean_song() -> Vec<LineInput<'static>> {
        const TEXT: [&str; 4] = ["夜に駆ける", "沈むように溶けてゆくように", "二人だけの空が広がる夜に", "さよならだけだった"];
        (0..20).map(|i| LineInput { time_sec: Some(10.0 + i as f64 * 10.0), text: TEXT[i % 4] }).collect()
    }

    #[test]
    fn a_clean_song_is_complete_and_every_line_is_trusted() {
        let v = validate(&clean_song(), Some(215.0));
        assert_eq!(v.completeness, 1.0, "{:?}", v.issues);
        assert!(v.issues.is_empty(), "{:?}", v.issues);
        assert_eq!(v.language.code, "ja");
        assert!(v.line_confidence.iter().all(|c| *c == Some(1.0)));
        assert_eq!(v.repeated_line_ratio, 1.0, "四句轮流唱，每句都出现了不止一次");
    }

    #[test]
    fn nothing_at_all_is_an_error() {
        let v = validate(&[], Some(200.0));
        assert_eq!(codes(&v), ["empty"]);
        assert_eq!(v.completeness, 0.0);
    }

    #[test]
    fn a_pasted_twice_line_is_caught_but_a_repeated_chorus_is_not() {
        let v = validate(
            &lines(&[
                (Some(10.0), "君の声が"),
                (Some(10.0), "君の声が"), // 同时间同文本：贴重了
                (Some(20.0), "聞こえる"),
                (Some(30.0), "君の声が"), // 副歌再唱一遍：正常
            ]),
            None,
        );
        assert_eq!(codes(&v), ["stacked_duplicate"]);
        assert_eq!(v.issues[0].line_idx, Some(1));
        assert_eq!(v.line_confidence[1], Some(0.5));
        assert_eq!(v.line_confidence[3], Some(1.0));
    }

    #[test]
    fn time_going_backwards_only_flags_the_line_that_jumped() {
        let v = validate(
            &lines(&[(Some(10.0), "あ"), (Some(20.0), "い"), (Some(5.0), "う"), (Some(25.0), "え"), (Some(30.0), "お")]),
            None,
        );
        assert_eq!(codes(&v), ["non_monotonic"], "后面的行不该跟着倒退的那行连报");
        assert_eq!(v.line_confidence[2], Some(0.2));
        assert_eq!(v.line_confidence[3], Some(1.0));
    }

    #[test]
    fn timestamps_past_the_end_of_the_song_mean_drift() {
        let mut song = clean_song();
        song.push(LineInput { time_sec: Some(260.0), text: "おわり" });
        let v = validate(&song, Some(215.0));
        assert!(codes(&v).contains(&"beyond_duration"));
        assert_eq!(*v.line_confidence.last().unwrap(), Some(0.1));
    }

    #[test]
    fn lyrics_that_stop_halfway_are_probably_truncated() {
        let song: Vec<_> = clean_song().into_iter().take(9).collect(); // 最后一行 90 秒
        let v = validate(&song, Some(240.0));
        assert!(codes(&v).contains(&"ends_early"), "{:?}", codes(&v));
        assert!(v.completeness <= 0.7);
    }

    #[test]
    fn a_handful_of_lines_for_a_full_song_is_incomplete() {
        let v = validate(&lines(&[(Some(10.0), "夜に駆ける"), (Some(150.0), "さよなら")]), Some(200.0));
        assert!(codes(&v).contains(&"too_few_lines"));
    }

    #[test]
    fn mojibake_is_an_error() {
        let v = validate(&lines(&[(Some(1.0), "\u{FFFD}\u{FFFD}の歌")]), None);
        assert!(codes(&v).contains(&"replacement_char"));
        assert_eq!(v.count_at_least(Severity::Error), 1);
    }

    #[test]
    fn untimed_lyrics_have_no_time_confidence() {
        let v = validate(&lines(&[(None, "あいうえお"), (None, "かきくけこ")]), None);
        assert_eq!(codes(&v), ["untimed"]);
        assert_eq!(v.line_confidence, vec![None, None]);
        assert_eq!(v.completeness, 1.0, "没时间轴不影响语料完整度，只影响同步");
    }

    #[test]
    fn partly_timed_lyrics_trust_their_timestamps_less() {
        let v = validate(&lines(&[(Some(1.0), "あ"), (None, "い")]), None);
        assert_eq!(codes(&v), ["mixed_timing"]);
        assert_eq!(v.line_confidence, vec![Some(0.8), None]);
    }

    #[test]
    fn a_long_silence_is_noted_and_lowers_the_next_lines_confidence() {
        let v = validate(&lines(&[(Some(10.0), "あ"), (Some(80.0), "い")]), None);
        assert_eq!(codes(&v), ["long_gap"]);
        assert_eq!(v.line_confidence[1], Some(0.7));
    }

    #[test]
    fn leftover_timestamps_and_credits_are_flagged() {
        let v = validate(
            &lines(&[(Some(1.0), "<00:01.00>夜<00:01.50>に"), (Some(2.0), "作詞：Ayase"), (Some(3.0), "[01:02]")]),
            None,
        );
        assert_eq!(codes(&v), ["leftover_markup", "leftover_markup", "leftover_markup"]);
        // 正文里出现「作詞」二字不算
        let v = validate(&lines(&[(Some(1.0), "作詞家になりたい")]), None);
        assert!(v.issues.is_empty());
    }

    #[test]
    fn languages_are_told_apart_by_script() {
        assert_eq!(guess_language(["夜に駆ける", "Oh baby I love you so"]).code, "ja");
        assert_eq!(guess_language(["I just wanna be with you tonight"]).code, "en");
        assert_eq!(guess_language(["我爱你在夜里奔跑"]).code, "zh");
        assert_eq!(guess_language(["사랑해요 오늘 밤"]).code, "ko");
        assert_eq!(guess_language(["♪", "123"]).code, "unknown");
    }

    #[test]
    fn a_chinese_translation_is_not_mistaken_for_japanese_lyrics() {
        let v = validate(&lines(&[(Some(1.0), "像沉没一样溶化下去"), (Some(2.0), "两个人的天空在夜里展开")]), None);
        assert_eq!(v.language.code, "zh");
        assert!(codes(&v).contains(&"not_japanese"));
    }

    #[test]
    fn every_emitted_code_is_documented() {
        let all = [
            validate(&[], None),
            validate(&lines(&[(Some(1.0), "\u{FFFD}"), (Some(1.0), "\u{FFFD}"), (None, "作詞：a"), (Some(0.5), "x")]), Some(300.0)),
        ];
        for v in &all {
            for issue in &v.issues {
                assert!(ISSUE_CODES.iter().any(|(c, _)| *c == issue.code), "{} 没写进 ISSUE_CODES", issue.code);
            }
        }
    }
}
