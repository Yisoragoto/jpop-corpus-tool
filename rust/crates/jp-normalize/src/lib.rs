//! 归一化层：把「人写出来的曲名 / 歌手名」变成可以互相比较的键。
//!
//! 移植自 Python 侧 `scraper/normalize.py`，**逐字对齐**。
//! 迁移期两边共用一个 `corpus.db`，键算得不一样就会一边认为重复、
//! 另一边认为是新的，库会被写脏。所以这里的每条规则都拿真实数据对过账。
//!
//! 设计约束：归一化**绝不修改原值**。每个函数都返回同时带 `original` 和
//! `normalized` 的对象，将来换了规则可以拿原值重跑。
//!
//! 匹配失败的绝大多数原因都在这一层：全角半角、大小写、feat. 的七八种写法、
//! 【】［］（）四种括号、以及 Live / Remastered / TV size 这类版本后缀。

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

// ────────────────────────────── 字符级规则 ──────────────────────────────

/// 各类括号的字符集，用于剥掉括号段两端的残留符号。
const BRACKET_CHARS: &str = "()[]{}（）［］〔〕【】〈〉《》「」『』";

/// Python 侧 `_PUNCT_RE` 的字符表，逐字搬过来。
///
/// **不能用 `is_alphanumeric()` 代替。** 那样两个方向都会错：
/// 片假名长音符 `ー` 的 Unicode 类别是 Lm（修饰字母），`is_alphanumeric`
/// 判真会把它留下，而 Python 当标点去掉（「アルジャーノン」→「アルジャノン」）；
/// 反过来 `♪` `→` `☆` 这类不在表里、Python 会保留的符号又会被丢掉。
///
/// 拿真实曲库量过：294 个曲名 / 歌手 / 专辑 / 人名里有 42 个因此对不上。
const PUNCT: &str = concat!(
    "「」『』\"'’‘“”",
    "()[]{}（）［］〔〕【】〈〉《》",
    "-–—―ー_.,!?！？、。・:：;/／\\~〜=+*&#@｜|^%$…‥·",
);

/// 匹配一段括号（半角圆/方/花、全角圆/方、日文角括号）。
/// 曲名里的版本信息几乎都在里面。
static PAREN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[(\[{（［〔【〈《][^()\[\]{}（）［］〔〕【】〈〉《》]*[)\]}）］〕】〉》]").unwrap()
});

/// feat 的各种写法。
///
/// Python 用零宽先行 `(?=\s|$)`，`regex` crate 不支持先行断言；
/// 这里改成把那个空白一起吃掉。后续要么删空白、要么对切分结果做 trim，
/// 两种用法下结果都等价。
const FEAT_PATTERN: &str =
    r"(?i:\bfeat\.?(?:\s|$)|\bft\.?(?:\s|$)|\bfeaturing\b|\bwith\b|\bw/|ｆｅａｔ)";

static FEAT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(FEAT_PATTERN).unwrap());

/// 歌手串的分隔符。斜杠是本项目库里的合作分隔符；顿号、× 也常见。
/// 逗号和 & 不拆——「Mrs. GREEN APPLE」这类名字本身带这些符号。
static ARTIST_SPLIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"[/／、]|[×✕✖]|{FEAT_PATTERN}")).unwrap());

static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// 全角→半角、兼容字符折叠。ＴＶ→TV、①→1、㈱→(株)。
pub fn nfkc(text: &str) -> String {
    text.nfkc().collect()
}

/// 大小写折叠。Python 用 `casefold`，Rust 只有 `to_lowercase`——
/// 差别仅在 ß→ss 这类特殊折叠上，日语曲库里不出现。
pub fn fold_case(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// 剥掉所有括号段，保留外面的部分。嵌套多剥几轮。
pub fn strip_brackets(text: &str) -> String {
    let mut prev = text.to_string();
    for _ in 0..3 {
        let stripped = PAREN_RE.replace_all(&prev, " ").into_owned();
        if stripped == prev {
            break;
        }
        prev = stripped;
    }
    WS_RE.replace_all(&prev, " ").trim().to_string()
}

/// 把括号里的内容取出来，用于识别版本标记。
pub fn bracket_contents(text: &str) -> Vec<String> {
    PAREN_RE
        .find_iter(text)
        .map(|m| {
            m.as_str()
                .trim_matches(|c: char| BRACKET_CHARS.contains(c) || c == ' ')
                .to_string()
        })
        .collect()
}

/// 比较用的键：NFKC → 折大小写 → 去 feat → 去标点 → 去所有空白。
///
/// 去掉全部空白而不是折叠成一个，是因为日文曲名里空格的有无非常随意，
/// 「夜に駆ける」和「夜に 駆ける」必须等价。
pub fn matching_key(text: &str) -> String {
    let plain: String = text.nfkc().flat_map(char::to_lowercase).collect();
    let plain = FEAT_RE.replace_all(&plain, " ");
    let key: String = plain
        .chars()
        .filter(|c| !PUNCT.contains(*c) && !c.is_whitespace())
        .collect();
    if key.is_empty() {
        // 整首歌名就是标点的（きのこ帝国有一首叫「&」），剥完是空串，
        // 空串跟谁都能匹配上。这时退回只做全角/大小写归一。
        plain.chars().filter(|c| !c.is_whitespace()).collect()
    } else {
        key
    }
}

// ────────────────────────────── 版本标记 ──────────────────────────────

/// 正则 → 规范标签。规范化后不同写法能互相比较：
/// 「(Live)」「(ライヴ)」「- live version」都归成 `live`。
static VERSION_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    let raw: &[(&str, &str)] = &[
        (r"(?i)\blive\b|ライブ|ライヴ", "live"),
        (r"(?i)\binst(?:rumental)?\b|インスト", "instrumental"),
        (
            r"(?i)\bkaraoke\b|カラオケ|\boff\s*vocal\b|オフボーカル",
            "karaoke",
        ),
        (r"(?i)\bacoustic\b|アコースティック", "acoustic"),
        (r"(?i)\bre-?mix\b|リミックス|\bmix\b", "remix"),
        (r"(?i)\bremaster(?:ed)?\b|リマスター", "remaster"),
        (r"(?i)\bcover\b|カバー|カヴァー", "cover"),
        (r"(?i)\bdemo\b|デモ", "demo"),
        (
            r"(?i)\btv\s*(?:size|edit|ver\.?|version)\b|\btv\s*サイズ|テレビサイズ",
            "tv_size",
        ),
        (r"(?i)\bradio\s*edit\b", "radio_edit"),
        (
            r"(?i)\balbum\s*(?:ver\.?|version|mix)\b|アルバムバージョン",
            "album_version",
        ),
        (
            r"(?i)\bsingle\s*(?:ver\.?|version|mix)\b|シングルバージョン",
            "single_version",
        ),
        (r"(?i)\bbonus\s*track\b|ボーナストラック", "bonus_track"),
        (r"(?i)\bextended\b|\blong\s*(?:ver\.?|version)\b", "extended"),
        (
            r"(?i)\bshort\s*(?:ver\.?|version)\b|ショートバージョン",
            "short_version",
        ),
        (r"(?i)\benglish\s*(?:ver\.?|version)\b|英語版", "english_version"),
        (
            r"(?i)\bjapanese\s*(?:ver\.?|version)\b|日本語版",
            "japanese_version",
        ),
        (r"(?i)\boriginal\b|原曲", "original"),
        (r"(?i)\bexplicit\b", "explicit"),
        // provider 会把 MV / Lyric Video 当成独立曲目收录，
        // 它们不是我们要的音频版本
        (
            r"(?i)\b(?:lyric|music|official)?\s*video\b|\bmv\b|ミュージックビデオ",
            "video",
        ),
    ];
    raw.iter()
        .map(|(p, tag)| (Regex::new(p).expect("版本正则写死在代码里"), *tag))
        .collect()
});

/// 「不是原版」的标签。文件说原版而候选带这些标签（或反过来）应当扣分。
pub const VARIANT_TAGS: &[&str] = &[
    "live",
    "instrumental",
    "karaoke",
    "acoustic",
    "remix",
    "cover",
    "demo",
    "tv_size",
    "radio_edit",
    "extended",
    "short_version",
    "english_version",
    "japanese_version",
    "video",
];

/// 尾部「- xxx」形式的版本后缀。限定长度，避免把「A - B」这种正常曲名吃掉。
static TRAILING_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[-–—]\s*([^-–—]{1,40})$").unwrap());

/// 识别文本里的版本标记，返回规范标签集合。
///
/// 只看括号内和结尾的「- xxx」段，不看整个曲名——否则
/// 「Live at Budokan」这种本名带 Live 的曲子会被永久误判成 Live 版。
pub fn version_tags(text: &str) -> BTreeSet<String> {
    let mut segments = bracket_contents(text);
    if let Some(caps) = TRAILING_VERSION_RE.captures(text.trim()) {
        segments.push(caps[1].to_string());
    }
    let haystack = nfkc(&segments.join(" "));
    if haystack.trim().is_empty() {
        return BTreeSet::new();
    }
    VERSION_PATTERNS
        .iter()
        .filter(|(re, _)| re.is_match(&haystack))
        .map(|(_, tag)| (*tag).to_string())
        .collect()
}

/// 去掉结尾的「- TV size」「- Remastered 2019」这类版本后缀。
///
/// 只有当尾段确实命中版本模式时才剥，否则「Fire - Rain」这种本名带连字符的
/// 曲子会被砍掉半个标题。
pub fn strip_version_suffix(text: &str) -> String {
    let mut body = text.trim().to_string();
    for _ in 0..2 {
        // 「X - Live - Remastered」剥两轮
        let Some(caps) = TRAILING_VERSION_RE.captures(&body) else {
            break;
        };
        let whole = caps.get(0).expect("整体分组一定存在");
        let segment = nfkc(&caps[1]);
        if !VERSION_PATTERNS.iter().any(|(re, _)| re.is_match(&segment)) {
            break;
        }
        let start = whole.start();
        body = body[..start].trim().to_string();
    }
    body
}

fn split_names(text: &str) -> Vec<String> {
    ARTIST_SPLIT_RE
        .split(text)
        .map(|p| p.trim_matches(|c: char| " .,-&".contains(c)).to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// 把曲名里的 feat. 部分摘出来。
///
/// 返回 (去掉 feat 段的曲名, 协作歌手)。
/// 「アイドル (feat. Ado)」→ ("アイドル", ["Ado"])
pub fn extract_feat_artists(text: &str) -> (String, Vec<String>) {
    if text.is_empty() {
        return (String::new(), Vec::new());
    }
    static PAREN_FEAT_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"[(\[（［][^()\[\]（）［］]*{FEAT_PATTERN}[^()\[\]（）［］]*[)\]）］]"
        ))
        .unwrap()
    });

    let mut featured: Vec<String> = Vec::new();

    // 先处理括号里的 feat 段
    let mut body = String::new();
    let mut last = 0usize;
    for m in PAREN_FEAT_RE.find_iter(text) {
        body.push_str(&text[last..m.start()]);
        let inner = m
            .as_str()
            .trim_matches(|c: char| BRACKET_CHARS.contains(c) || c == ' ');
        // count=1：只去掉第一个 feat 标记，后面的内容都是人名
        let rest = FEAT_RE.replacen(inner, 1, "");
        let rest = rest.trim_matches(|c: char| " .,-".contains(c));
        if !rest.is_empty() {
            featured.extend(split_names(rest));
        }
        body.push(' ');
        last = m.end();
    }
    body.push_str(&text[last..]);

    // 再处理裸的尾部 feat 段
    if let Some(m) = FEAT_RE.find(&body) {
        featured.extend(split_names(&body[m.end()..]));
        body = body[..m.start()].to_string();
    }

    let cleaned = WS_RE
        .replace_all(&body, " ")
        .trim_matches(|c: char| " -–—".contains(c))
        .to_string();
    (cleaned, featured)
}

/// 把合作曲的歌手串拆成单个歌手。拆不出来就原样返回单元素列表。
///
/// `slash_only` 为真时只按斜杠拆——这是本项目库里唯一可靠的合作分隔符，
/// 顿号和 × 会误伤部分乐队名。查询构造时可以放开成全分隔符。
pub fn split_artists(name: &str, slash_only: bool) -> Vec<String> {
    if name.trim().is_empty() {
        return Vec::new();
    }
    let parts: Vec<String> = if slash_only {
        name.split(['/', '／'])
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect()
    } else {
        ARTIST_SPLIT_RE
            .split(name)
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect()
    };
    if parts.is_empty() {
        vec![name.trim().to_string()]
    } else {
        parts
    }
}

// ────────────────────────── 归一化结果对象 ──────────────────────────

/// 一次归一化的完整结果。`original` 永远是传进来的原样字符串。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedText {
    pub original: String,
    /// 完整文本的比较键（含括号内容）
    pub normalized: String,
    /// 剥掉括号后的比较键，用于「主标题相同、版本不同」的判断
    pub base: String,
    /// 剥掉括号后的可读形式，用于再次发起搜索
    pub base_display: String,
    pub tags: BTreeSet<String>,
    pub featured: Vec<String>,
    pub parts: Vec<String>,
}

impl NormalizedText {
    /// Python 那边 `__bool__` 看的是 `normalized` 非空。
    pub fn is_empty(&self) -> bool {
        self.normalized.is_empty()
    }

    /// 只保留「非原版」标签，用于版本冲突判定。
    pub fn variant_tags(&self) -> BTreeSet<String> {
        self.tags
            .iter()
            .filter(|t| VARIANT_TAGS.contains(&t.as_str()))
            .cloned()
            .collect()
    }
}

fn build(
    original: &str,
    body: &str,
    tags: BTreeSet<String>,
    featured: Vec<String>,
    parts: Vec<String>,
) -> NormalizedText {
    let base_display = strip_version_suffix(&strip_brackets(body));
    let base = {
        let b = matching_key(&base_display);
        if b.is_empty() { matching_key(body) } else { b }
    };
    NormalizedText {
        original: original.to_string(),
        normalized: matching_key(body),
        base,
        base_display: if base_display.is_empty() {
            body.trim().to_string()
        } else {
            base_display
        },
        tags,
        featured,
        parts,
    }
}

/// 曲名归一化。摘出 feat. 歌手和版本标记，原值保留。
pub fn normalize_title(title: &str) -> NormalizedText {
    let tags = version_tags(title);
    let (body, featured) = extract_feat_artists(title);
    let body = if body.is_empty() { title } else { &body };
    build(title, body, tags, featured, Vec::new())
}

/// 歌手名归一化。
///
/// `parts` 是拆开后的单个歌手（按斜杠），`featured` 是 feat. 后面跟的人。
/// 「キタニタツヤ/suis」→ parts=("キタニタツヤ", "suis")
/// 「YOASOBI feat. Ado」→ featured=("Ado",)
pub fn normalize_artist(artist: &str) -> NormalizedText {
    let (body, featured) = extract_feat_artists(artist);
    let body = if body.is_empty() { artist } else { &body };
    let parts = split_artists(body, true);
    build(artist, body, BTreeSet::new(), featured, parts)
}

/// 专辑名里的版本线索。和曲名不同，这些词在专辑名里常常**不带括号**
/// （「THE BEST」「10 & Harmony COMPLETE BOX」），所以扫整个字符串。
static ALBUM_EXTRA_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    let raw: &[(&str, &str)] = &[
        (r"(?i)\bdeluxe\b|デラックス", "deluxe"),
        (r"(?i)初回|限定盤|通常盤", "limited"),
        (
            r"(?i)\bbest\b|ベスト|\bgreatest\s*hits\b|\bcomplete\s*box\b|\bbox\s*set\b\
|\bcollection\b|\banthology\b|\bcompilation\b|\bmegamix\b\
|コンプリート|オムニバス|\bvarious\s*artists\b",
            "compilation",
        ),
    ];
    raw.iter()
        .map(|(p, tag)| (Regex::new(p).expect("专辑正则写死在代码里"), *tag))
        .collect()
});

/// 专辑名归一化。
///
/// Deluxe / 初回限定盤 / ベスト / COMPLETE BOX 这类视作版本标记。
/// `compilation` 尤其重要：同一首歌几乎总能在某张精选集或杂锦碟里找到，
/// 没有它，匹配到的专辑经常是「Halloween Mix for Kids」这种。
pub fn normalize_album(album: &str) -> NormalizedText {
    let plain = nfkc(album);
    let mut tags = version_tags(album);
    for (re, tag) in ALBUM_EXTRA_PATTERNS.iter() {
        if re.is_match(&plain) {
            tags.insert((*tag).to_string());
        }
    }
    build(album, album, tags, Vec::new(), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(text: &str) -> Vec<String> {
        version_tags(text).into_iter().collect()
    }

    #[test]
    fn matching_key_matches_python() {
        // 这些期望值全部是拿 Python 的 matching_key 实跑出来的
        assert_eq!(matching_key("YOASOBI"), "yoasobi");
        assert_eq!(matching_key("ＹＯＡＳＯＢＩ"), "yoasobi");
        assert_eq!(matching_key("Mrs. GREEN APPLE"), "mrsgreenapple");
        assert_eq!(matching_key("山口　一郎"), "山口一郎");
        assert_eq!(matching_key("夜に 駆ける"), "夜に駆ける");
        assert_eq!(matching_key("アルジャーノン"), "アルジャノン");
        assert_eq!(matching_key("!?"), "!?");
        assert_eq!(matching_key("&"), "&");
    }

    #[test]
    fn version_tags_only_look_at_brackets_and_tail() {
        assert_eq!(tags("夜に駆ける (Live)"), vec!["live"]);
        assert_eq!(tags("群青 - TV size"), vec!["tv_size"]);
        // 本名带 Live 的不该被误判
        assert!(tags("Live at Budokan").is_empty());
    }

    #[test]
    fn version_suffix_is_only_stripped_when_it_is_one() {
        assert_eq!(strip_version_suffix("群青 - TV size"), "群青");
        // 正常曲名不该被砍
        assert_eq!(strip_version_suffix("Fire - Rain"), "Fire - Rain");
    }

    #[test]
    fn feat_is_extracted_from_brackets_and_tail() {
        let (body, feat) = extract_feat_artists("アイドル (feat. Ado)");
        assert_eq!(body, "アイドル");
        assert_eq!(feat, vec!["Ado"]);

        let (body, feat) = extract_feat_artists("YOASOBI feat. Ado");
        assert_eq!(body, "YOASOBI");
        assert_eq!(feat, vec!["Ado"]);
    }

    #[test]
    fn artist_parts_split_on_slash_only() {
        let a = normalize_artist("キタニタツヤ/suis");
        assert_eq!(a.parts, vec!["キタニタツヤ", "suis"]);
        // 逗号和 & 不拆
        let a = normalize_artist("Mrs. GREEN APPLE");
        assert_eq!(a.parts, vec!["Mrs. GREEN APPLE"]);
    }

    #[test]
    fn base_drops_brackets_but_normalized_keeps_them() {
        let t = normalize_title("夜に駆ける (Official Video)");
        assert_eq!(t.base_display, "夜に駆ける");
        assert_ne!(t.normalized, t.base);
        assert!(t.tags.contains("video"));
    }

    #[test]
    fn compilation_is_detected_without_brackets() {
        let a = normalize_album("THE BEST");
        assert!(a.tags.contains("compilation"));
        let a = normalize_album("834.194");
        assert!(a.tags.is_empty());
    }

    #[test]
    fn original_is_never_modified() {
        let raw = "  夜に駆ける (Live)  ";
        assert_eq!(normalize_title(raw).original, raw);
        assert_eq!(normalize_album(raw).original, raw);
        assert_eq!(normalize_artist(raw).original, raw);
    }
}
