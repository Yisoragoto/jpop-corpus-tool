//! Decoded word-timed lyrics. Positions are Unicode scalar indices, `[start, end)`.
//!
//! YRC/QRC/LYS word clocks are absolute; KRC word clocks are relative to their line.
//! QRC comes either wrapped in QQ's XML (`LyricContent="…"`) or as the bare
//! `[line,duration]word(start,duration)` text that players embed in tags.
//! LYS (Lyricify Syllable) rows are `[property]word(start,duration)…` with no
//! line clock: a line spans its first word onset to its last word end.
//! `[offset:N]` follows jp-import's LRC convention: subtract N milliseconds,
//! clamping at zero. Validate clocks before applying the offset.
//!
//! TTML targets the AMLL/Apple lyric dialect: all begin/end values (including
//! seconds and milliseconds) are absolute song times, not parent-relative SMIL
//! times. Sequential containers, frame/tick clocks and non-media time bases are
//! rejected. See https://github.com/amll-dev/amll-ttml-db/blob/main/instructions/ttml-specification-en.md .
//! Untimed main text downgrades that line; a high-precision result still needs
//! at least one actually timed line. A span may contain several characters; never invent
//! individual character clocks. Missing word ends remain `None`.

use anyhow::{Context, Result, bail, ensure};
use roxmltree::{Document, Node, ParsingOptions};
use std::fmt::Write;

const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 64;
const MAX_XML_NODES: u32 = 200_000;
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const TTML_NS: &str = "http://www.w3.org/ns/ttml";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimedLyrics {
    pub lines: Vec<TimedLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedLine {
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    pub text: String,
    pub words: Vec<TimedWord>,
    /// 源文件自带的注音（TTML 的 `tts:ruby`）。别的格式没有，是空的
    pub rubies: Vec<RubyText>,
}

/// 源文件给的一段注音：`text` 里第 `start..end` 个字（Unicode 字符）读作 `reading`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RubyText {
    pub start: usize,
    pub end: usize,
    pub reading: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedWord {
    pub start: usize,
    pub end: usize,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
}

impl TimedLyrics {
    /// Millisecond enhanced LRC. Every known word end is emitted at its actual
    /// character boundary, including ends before the next word's onset. Adjacent
    /// `<end><begin>` marks preserve silence without inventing lyric characters.
    /// Older onset-only LRC readers may discard these end marks; persist the
    /// original TimedWord.end_ms when exact durations matter.
    /// Like jp-import, consumers may trim outer whitespace on re-import.
    pub fn to_enhanced_lrc(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let _ = write!(out, "[{}]", lrc_clock(line.start_ms));
            let len = line.text.chars().count();
            // Public structs can also be constructed by callers. Ignore invalid
            // ranges instead of indexing a String with untrusted byte offsets.
            let mut marks = Vec::new();
            for word in line
                .words
                .iter()
                .filter(|w| w.start < w.end && w.end <= len)
            {
                marks.push((word.start, 1, word.start_ms));
                if let Some(end) = word.end_ms {
                    marks.push((word.end, 0, end));
                }
            }
            // End before begin when two words share a character boundary.
            marks.sort_by_key(|&(at, kind, _)| (at, kind));
            let mut marks = marks.into_iter().peekable();
            for (at, ch) in line.text.chars().enumerate() {
                while marks.peek().is_some_and(|m| m.0 == at) {
                    if let Some((_, _, time)) = marks.next() {
                        let _ = write!(out, "<{}>", lrc_clock(time));
                    }
                }
                out.push(ch);
            }
            for (_, _, time) in marks {
                let _ = write!(out, "<{}>", lrc_clock(time));
            }
            out.push('\n');
        }
        out
    }
}

fn lrc_clock(ms: u64) -> String {
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

#[derive(Debug, Clone, Copy)]
struct Clock {
    start: u64,
    end: Option<u64>,
}

#[derive(Clone, Copy)]
struct Glyph {
    ch: char,
    word: Option<usize>,
    preserve: bool,
}

#[derive(Default)]
struct TextBuilder {
    glyphs: Vec<Glyph>,
    clocks: Vec<Clock>,
    /// 注音：(第几个 glyph 起, 到第几个止, 读音)，`finish` 时换算成正文里的字符位置
    rubies: Vec<(usize, usize, String)>,
}

impl TextBuilder {
    fn clock(&mut self, clock: Clock) -> usize {
        let id = self.clocks.len();
        self.clocks.push(clock);
        id
    }

    fn raw(&mut self, text: &str, word: Option<usize>) {
        self.glyphs.extend(text.chars().map(|ch| Glyph {
            ch,
            word,
            preserve: false,
        }));
    }

    fn xml(&mut self, text: &str, word: Option<usize>, preserve: bool) {
        // Pretty-print indentation is not a Japanese word separator. A plain
        // space between spans IS significant. xml:space="preserve" bypasses this.
        if !preserve && text.chars().all(xml_whitespace) && text.contains(['\n', '\r']) {
            return;
        }
        for mut ch in text.chars() {
            if !preserve && xml_whitespace(ch) {
                ch = ' ';
                if self
                    .glyphs
                    .last()
                    .is_some_and(|g| g.ch == ' ' && !g.preserve)
                {
                    continue;
                }
            }
            self.glyphs.push(Glyph { ch, word, preserve });
        }
    }

    fn finish(self, xml: bool) -> Result<(String, Vec<TimedWord>)> {
        self.finish_with_rubies(xml).map(|(text, words, _)| (text, words))
    }

    fn finish_with_rubies(self, xml: bool) -> Result<(String, Vec<TimedWord>, Vec<RubyText>)> {
        let trim = |g: &Glyph| {
            !g.preserve
                && if xml {
                    xml_whitespace(g.ch)
                } else {
                    g.ch.is_whitespace()
                }
        };
        let first = self
            .glyphs
            .iter()
            .position(|g| !trim(g))
            .unwrap_or(self.glyphs.len());
        let last = self
            .glyphs
            .iter()
            .rposition(|g| !trim(g))
            .map_or(first, |i| i + 1);
        let glyphs = &self.glyphs[first..last];
        let text = glyphs.iter().map(|g| g.ch).collect();
        let mut words = Vec::new();
        let mut missing_main = false;
        let mut at = 0;
        while at < glyphs.len() {
            let word = glyphs[at].word;
            let mut end = at + 1;
            while end < glyphs.len() && glyphs[end].word == word {
                end += 1;
            }
            if let Some(id) = word {
                let mut start = at;
                let mut stop = end;
                while start < stop && glyphs[start].ch.is_whitespace() {
                    start += 1;
                }
                while stop > start && glyphs[stop - 1].ch.is_whitespace() {
                    stop -= 1;
                }
                if start < stop {
                    let clock = self.clocks[id];
                    words.push(TimedWord {
                        start,
                        end: stop,
                        start_ms: clock.start,
                        end_ms: clock.end,
                    });
                }
            } else {
                missing_main |= !glyphs[at..end].iter().all(|g| untimed_separator(g.ch));
            }
            at = end;
        }
        // A partially timed line falls back as a whole, without inventing
        // timing for its missing text. Other fully timed lines remain usable.
        if missing_main {
            words.clear();
        }
        // 注音的位置跟着去掉的行首空白挪；注音底下的字两头的空白不算
        let mut rubies = Vec::new();
        for (from, to, reading) in &self.rubies {
            let mut start = from.saturating_sub(first).min(glyphs.len());
            let mut stop = to.saturating_sub(first).min(glyphs.len());
            while start < stop && glyphs[start].ch.is_whitespace() {
                start += 1;
            }
            while stop > start && glyphs[stop - 1].ch.is_whitespace() {
                stop -= 1;
            }
            let reading = reading.trim();
            if start < stop && !reading.is_empty() {
                rubies.push(RubyText { start, end: stop, reading: reading.to_string() });
            }
        }
        Ok((text, words, rubies))
    }
}

fn xml_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\r' | '\n')
}

fn untimed_separator(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_ascii_punctuation()
        || "、。！？「」『』（）［］【】〈〉《》〔〕…‥—―〜～・：；，．“”‘’".contains(ch)
}

fn check_input(text: &str) -> Result<()> {
    ensure!(
        text.len() <= MAX_INPUT_BYTES,
        "lyrics exceed the 8 MiB input limit"
    );
    Ok(())
}

fn integer(value: &str) -> Result<u64> {
    let value = value.trim();
    ensure!(
        !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
        "invalid unsigned timestamp: {value:?}"
    );
    value
        .parse()
        .with_context(|| format!("timestamp overflow: {value}"))
}

fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).context("timestamp overflow")
}

fn fields<const N: usize>(value: &str) -> Result<[u64; N]> {
    let mut values = value.split(',');
    let mut result = [0; N];
    for item in &mut result {
        *item = integer(values.next().context("missing timing field")?)?;
    }
    ensure!(values.next().is_none(), "too many timing fields: {value:?}");
    Ok(result)
}

fn duration_clock(start: u64, duration: u64) -> Result<Clock> {
    Ok(Clock {
        start,
        end: Some(add(start, duration)?),
    })
}

fn line_header(text: &str) -> Result<(Clock, &str)> {
    let rest = text
        .strip_prefix('[')
        .context("expected [line_ms,duration_ms]")?;
    let (header, body) = rest
        .split_once(']')
        .context("unterminated line timestamp")?;
    let [start, duration] = fields(header)?;
    Ok((duration_clock(start, duration)?, body))
}

/// Standalone metadata tags only. Unknown free text is never silently dropped.
fn metadata(text: &str) -> Option<(&str, &str)> {
    let inner = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (key, value) = inner.split_once(':')?;
    (!key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .then_some((key, value))
}

fn record_offset(key: &str, value: &str, offset: &mut Option<i64>) -> Result<()> {
    if key.eq_ignore_ascii_case("offset") && offset.is_none() {
        *offset = Some(
            value
                .trim()
                .parse()
                .context("invalid offset milliseconds")?,
        );
    }
    Ok(())
}

fn shift(ms: u64, offset: i64) -> Result<u64> {
    if offset >= 0 {
        Ok(ms.saturating_sub(offset as u64))
    } else {
        add(ms, offset.unsigned_abs())
    }
}

/// Validate public/caller-constructed lyrics before storing word timings.
/// Every main-text character must be covered, except whitespace/punctuation;
/// word onsets must be ordered and all known ends must fit their line.
pub fn validate(lyrics: &TimedLyrics) -> Result<()> {
    ensure!(!lyrics.lines.is_empty(), "no word-timed main lyrics");
    ensure!(
        lyrics.lines.iter().any(|l| !l.words.is_empty()),
        "no actual word-timed main lyrics"
    );
    for line in &lyrics.lines {
        if let Some(end) = line.end_ms {
            ensure!(end >= line.start_ms, "line ends before it starts");
        }
        if line.words.is_empty() {
            continue;
        }
        let mut previous = line.start_ms;
        let mut previous_end = 0;
        let chars: Vec<_> = line.text.chars().collect();
        let len = chars.len();
        for word in &line.words {
            ensure!(
                word.start < word.end && word.end <= len && word.start >= previous_end,
                "invalid word character range"
            );
            ensure!(
                chars[previous_end..word.start]
                    .iter()
                    .copied()
                    .all(untimed_separator),
                "main lyric text is missing word timing"
            );
            ensure!(
                chars[word.start..word.end]
                    .iter()
                    .any(|c| !c.is_whitespace()),
                "word contains only whitespace"
            );
            ensure!(
                word.start_ms >= previous,
                "word timestamps go backwards or precede line"
            );
            ensure!(
                line.end_ms.is_none_or(|end| word.start_ms <= end),
                "word starts after line ends"
            );
            if let Some(end) = word.end_ms {
                ensure!(end > word.start_ms, "word ends before or at its start");
                ensure!(
                    line.end_ms.is_none_or(|bound| end <= bound),
                    "word ends at {end} after line end {:?} (line begins {})",
                    line.end_ms,
                    line.start_ms
                );
            }
            previous = word.start_ms;
            previous_end = word.end;
        }
        ensure!(
            chars[previous_end..].iter().copied().all(untimed_separator),
            "main lyric text is missing word timing"
        );
    }
    Ok(())
}

fn complete(lines: Vec<TimedLine>, offset: i64) -> Result<TimedLyrics> {
    let mut lyrics = TimedLyrics { lines };
    for line in &mut lyrics.lines {
        if let Some(bound) = line.end_ms
            && let Some(actual) = line
                .words
                .iter()
                .map(|w| w.end_ms.unwrap_or(w.start_ms))
                .max()
            && actual > bound
        {
            if actual - bound <= 2 {
                // Provider line lengths may be rounded to milliseconds. Keep
                // the original word end; the container may include that clock.
                line.end_ms = Some(actual);
            } else {
                line.words.clear();
            }
        }
        if line.words.iter().any(|w| w.end_ms == Some(w.start_ms)) {
            let chars: Vec<_> = line.text.chars().collect();
            let zero_main = line.words.iter().any(|w| {
                w.end_ms == Some(w.start_ms)
                    && chars
                        .get(w.start..w.end)
                        .is_none_or(|s| s.iter().copied().any(|c| !untimed_separator(c)))
            });
            if zero_main {
                // A zero-duration sung unit is not a usable high-precision
                // clock. Retain this line's text/time as an honest fallback.
                line.words.clear();
            } else {
                // Provider punctuation markers are not sung timing units.
                line.words.retain(|w| w.end_ms != Some(w.start_ms));
            }
        }
    }
    validate(&lyrics)?;
    for line in &mut lyrics.lines {
        line.start_ms = shift(line.start_ms, offset)?;
        line.end_ms = line.end_ms.map(|ms| shift(ms, offset)).transpose()?;
        for word in &mut line.words {
            word.start_ms = shift(word.start_ms, offset)?;
            word.end_ms = word.end_ms.map(|ms| shift(ms, offset)).transpose()?;
        }
    }
    // Stable ordering preserves simultaneous duet lines.
    lyrics.lines.sort_by_key(|line| line.start_ms);
    Ok(lyrics)
}

#[derive(Clone, Copy)]
enum PrefixFormat {
    Yrc,
    Krc,
}

/// Numeric markers and comma tuples are reserved for timing; ordinary `(笑)`
/// stays text. Recognize malformed tuples too, so NaN/invalid clocks cannot
/// silently turn into lyric text attached to the preceding timed word.
fn next_marker(text: &str, open: char, close: char) -> Result<Option<(usize, usize, &str)>> {
    for (at, ch) in text.char_indices().filter(|(_, ch)| *ch == open) {
        let rest = &text[at + ch.len_utf8()..];
        let numeric = rest
            .trim_start()
            .starts_with(|ch: char| ch.is_ascii_digit() || matches!(ch, '-' | '+' | ','));
        // Stop at another opening delimiter: scanning nested literal brackets
        // must not repeatedly rescan the entire remainder of a large input.
        let tuple = rest
            .find([open, close, ','])
            .is_some_and(|i| rest[i..].starts_with(','));
        if !numeric && !tuple {
            continue;
        }
        let end = rest.find(close).context("unterminated word timestamp")?;
        return Ok(Some((
            at,
            at + ch.len_utf8() + end + close.len_utf8(),
            &rest[..end],
        )));
    }
    Ok(None)
}

fn prefix_line(body: &str, line: Clock, format: PrefixFormat) -> Result<TimedLine> {
    let (open, close) = match format {
        PrefixFormat::Yrc => ('(', ')'),
        PrefixFormat::Krc => ('<', '>'),
    };
    let mut builder = TextBuilder::default();
    let mut rest = body;
    let mut active = None;
    while let Some((at, end, mark)) = next_marker(rest, open, close)? {
        // 两个时间标记挨着（或行尾多一个）就是一个没有字的单位。酷狗的 KRC 里实测有
        // （「パラノイドパレード」第 19 行），以前整首报错。没字的单位不产生词，丢掉即可。
        builder.raw(&rest[..at], active);
        let [start, duration, _flags] = fields(mark)?;
        let start = match format {
            PrefixFormat::Yrc => start,
            PrefixFormat::Krc => add(line.start, start)?,
        };
        active = Some(builder.clock(duration_clock(start, duration)?));
        rest = &rest[end..];
    }
    builder.raw(rest, active);
    let (text, words) = builder.finish(false)?;
    Ok(TimedLine {
        start_ms: line.start,
        end_ms: line.end,
        text,
        words,
        rubies: Vec::new(),
    })
}

fn parse_prefix(text: &str, format: PrefixFormat) -> Result<TimedLyrics> {
    check_input(text)?;
    let mut offset = None;
    let mut lines = Vec::new();
    for (index, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let raw = raw.trim_start();
        if raw.trim().is_empty() {
            continue;
        }
        if let Some((key, value)) = metadata(raw) {
            record_offset(key, value, &mut offset)?;
            continue;
        }
        // Netease's JSON credit records are metadata, not lyric rows.
        if matches!(format, PrefixFormat::Yrc) && raw.starts_with('{') {
            continue;
        }
        let parsed = (|| -> Result<Option<TimedLine>> {
            let (clock, body) = line_header(raw)?;
            if body.trim().is_empty() {
                return Ok(None);
            }
            Ok(Some(prefix_line(body, clock, format)?))
        })()
        .with_context(|| format!("invalid lyric line {}", index + 1))?;
        if let Some(line) = parsed {
            lines.push(line);
        }
    }
    complete(lines, offset.unwrap_or(0))
}

pub fn parse_yrc(text: &str) -> Result<TimedLyrics> {
    parse_prefix(text, PrefixFormat::Yrc)
}

/// Input is the already decoded KRC text, without the krc1/compressed envelope.
pub fn parse_krc(text: &str) -> Result<TimedLyrics> {
    parse_prefix(text, PrefixFormat::Krc)
}

/// Check lexical nesting BEFORE building the XML tree. Comments, CDATA and
/// quoted attributes may contain angle brackets and do not add nesting.
fn xml_preflight(text: &str) -> Result<()> {
    check_input(text)?;
    let mut rest = text;
    let mut depth = 0usize;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        let special = if rest.starts_with("<!--") {
            Some(("-->", 4))
        } else if rest.starts_with("<![CDATA[") {
            Some(("]]>", 9))
        } else if rest.starts_with("<?") {
            Some(("?>", 2))
        } else {
            None
        };
        if let Some((delimiter, skip)) = special {
            let end = rest[skip..]
                .find(delimiter)
                .context("unterminated XML construct")?;
            rest = &rest[skip + end + delimiter.len()..];
            continue;
        }
        ensure!(
            !rest.starts_with("<!"),
            "XML DTD/declarations are forbidden"
        );
        let mut quote = None;
        let mut end = None;
        for (i, ch) in rest.char_indices().skip(1) {
            if let Some(q) = quote {
                if ch == q {
                    quote = None;
                }
            } else if ch == '\'' || ch == '"' {
                quote = Some(ch);
            } else if ch == '>' {
                end = Some(i);
                break;
            }
        }
        let end = end.context("unterminated XML tag")?;
        if rest.starts_with("</") {
            depth = depth.checked_sub(1).context("unbalanced XML closing tag")?;
        } else {
            ensure!(
                depth < MAX_XML_DEPTH,
                "XML exceeds depth limit {MAX_XML_DEPTH}"
            );
            if !rest[..end].trim_end().ends_with('/') {
                depth += 1;
            }
        }
        rest = &rest[end + 1..];
    }
    ensure!(depth == 0, "unclosed XML element");
    Ok(())
}

fn safe_xml(text: &str) -> Result<Document<'_>> {
    xml_preflight(text)?;
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_XML_NODES,
        },
    )
    .context("invalid lyric XML")
}

/// QQ puts the lyric body into an XML attribute without escaping `&`
/// (「lulu.」 has one in its lyrics), so the whole song failed to parse.
/// Escape only the ampersands that do not already start an entity.
fn escape_bare_ampersands(text: &str) -> std::borrow::Cow<'_, str> {
    let entity = |rest: &str| {
        let Some(end) = rest[1..].find(';').map(|i| i + 1) else {
            return false;
        };
        let name = &rest[1..end];
        if let Some(num) = name.strip_prefix('#') {
            if let Some(hex) = num.strip_prefix(['x', 'X']) {
                !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit())
            } else {
                !num.is_empty() && num.chars().all(|c| c.is_ascii_digit())
            }
        } else {
            matches!(name, "amp" | "lt" | "gt" | "quot" | "apos")
        }
    };
    if text.match_indices('&').all(|(i, _)| entity(&text[i..])) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 16);
    for (i, c) in text.char_indices() {
        if c == '&' && !entity(&text[i..]) {
            out.push_str("&amp;");
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

/// QQ also leaves `"` and `<` unescaped inside `LyricContent="…"`
/// (「東京フラッシュ」), which ends the attribute early. The value runs up to the
/// element's closing `"/>`, so escape everything XML-significant in between.
fn repair_lyric_content(text: &str) -> std::borrow::Cow<'_, str> {
    const OPEN: &str = "LyricContent=\"";
    let Some(at) = text.find(OPEN).map(|i| i + OPEN.len()) else {
        return escape_bare_ampersands(text);
    };
    let Some(len) = text[at..].find("\"/>").or_else(|| text[at..].find("\" />")) else {
        return escape_bare_ampersands(text);
    };
    let value = &text[at..at + len];
    if !value.contains(['"', '<']) {
        return escape_bare_ampersands(text);
    }
    let mut out = String::with_capacity(text.len() + 32);
    out.push_str(&text[..at]);
    out.push_str(&escape_bare_ampersands(value).replace('"', "&quot;").replace('<', "&lt;"));
    out.push_str(&escape_bare_ampersands(&text[at + len..]));
    std::borrow::Cow::Owned(out)
}

/// Input is decoded QRC: QQ's XML wrapper, or the bare text inside it (what
/// players embed in an audio file's lyrics tag). In the XML form only the
/// primary LyricContent stream is accepted; multiple different streams are
/// ambiguous and must be selected by the caller.
pub fn parse_qrc(text: &str) -> Result<TimedLyrics> {
    if !text.trim_start_matches(BOM).trim_start().starts_with('<') {
        check_input(text)?;
        return parse_qrc_content(text);
    }
    let text = repair_lyric_content(text);
    let doc = safe_xml(&text)?;
    let mut content = None;
    for node in doc.descendants().filter(|n| n.is_element()) {
        if let Some(value) = node.attribute("LyricContent") {
            if let Some(existing) = content {
                ensure!(
                    existing == value,
                    "multiple different QRC LyricContent streams"
                );
            } else {
                content = Some(value);
            }
        }
    }
    parse_qrc_content(content.context("QRC has no LyricContent attribute")?)
}

const BOM: char = '\u{feff}';

/// The QRC body: `[offset:…]`-style headers, then `[start,duration]` rows.
/// Literal newlines in XML attributes are normalized to spaces, so rows are
/// split on `[start,duration]`, never on newlines, and XML entities are decoded
/// exactly once. Header tags may also sit on their own lines between rows
/// (bare embedded QRC carries `[kana:…]`); they are skipped wherever they appear.
fn parse_qrc_content(content: &str) -> Result<TimedLyrics> {
    let mut rest = content.trim_start_matches(BOM).trim_start();
    let mut offset = None;
    let mut lines = Vec::new();
    while !rest.trim().is_empty() {
        rest = rest.trim_start();
        if let Some(end) = rest.find(']')
            && let Some((key, value)) = metadata(&rest[..=end])
        {
            record_offset(key, value, &mut offset)?;
            rest = &rest[end + 1..];
            continue;
        }
        let (clock, body) = line_header(rest)?;
        let next = next_row(body)?;
        let row = &body[..next];
        if !row.trim().is_empty() {
            let (text, words) = paren_words(row)?;
            lines.push(TimedLine {
                start_ms: clock.start,
                end_ms: clock.end,
                text,
                words,
                rubies: Vec::new(),
            });
        }
        rest = &body[next..];
    }
    complete(lines, offset.unwrap_or(0))
}

/// Where the next `[start,duration]` row, or a `[key:value]` header on a line
/// of its own, begins. (XML turns newlines into spaces, so wrapped QRC only
/// ever splits on rows, exactly as before.)
fn next_row(body: &str) -> Result<usize> {
    let numeric = next_marker(body, '[', ']')?.map(|(at, _, _)| at);
    let own_line = |at: usize| body[..at].trim_end_matches([' ', '\t', '\r']).ends_with('\n');
    let tag = body.match_indices('[').map(|(at, _)| at).filter(|&at| own_line(at)).find(|&at| {
        body[at..]
            .find(']')
            .is_some_and(|end| metadata(&body[at..=at + end]).is_some())
    });
    Ok(match (numeric, tag) {
        (Some(a), Some(b)) => a.min(b),
        (a, b) => a.or(b).unwrap_or(body.len()),
    })
}

/// `word(start,duration)word(start,duration)…`: the text BEFORE each clock is
/// the word it times (QRC and LYS).
fn paren_words(row: &str) -> Result<(String, Vec<TimedWord>)> {
    let mut builder = TextBuilder::default();
    let mut words = row;
    while let Some((at, end, mark)) = next_marker(words, '(', ')')? {
        let [start, duration] = fields(mark)?;
        let clock = duration_clock(start, duration)?;
        if words[..at].trim().is_empty() {
            // QRC also times spaces/empty separators. Retain separator
            // text without manufacturing a sung unit or losing the song.
            builder.raw(&words[..at], None);
        } else {
            let word = builder.clock(clock);
            builder.raw(&words[..at], Some(word));
        }
        words = &words[end..];
    }
    builder.raw(words, None);
    builder.finish(false)
}

/// Lyricify Syllable: `[property]word(start,duration)…`, one row per line.
/// The property digit (singer side / background vocal) is not lyric text and is
/// dropped. A line runs from its first word onset to its last word end.
pub fn parse_lys(text: &str) -> Result<TimedLyrics> {
    check_input(text)?;
    let mut offset = None;
    let mut lines = Vec::new();
    for (index, raw) in text.trim_start_matches(BOM).lines().enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        if let Some((key, value)) = metadata(raw) {
            record_offset(key, value, &mut offset)?;
            continue;
        }
        let parsed = (|| -> Result<Option<TimedLine>> {
            let rest = raw.strip_prefix('[').context("expected [property]")?;
            let (property, body) = rest.split_once(']').context("unterminated LYS property")?;
            ensure!(
                !property.is_empty() && property.bytes().all(|b| b.is_ascii_digit()),
                "invalid LYS property: {property:?}"
            );
            if body.trim().is_empty() {
                return Ok(None);
            }
            let (text, words) = paren_words(body)?;
            let Some(first) = words.first() else {
                bail!("LYS line has no word timing");
            };
            let start_ms = first.start_ms;
            let end_ms = words.iter().filter_map(|w| w.end_ms).max();
            Ok(Some(TimedLine { start_ms, end_ms, text, words, rubies: Vec::new() }))
        })()
        .with_context(|| format!("invalid lyric line {}", index + 1))?;
        if let Some(line) = parsed {
            lines.push(line);
        }
    }
    complete(lines, offset.unwrap_or(0))
}

/// Integer arithmetic avoids NaN, infinities, rounding overflow and float casts.
/// Sub-millisecond precision is truncated after validating every digit.
fn decimal_ms(value: &str, scale: u64) -> Result<u64> {
    let (whole, fraction) = value
        .split_once('.')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    let base = integer(whole)?
        .checked_mul(scale)
        .context("timestamp overflow")?;
    let Some(fraction) = fraction else {
        return Ok(base);
    };
    ensure!(
        !fraction.is_empty() && fraction.bytes().all(|b| b.is_ascii_digit()),
        "invalid fractional time: {value:?}"
    );
    // Only s (scale 1000) and ms (scale 1) are supported.
    let mut ms = 0;
    let mut place = scale / 10;
    for digit in fraction.bytes() {
        ms += u64::from(digit - b'0') * place;
        place /= 10;
    }
    add(base, ms)
}

fn ttml_time(text: &str) -> Result<u64> {
    let text = text.trim();
    if let Some(ms) = text.strip_suffix("ms") {
        return decimal_ms(ms, 1);
    }
    if let Some(seconds) = text.strip_suffix('s') {
        return decimal_ms(seconds, 1000);
    }
    let parts: Vec<_> = text.split(':').collect();
    match parts.as_slice() {
        [seconds] => decimal_ms(seconds, 1000),
        [minutes, seconds] => {
            let seconds = decimal_ms(seconds, 1000)?;
            ensure!(seconds < 60_000, "invalid clock seconds");
            add(
                integer(minutes)?
                    .checked_mul(60_000)
                    .context("timestamp overflow")?,
                seconds,
            )
        }
        [hours, minutes, seconds] => {
            let minutes = integer(minutes)?;
            let seconds = decimal_ms(seconds, 1000)?;
            ensure!(
                minutes < 60 && seconds < 60_000,
                "invalid clock minutes/seconds"
            );
            let hours = integer(hours)?
                .checked_mul(3_600_000)
                .context("timestamp overflow")?;
            add(add(hours, minutes * 60_000)?, seconds)
        }
        _ => bail!("unsupported TTML clock: {text:?}"),
    }
}

#[derive(Clone, Copy, Default)]
struct Bounds {
    start: Option<u64>,
    end: Option<u64>,
}

fn element_bounds(node: Node<'_, '_>, parent: Bounds) -> Result<Bounds> {
    if let Some(value) = node.attribute("timeContainer") {
        ensure!(value == "par", "sequential TTML timing is unsupported");
    }
    let begin = node.attribute("begin").map(ttml_time).transpose()?;
    let mut end = node.attribute("end").map(ttml_time).transpose()?;
    let start = begin.or(parent.start);
    if let Some(duration) = node.attribute("dur") {
        let duration = ttml_time(duration)?;
        let until = add(start.unwrap_or(0), duration)?;
        if let Some(end) = end {
            ensure!(end == until, "conflicting TTML end/dur");
        }
        end = Some(until);
    }
    if let (Some(start), Some(end)) = (start, end) {
        ensure!(end >= start, "TTML element ends before it starts");
    }
    if let (Some(begin), Some(bound)) = (begin, parent.start) {
        ensure!(begin >= bound, "TTML child starts before its parent");
    }
    if let Some(bound) = parent.end {
        ensure!(
            begin.is_none_or(|begin| begin <= bound) && end.is_none_or(|end| end <= bound),
            "TTML child exceeds parent end"
        );
    }
    Ok(Bounds {
        start,
        end: end.or(parent.end),
    })
}

fn auxiliary(node: Node<'_, '_>) -> bool {
    matches!(
        node.tag_name().name(),
        "head"
            | "metadata"
            | "translation"
            | "translations"
            | "transliteration"
            | "transliterations"
    ) || node.attributes().filter(|a| a.name() == "role").any(|a| {
        a.value().split_ascii_whitespace().any(|role| {
            matches!(
                role.to_ascii_lowercase().as_str(),
                "x-translation"
                    | "translation"
                    | "x-roman"
                    | "x-romanization"
                    | "romanization"
                    | "x-transliteration"
                    | "transliteration"
                    | "x-bg"
                    | "x-background"
                    | "background"
            )
        })
    })
}

fn space_mode(node: Node<'_, '_>, inherited: bool) -> Result<bool> {
    match node.attribute((XML_NS, "space")) {
        None => Ok(inherited),
        Some("preserve") => Ok(true),
        Some("default") => Ok(false),
        Some(value) => bail!("invalid xml:space: {value:?}"),
    }
}

/// `tts:ruby` 的取值（container / base / text / textContainer / delimiter …）。
fn ruby_role<'a>(node: Node<'a, '_>) -> Option<&'a str> {
    node.attributes().find(|a| a.name() == "ruby").map(|a| a.value())
}

/// 一个 `tts:ruby="container"`：正文只要 base，注音（text / textContainer / delimiter）不进正文。
/// 时间按 container 自己的 → base 自己的 → 注音上的（最早的开始、最晚的结束）取，
/// 让带振假名的字照样有逐字时间。
fn ruby_container(
    node: Node<'_, '_>,
    bounds: Bounds,
    active: Option<usize>,
    preserve: bool,
    builder: &mut TextBuilder,
) -> Result<()> {
    let preserve = space_mode(node, preserve)?;
    let own = element_bounds(node, bounds)?;
    let own_end = node.attribute("end").is_some() || node.attribute("dur").is_some();
    let mut start: Option<u64> = None;
    let mut end: Option<u64> = None;
    let mut every_end = true;
    for reading in node
        .descendants()
        .filter(|n| n.is_element() && ruby_role(*n) == Some("text"))
    {
        let reading_bounds = element_bounds(reading, own)?;
        if reading.attribute("begin").is_some() {
            let at = reading_bounds.start.context("missing ruby text begin")?;
            start = Some(start.map_or(at, |s| s.min(at)));
        }
        if reading.attribute("end").is_some() || reading.attribute("dur").is_some() {
            if let Some(at) = reading_bounds.end {
                end = Some(end.map_or(at, |e| e.max(at)));
            }
        } else {
            every_end = false;
        }
    }
    let active = if node.attribute("begin").is_some() {
        Some(builder.clock(Clock {
            start: own.start.context("missing ruby container begin")?,
            end: own.end.filter(|_| own_end),
        }))
    } else if let Some(start) = start {
        Some(builder.clock(Clock {
            start,
            end: end.filter(|&e| every_end && e >= start),
        }))
    } else {
        active
    };
    // 注音：所有 `tts:ruby="text"` 的字连起来，标在 base 那几个字上
    let reading: String = node
        .descendants()
        .filter(|n| n.is_element() && ruby_role(*n) == Some("text"))
        .flat_map(|n| n.descendants().filter(|d| d.is_text()).map(|d| d.text().unwrap_or_default()))
        .collect();
    let from = builder.glyphs.len();
    ttml_words(node, own, active, preserve, builder)?;
    let to = builder.glyphs.len();
    if to > from {
        builder.rubies.push((from, to, reading));
    }
    Ok(())
}

fn lyric_element(node: Node<'_, '_>) -> bool {
    node.tag_name()
        .namespace()
        .is_none_or(|ns| ns.is_empty() || ns == TTML_NS)
}

fn ttml_words(
    node: Node<'_, '_>,
    bounds: Bounds,
    active: Option<usize>,
    preserve: bool,
    builder: &mut TextBuilder,
) -> Result<()> {
    for child in node.children() {
        if child.is_text() {
            builder.xml(child.text().unwrap_or_default(), active, preserve);
        } else if child.is_element() && !auxiliary(child) {
            ensure!(
                lyric_element(child),
                "foreign XML element in main TTML text"
            );
            ensure!(
                child.tag_name().name() == "span",
                "unsupported element in TTML lyric line: {}",
                child.tag_name().name()
            );
            match ruby_role(child) {
                Some("container") => {
                    ruby_container(child, bounds, active, preserve, builder)?;
                    continue;
                }
                // 注音、注音外壳、括注音用的括号：不是正文
                Some("text" | "textContainer" | "delimiter") => continue,
                _ => {}
            }
            let preserve = space_mode(child, preserve)?;
            // AMLL permits standalone space spans at 00:00 even inside a later
            // line. They carry separators, not sung words or useful clocks.
            let only_space = child.descendants().filter(|n| n.is_text()).all(|n| {
                n.text()
                    .unwrap_or_default()
                    .chars()
                    .all(char::is_whitespace)
            });
            let child_bounds = if only_space {
                element_bounds(child, Bounds::default())?
            } else {
                element_bounds(child, bounds)?
            };
            let own_end = child.attribute("end").is_some() || child.attribute("dur").is_some();
            let active = if child.attribute("begin").is_some() {
                Some(builder.clock(Clock {
                    start: child_bounds.start.context("missing span begin")?,
                    end: child_bounds.end.filter(|_| own_end),
                }))
            } else if own_end && active.is_some() {
                // An inner styled span can inherit a timed span's begin but
                // provide its own end. Keep that explicit end; a p's begin
                // alone still cannot manufacture word timing.
                Some(builder.clock(Clock {
                    start: child_bounds.start.context("missing inherited span begin")?,
                    end: child_bounds.end,
                }))
            } else {
                active
            };
            ttml_words(child, child_bounds, active, preserve, builder)?;
        }
    }
    Ok(())
}

fn ttml_container(
    node: Node<'_, '_>,
    parent: Bounds,
    preserve: bool,
    lines: &mut Vec<(TimedLine, Option<String>)>,
) -> Result<()> {
    if auxiliary(node) {
        return Ok(());
    }
    ensure!(
        lyric_element(node),
        "foreign element in TTML body namespace: {}",
        node.tag_name().namespace().unwrap_or("none")
    );
    let bounds = element_bounds(node, parent)?;
    let preserve = space_mode(node, preserve)?;
    if node.tag_name().name() == "p" {
        let mut builder = TextBuilder::default();
        ttml_words(node, bounds, None, preserve, &mut builder)?;
        let (text, words, rubies) = builder.finish_with_rubies(true)?;
        if text.chars().all(char::is_whitespace) {
            return Ok(());
        }
        let start_ms = node
            .attribute("begin")
            .map(ttml_time)
            .transpose()?
            .or_else(|| words.iter().map(|w| w.start_ms).min())
            .or(bounds.start)
            .context("TTML line has no actual start")?;
        let end_ms = if node.attribute("end").is_some() || node.attribute("dur").is_some() {
            bounds.end
        } else if words.iter().all(|w| w.end_ms.is_some()) {
            words.iter().filter_map(|w| w.end_ms).max()
        } else {
            None
        };
        // Apple Music 的 `itunes:key="L1"`：head 里的译文按它对到行上
        let key = node
            .attributes()
            .find(|a| a.name() == "key")
            .map(|a| a.value().to_string());
        lines.push((
            TimedLine {
                start_ms,
                end_ms,
                text,
                words,
                rubies,
            },
            key,
        ));
    } else {
        ensure!(
            matches!(node.tag_name().name(), "tt" | "body" | "div"),
            "unsupported TTML container"
        );
        ensure!(
            node.children().filter(|n| n.is_text()).all(|n| n
                .text()
                .unwrap_or_default()
                .chars()
                .all(char::is_whitespace)),
            "main TTML text outside a lyric line"
        );
        for child in node.children().filter(|n| n.is_element()) {
            ttml_container(child, bounds, preserve, lines)?;
        }
    }
    Ok(())
}

pub fn parse_ttml(text: &str) -> Result<TimedLyrics> {
    parse_ttml_with_translations(text).map(|(lyrics, _)| lyrics)
}

/// Apple Music TTML 的 `<iTunesMetadata><translations>`：`<text for="L1">` 按 key 收起来。
/// 罗马音（transliterations）不要。同一个 key 有几种语言就有几条。
fn itunes_translations(doc: &Document<'_>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for translation in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "translation")
    {
        for item in translation
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "text")
        {
            let Some(key) = item.attribute("for") else { continue };
            let joined: String = item
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect();
            let text = joined.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                out.push((key.to_string(), text));
            }
        }
    }
    out
}

/// [`parse_ttml`]，另外带上 head 里按 `itunes:key` 对到行上的译文（只有文字和行时间，没有逐字）。
pub fn parse_ttml_with_translations(text: &str) -> Result<(TimedLyrics, Vec<TimedLine>)> {
    let doc = safe_xml(text)?;
    let root = doc.root_element();
    ensure!(
        root.tag_name().name() == "tt" && lyric_element(root),
        "expected TTML tt root"
    );
    for attr in root.attributes().filter(|a| a.name() == "timeBase") {
        ensure!(attr.value() == "media", "unsupported TTML time base");
    }
    ensure!(
        root.children()
            .filter(|n| n.is_element() && n.tag_name().name() == "body")
            .count()
            == 1,
        "TTML requires exactly one body"
    );
    let mut keyed = Vec::new();
    ttml_container(root, Bounds::default(), false, &mut keyed)?;
    let wanted = itunes_translations(&doc);
    let mut translations = Vec::new();
    if !wanted.is_empty() {
        for (line, key) in &keyed {
            let Some(key) = key else { continue };
            for (_, text) in wanted.iter().filter(|(k, _)| k == key) {
                translations.push(TimedLine {
                    start_ms: line.start_ms,
                    end_ms: line.end_ms,
                    text: text.clone(),
                    words: Vec::new(),
                    rubies: Vec::new(),
                });
            }
        }
        translations.sort_by_key(|line| line.start_ms);
    }
    let lyrics = complete(keyed.into_iter().map(|(line, _)| line).collect(), 0)?;
    Ok((lyrics, translations))
}

#[cfg(test)]
mod tests {
    use super::*;

    // All fixtures are invented Japanese phrases, not downloaded lyrics.
    fn ttml(body: &str) -> String {
        format!(
            r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:m="http://www.w3.org/ns/ttml#metadata"><body>{body}</body></tt>"#
        )
    }

    fn qrc(content: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?><QrcInfos><QrcHeadInfo Version="100"/><LyricInfo LyricCount="1"><Lyric_1 LyricType="1" LyricContent="{content}"/></LyricInfo></QrcInfos>"#
        )
    }

    fn word(start: usize, end: usize, start_ms: u64, end_ms: Option<u64>) -> TimedWord {
        TimedWord {
            start,
            end,
            start_ms,
            end_ms,
        }
    }

    #[test]
    fn qrc_with_a_bare_ampersand_in_the_lyrics_still_parses() {
        let xml = qrc("[1000,2000]A(1000,500)&(1500,500)B(2000,1000)\n[4000,1000]&amp;(4000,1000)");
        let lyrics = parse_qrc(&xml).unwrap();
        assert_eq!(lyrics.lines[0].text, "A&B");
        assert_eq!(lyrics.lines[1].text, "&", "an escaped entity is decoded once, not twice");
        assert_eq!(escape_bare_ampersands("a &amp; b &#38; &#x26; c"), "a &amp; b &#38; &#x26; c");
        assert_eq!(escape_bare_ampersands("A & B &foo;"), "A &amp; B &amp;foo;");
    }

    #[test]
    fn qrc_with_a_bare_quote_or_angle_bracket_in_the_lyrics_still_parses() {
        let xml = qrc("[1000,2000]\"Stay\"(1000,1000)<3(2000,1000)");
        let lyrics = parse_qrc(&xml).unwrap();
        assert_eq!(lyrics.lines[0].text, "\"Stay\"<3");
    }

    #[test]
    fn yrc_absolute_clocks_unicode_multiline_and_metadata() {
        let lyrics = parse_yrc(concat!(
            "\u{feff}[ar:架空の歌い手]\n",
            "{\"t\":0,\"c\":[{\"tx\":\"作詞: 架空\"}]}\n",
            "[4000,1000](4000,300,0)空(4500,400,0)へ\n",
            "[1000,2000](1000,400,0)𠮷😀(1600,600,0)か\u{3099}光\n",
        ))
        .unwrap();
        assert_eq!(lyrics.lines.len(), 2);
        let line = &lyrics.lines[0];
        assert_eq!(line.text, "𠮷😀か\u{3099}光");
        assert_eq!(line.start_ms, 1000);
        assert_eq!(line.end_ms, Some(3000));
        assert_eq!(
            line.words,
            [word(0, 2, 1000, Some(1400)), word(2, 5, 1600, Some(2200))]
        );
        assert_eq!(lyrics.lines[1].text, "空へ");
        validate(&lyrics).unwrap();
    }

    #[test]
    fn krc_offsets_are_relative_and_text_ranges_ignore_outer_spaces() {
        let lyrics = parse_krc(concat!(
            "[id:synthetic]\n[offset:100]\n[language:eyJzeW50aGV0aWMiOnRydWV9]\n",
            "[1000,2000]<100,300,0>  月 <700,500,0>が😀  \n",
            "[4000,800]<0,300,0>青<400,300,0>い\n",
        ))
        .unwrap();
        let line = &lyrics.lines[0];
        assert_eq!(line.text, "月 が😀");
        assert_eq!((line.start_ms, line.end_ms), (900, Some(2900)));
        assert_eq!(
            line.words,
            [word(0, 1, 1000, Some(1300)), word(2, 4, 1600, Some(2100))]
        );
        assert_eq!(lyrics.lines[1].words[0].start_ms, 3900);
    }

    #[test]
    fn offsets_shift_every_end_clamp_at_zero_and_use_first_tag() {
        let lyrics =
            parse_yrc("[offset:200]\n[offset:999]\n[100,800](100,300,0)雲(600,200,0)へ").unwrap();
        assert_eq!(
            (lyrics.lines[0].start_ms, lyrics.lines[0].end_ms),
            (0, Some(700))
        );
        assert_eq!(
            lyrics.lines[0].words,
            [word(0, 1, 0, Some(200)), word(1, 2, 400, Some(600))]
        );
        let delayed = parse_krc("[offset:-50]\n[100,800]<0,300,0>雲<500,200,0>へ").unwrap();
        assert_eq!(delayed.lines[0].words[1].end_ms, Some(850));
        assert!(parse_yrc("[offset:NaN]\n[0,1](0,1,0)あ").is_err());
        assert!(
            parse_yrc("[offset:-1]\n[18446744073709551615,0](18446744073709551615,0,0)あ").is_err()
        );
    }

    #[test]
    fn qrc_postfix_clocks_xml_escapes_and_newline_entities() {
        let lyrics = parse_qrc(&qrc(concat!(
            "[ti:架空 &amp; 題名]&#10;[offset:100]&#10;",
            "[1000,2000]君&amp;𠮷(1100,400)が&lt;光&gt;(1600,500)&#10;",
            "[4000,1000]&quot;空&quot;(4000,200)へ(4500,400)",
        )))
        .unwrap();
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[0].text, "君&𠮷が<光>");
        assert_eq!(
            lyrics.lines[0].words,
            [word(0, 3, 1000, Some(1400)), word(3, 7, 1500, Some(2000))]
        );
        assert_eq!(lyrics.lines[1].text, "\"空\"へ");
    }

    #[test]
    fn qrc_literal_attribute_newlines_normalize_but_do_not_merge_rows() {
        let lyrics = parse_qrc(&qrc(
            "[ar:架空]\n[1000,1000]星(1000,400)へ(1500,300)\n[3000,1000]夢(3000,400)を(3500,300)\n",
        ))
        .unwrap();
        assert_eq!(
            lyrics
                .lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            ["星へ", "夢を"]
        );
        let once = parse_qrc(&qrc("[0,1000]&amp;amp;(0,400)だ(500,400)")).unwrap();
        assert_eq!(once.lines[0].text, "&amp;だ");
    }

    #[test]
    fn qrc_requires_unambiguous_content_and_complete_word_times() {
        for xml in [
            "<QrcInfos/>",
            "<QrcInfos><a LyricContent='[0,1000]君(0,400)'/><b LyricContent='[0,1000]夢(0,400)'/></QrcInfos>",
        ] {
            assert!(parse_qrc(xml).is_err(), "{xml}");
        }
        for body in [
            "[0,1000]君へ",
            "[0,1000]君(0,400)へ",
            "[0,1000](0,400)",
            "[0,1000]君(0,-1)",
            "[0,1000]君(0,400,0)",
        ] {
            assert!(parse_qrc(&qrc(body)).is_err(), "{body}");
        }
    }

    #[test]
    fn ttml_contiguous_japanese_ignores_auxiliary_subtrees_and_head() {
        let xml = r#"<tt xmlns="http://www.w3.org/ns/ttml"
              xmlns:m="http://www.w3.org/ns/ttml#metadata"
              xmlns:a="http://itunes.apple.com/lyric-ttml-extensions" a:timing="Word">
            <head><metadata><translations><translation><text for="L1">架空の訳</text></translation></translations></metadata></head>
            <body dur="00:10.000"><div begin="1s" end="9s">
                <p begin="00:01.000" end="00:03.000" a:key="L1">
                    <span begin="00:01.000" end="00:01.400">青𠮷</span>
                    <span begin="00:01.700" end="00:02.600">い空</span>
                    <span m:role="x-translation" xml:lang="zh">蓝天</span>
                    <span m:role="x-roman">aoi sora</span>
                    <span m:role="x-bg"><span begin="bad" end="bad">背景</span></span>
                </p>
                <p m:role="x-bg" begin="4s" end="5s">別の背景</p>
                <div m:role="x-translation"><p begin="4s" end="5s">別の訳</p></div>
                <p begin="4s" end="5s"><span begin="4s" end="4.3s">歩</span><span begin="4.5s" end="5s">く</span></p>
            </div></body></tt>"#;
        let lyrics = parse_ttml(xml).unwrap();
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[0].text, "青𠮷い空");
        assert_eq!(
            lyrics.lines[0].words,
            [word(0, 2, 1000, Some(1400)), word(2, 4, 1700, Some(2600))]
        );
        assert_eq!(lyrics.lines[1].text, "歩く");
    }

    #[test]
    fn ttml_ruby_keeps_only_the_base_and_takes_timing_from_container_or_readings() {
        let doc = |body: &str| {
            format!(
                r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:tts="http://www.w3.org/ns/ttml#styling"><body><p begin="1s" end="5s">{body}</p></body></tt>"#
            )
        };
        // 时间在容器上；括号（delimiter）和注音都不进正文
        let lyrics = parse_ttml(&doc(concat!(
            "<span tts:ruby='container' begin='1s' end='2s'><span tts:ruby='base'>朝</span>",
            "<span tts:ruby='delimiter'>(</span><span tts:ruby='textContainer'><span tts:ruby='text'>あさ</span></span>",
            "<span tts:ruby='delimiter'>)</span></span><span begin='2s' end='3s'>だ</span>"
        )))
        .unwrap();
        assert_eq!(lyrics.lines[0].text, "朝だ");
        assert_eq!(lyrics.lines[0].words, vec![word(0, 1, 1000, Some(2000)), word(1, 2, 2000, Some(3000))]);
        // 时间在注音上：最早的开始到最晚的结束
        let lyrics = parse_ttml(&doc(concat!(
            "<span tts:ruby='container'><span tts:ruby='base'>光</span><span tts:ruby='textContainer'>",
            "<span tts:ruby='text' begin='1.2s' end='1.5s'>ひか</span><span tts:ruby='text' begin='1.5s' end='1.9s'>り</span>",
            "</span></span>"
        )))
        .unwrap();
        assert_eq!(lyrics.lines[0].text, "光");
        assert_eq!(lyrics.lines[0].words, vec![word(0, 1, 1200, Some(1900))]);
        // 哪里都没时间：和别的只有逐行的 TTML 一样，不算逐字歌词
        assert!(parse_ttml(&doc(
            "<span tts:ruby='container'><span tts:ruby='base'>夜</span><span tts:ruby='text'>よる</span></span>",
        ))
        .is_err());
        // 同一行别的字有时间、注音没时间：这一行退回逐行，正文照样不带注音
        let lyrics = parse_ttml(
            r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:tts="http://www.w3.org/ns/ttml#styling"><body><p begin="1s" end="2s"><span begin="1s" end="2s">朝</span></p><p begin="3s" end="5s"><span tts:ruby='container'><span tts:ruby='base'>夜</span><span tts:ruby='text'>よる</span></span><span begin="4s" end="5s">だ</span></p></body></tt>"#,
        )
        .unwrap();
        assert_eq!(lyrics.lines[1].text, "夜だ");
        assert!(lyrics.lines[1].words.is_empty());
    }

    #[test]
    fn ttml_xml_space_inherits_preserve_and_default_can_reset_it() {
        let xml = ttml(concat!(
            "<div xml:space='preserve'><p begin='1s' end='3s'>  ",
            "<span begin='1s' end='1.4s'>月</span> \t",
            "<span begin='2s' end='2.5s'>光</span>  </p>",
            "<p xml:space='default' begin='4s' end='6s'>\n    ",
            "<span begin='4s' end='4.5s'>夜</span>  ",
            "<span begin='5s' end='5.5s'>へ</span>\n  </p></div>",
        ));
        let lyrics = parse_ttml(&xml).unwrap();
        assert_eq!(lyrics.lines[0].text, "  月 \t光  ");
        assert_eq!(
            lyrics.lines[0].words,
            [word(2, 3, 1000, Some(1400)), word(5, 6, 2000, Some(2500))]
        );
        assert_eq!(lyrics.lines[1].text, "夜 へ");
        assert_eq!(lyrics.lines[1].words[1].start, 2);
    }

    #[test]
    fn ttml_keeps_ideographic_spaces_and_zero_timed_separator_spans() {
        let xml = ttml(
            "<p begin='10s' end='13s'><span begin='10s' end='11s'>風</span><span begin='0s' end='0s'> </span><span begin='12s' end='13s'>　色</span></p>",
        );
        let lyrics = parse_ttml(&xml).unwrap();
        assert_eq!(lyrics.lines[0].text, "風 　色");
        assert_eq!(
            lyrics.lines[0].words,
            [
                word(0, 1, 10_000, Some(11_000)),
                word(3, 4, 12_000, Some(13_000))
            ]
        );
    }

    #[test]
    fn ttml_nested_styled_spans_use_actual_timed_span_not_line_clock() {
        let xml = ttml(
            "<p begin='1s' end='4s'><span begin='1.2s' end='2s'>ひ<span>か</span>り</span><span begin='2.5s' dur='0.5s'><span>へ</span></span></p>",
        );
        let lyrics = parse_ttml(&xml).unwrap();
        assert_eq!(lyrics.lines[0].text, "ひかりへ");
        assert_eq!(
            lyrics.lines[0].words,
            [word(0, 3, 1200, Some(2000)), word(3, 4, 2500, Some(3000))]
        );
        let lyrics = parse_ttml(&ttml("<p begin='1s' end='4s'><span begin='1.2s' end='3s'><span end='2s'>ひかり</span><span begin='2.5s' end='3s'>へ</span></span></p>")).unwrap();
        assert_eq!(
            lyrics.lines[0].words,
            [word(0, 3, 1200, Some(2000)), word(3, 4, 2500, Some(3000))]
        );
        assert!(
            lyrics
                .to_enhanced_lrc()
                .contains("ひかり<00:02.000><00:02.500>")
        );
    }

    #[test]
    fn ttml_missing_ends_remain_unknown_and_missing_line_clocks_derive_from_words() {
        let xml = ttml("<p><span begin='1.2s'>朝</span><span begin='2000ms'>に</span></p>");
        let lyrics = parse_ttml(&xml).unwrap();
        assert_eq!(
            (lyrics.lines[0].start_ms, lyrics.lines[0].end_ms),
            (1200, None)
        );
        assert_eq!(
            lyrics.lines[0].words,
            [word(0, 1, 1200, None), word(1, 2, 2000, None)]
        );
        let known = parse_ttml(&ttml(
            "<p><span begin='1s' end='2s'>朝</span><span begin='2.5s' end='3s'>に</span></p>",
        ))
        .unwrap();
        assert_eq!(
            (known.lines[0].start_ms, known.lines[0].end_ms),
            (1000, Some(3000))
        );
    }

    #[test]
    fn ttml_time_formats_are_integer_safe_and_absolute_in_lyric_dialect() {
        for (value, expected) in [
            ("00:01:02.345", 62_345),
            ("01:02.3", 62_300),
            ("62.34s", 62_340),
            ("62345ms", 62_345),
            ("62.345", 62_345),
            ("0.0009s", 0),
            ("1.9ms", 1),
        ] {
            assert_eq!(ttml_time(value).unwrap(), expected, "{value}");
        }
        let lyrics = parse_ttml(&ttml("<div begin='10s' end='20s'><p begin='12s' end='15s'><span begin='13000ms' end='14s'>朝</span></p></div>")).unwrap();
        assert_eq!(lyrics.lines[0].words[0].start_ms, 13_000);
        for value in [
            "NaN",
            "inf",
            "-1s",
            "+1s",
            "1e3s",
            "1.",
            "1..2",
            "00:60.0",
            "1:60:00",
            "00:00:00:12",
            "1f",
            "3t",
            "18446744073709551615s",
        ] {
            assert!(ttml_time(value).is_err(), "{value}");
        }
    }

    #[test]
    fn an_empty_krc_or_yrc_unit_is_dropped_instead_of_failing_the_song() {
        let lyrics = parse_krc("[1000,2000]<0,300,0>夜<300,200,0><500,500,0>が<1000,0,0>").unwrap();
        assert_eq!(lyrics.lines[0].text, "夜が");
        assert_eq!(lyrics.lines[0].words, [word(0, 1, 1000, Some(1300)), word(1, 2, 1500, Some(2000))]);
        let lyrics = parse_yrc("[1000,2000](1000,300,0)夜(1300,200,0)(1500,500,0)が").unwrap();
        assert_eq!(lyrics.lines[0].text, "夜が");
        validate(&lyrics).unwrap();
    }

    #[test]
    fn all_formats_reject_line_only_or_partially_untimed_main_text() {
        for body in ["[0,1000]朝だ", "[0,1000]朝(500,100,0)だ"] {
            assert!(parse_yrc(body).is_err(), "{body}");
        }
        for body in ["[0,1000]朝だ", "[0,1000]朝<500,100,0>だ"] {
            assert!(parse_krc(body).is_err(), "{body}");
        }
        for body in [
            "<p begin='0s' end='1s'>朝だ</p>",
            "<p begin='0s' end='1s'><span>朝だ</span></p>",
            "<p begin='0s' end='1s'><span end='0.4s'>朝</span><span begin='0.5s' end='1s'>だ</span></p>",
            "<p begin='0s' end='1s'><span begin='0s' end='0.4s'>朝</span>だ</p>",
            "<p begin='0s' end='1s'>朝<span m:role='x-translation' begin='0s' end='1s'>morning</span></p>",
        ] {
            assert!(parse_ttml(&ttml(body)).is_err(), "{body}");
        }
    }

    #[test]
    fn mixed_precision_keeps_actual_units_and_downgrades_only_missing_lines() {
        let lyrics = parse_yrc("[0,1000](0,400,0)朝\n[2000,1000]光へ").unwrap();
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[0].words.len(), 1);
        assert!(lyrics.lines[1].words.is_empty());
        let lyrics=parse_ttml(&ttml("<p begin='0s' end='1s'><span begin='0s' end='1s'>朝</span></p><p begin='2s' end='3s'>夜</p>")).unwrap();
        assert_eq!(lyrics.lines[0].words.len(), 1);
        assert!(lyrics.lines[1].words.is_empty());
        assert!(lyrics.to_enhanced_lrc().contains("[00:02.000]夜"));
    }

    #[test]
    fn zero_duration_krc_markers_never_invent_time_or_discard_other_valid_lines() {
        let timed =
            parse_krc("[1000,1000]<0,300,0>光<300,0,0>、<500,400,0>へ\n[3000,1000]<0,0,0>声\n")
                .unwrap();
        assert_eq!(timed.lines.len(), 2);
        assert_eq!(timed.lines[0].words.len(), 2);
        assert!(timed.lines[1].words.is_empty());
        assert!(parse_krc("[1000,1000]<0,0,0>声").is_err());
    }

    #[test]
    fn qrc_separator_clocks_and_rounded_line_ends_preserve_original_units() {
        let lyrics = parse_qrc(&qrc(
            "[1301,499]光(1301,500) (1801,0)\n[2000,1000]へ(2000,500)",
        ))
        .unwrap();
        assert_eq!(lyrics.lines[0].words.len(), 1);
        assert_eq!(lyrics.lines[0].words[0].end_ms, Some(1801));
        assert_eq!(lyrics.lines[0].end_ms, Some(1801));
        assert_eq!(lyrics.lines[1].words.len(), 1);
        let lyrics = parse_qrc(&qrc("[1000,100]光(1000,500)\n[2000,1000]へ(2000,500)")).unwrap();
        assert!(lyrics.lines[0].words.is_empty());
        assert_eq!(lyrics.lines[1].words.len(), 1);
    }

    #[test]
    fn ttml_export_with_explicit_empty_body_namespace_is_supported() {
        let document = r#"<tt xmlns="http://www.w3.org/ns/ttml"><head/><body xmlns=""><div><p begin="1s" end="2s"><span begin="1s" end="1.4s">光</span><span begin="1.5s" end="2s">へ</span></p></div></body></tt>"#;
        let lyrics = parse_ttml(document).unwrap();
        assert_eq!(lyrics.lines[0].words.len(), 2);
        assert_eq!(lyrics.lines[0].words[1].start_ms, 1500);
    }

    #[test]
    fn ttml_punctuation_separators_do_not_hide_missing_words() {
        let lyrics = parse_ttml(&ttml("<p begin='1s' end='3s'>「<span begin='1s' end='2s'>朝</span>」、<span begin='2s' end='3s'>だ</span>。</p>")).unwrap();
        assert_eq!(lyrics.lines[0].text, "「朝」、だ。");
        assert_eq!(
            lyrics.lines[0].words,
            [word(1, 2, 1000, Some(2000)), word(4, 5, 2000, Some(3000))]
        );
    }

    #[test]
    fn malformed_and_out_of_order_text_clocks_return_errors() {
        for body in [
            "[oops,1000](0,100,0)あ",
            "[0,-1](0,100,0)あ",
            "[0,1000](0,100)あ",
            "[0,1000](0,100,0,0)あ",
            "[0,1000](0,100,0)あ(NaN,100,0)い",
            "[0,1000](0,100,0)あ(inf,100,0)い",
            "[0,1000](0,-100,0)あ",
            "[0,1000](0,100,0あ",
            "[0,1000](500,100,0)あ(200,100,0)い",
            "[1000,1000](900,100,0)あ",
            "[0,1000](900,200,0)あ",
            // 「(0,100,0)(200,100,0)あ」这种空单位现在丢掉而不报错，见
            // an_empty_krc_or_yrc_unit_is_dropped_instead_of_failing_the_song；整行没字仍然报错。
            "[0,1000](0,100,0)",
            "[18446744073709551615,1](0,1,0)あ",
            "[0,1000](18446744073709551616,1,0)あ",
        ] {
            assert!(parse_yrc(body).is_err(), "{body}");
        }
        assert!(parse_krc("[18446744073709551615,0]<1,0,0>あ").is_err());
        assert!(parse_krc("[0,1000]<900,200,0>あ").is_err());
        assert!(parse_krc("[0,1000]<0,400,0>あ<oops,400,0>い").is_err());
        assert!(parse_qrc(&qrc("[0,1000]あ(500,100)い(200,100)")).is_err());
        assert!(parse_qrc(&qrc("[0,1000]あ(0,400)い(NaN,400)")).is_err());
    }

    #[test]
    fn ttml_bad_bounds_and_unsupported_timing_return_errors() {
        for body in [
            "<p begin='2s' end='1s'><span begin='2s' end='3s'>朝</span></p>",
            "<p begin='1s' end='2s'><span begin='1.5s' end='1.4s'>朝</span></p>",
            "<p begin='1s' end='2s'><span begin='0.5s' end='1.4s'>朝</span></p>",
            "<p begin='1s' end='2s'><span begin='1.5s' end='2.1s'>朝</span></p>",
            "<p begin='1s' end='2s'><span begin='1.5s' end='2s' dur='1s'>朝</span></p>",
            "<p begin='0s' end='2s'><span begin='1s' end='2s'>朝</span><span begin='0s' end='1s'>に</span></p>",
            "<div timeContainer='seq'><p><span begin='1s'>朝</span></p></div>",
            "<p><span begin='00:00:00:12'>朝</span></p>",
            "<p xml:space='unexpected'><span begin='1s'>朝</span></p>",
        ] {
            assert!(parse_ttml(&ttml(body)).is_err(), "{body}");
        }
        assert!(
            parse_ttml(
                "<tt xmlns:p='http://www.w3.org/ns/ttml#parameter' p:timeBase='clock'><body/></tt>"
            )
            .is_err()
        );
    }

    #[test]
    fn xml_rejects_dtd_entities_deep_trees_and_malformed_documents() {
        for xml in [
            "<!DOCTYPE tt [<!ENTITY a '朝'>]><tt><body><p><span begin='0s'>&a;</span></p></body></tt>",
            "<!DOCTYPE tt SYSTEM 'file:///never-read'><tt/>",
            "<tt><body>&unknown;</body></tt>",
            "<tt><body><p></body></tt>",
            "<tt><body/></tt><tt/>",
        ] {
            assert!(parse_ttml(xml).is_err(), "{xml}");
        }
        let deep = format!(
            "<tt><body>{}<p><span begin='0s'>朝</span></p>{}</body></tt>",
            "<div>".repeat(70),
            "</div>".repeat(70)
        );
        assert!(parse_ttml(&deep).unwrap_err().to_string().contains("depth"));
        assert!(parse_qrc(&format!("{}{}", "<a>".repeat(70), "</a>".repeat(70))).is_err());
        let too_many = format!(
            "<tt><head>{}</head><body/></tt>",
            "<a/>".repeat(MAX_XML_NODES as usize)
        );
        assert!(parse_ttml(&too_many).is_err());
    }

    #[test]
    fn xml_preflight_handles_cdata_comments_and_quoted_angle_brackets() {
        let xml = ttml(
            "<!-- <!DOCTYPE imaginary> <unclosed --><?test x?><p data='a > b &lt; c'><span begin='0s' end='1s'><![CDATA[<朝>]]></span></p>",
        );
        assert_eq!(parse_ttml(&xml).unwrap().lines[0].text, "<朝>");
        let xml = qrc("[0,1000]&lt;朝&gt;(0,500)");
        assert_eq!(parse_qrc(&xml).unwrap().lines[0].text, "<朝>");
    }

    #[test]
    fn enhanced_lrc_keeps_every_real_end_before_gaps_and_at_unicode_boundaries() {
        let lyrics = parse_yrc("[1000,3000](1100,400,0)𠮷😀(2200,300,0)へ").unwrap();
        assert_eq!(
            lyrics.to_enhanced_lrc(),
            "[00:01.000]<00:01.100>𠮷😀<00:01.500><00:02.200>へ<00:02.500>\n"
        );
        let lyrics = parse_ttml(&ttml("<p begin='1s' end='4s' xml:space='preserve'> <span begin='1s' end='2s'>朝</span>  <span begin='3s'>へ</span> </p>")).unwrap();
        assert_eq!(
            lyrics.to_enhanced_lrc(),
            "[00:01.000] <00:01.000>朝<00:02.000>  <00:03.000>へ \n"
        );
        assert_eq!(lrc_clock(3_723_456), "62:03.456");
    }

    #[test]
    fn public_validate_checks_ranges_coverage_and_end_times_without_panics() {
        let good = parse_yrc("[0,1000](0,400,0)空(500,400,0)へ").unwrap();
        validate(&good).unwrap();
        for changed in [
            word(1, 2, 500, Some(400)),
            word(0, 9, 500, None),
            word(0, 0, 0, None),
            word(1, 2, 500, Some(1001)),
            word(0, 1, 500, None),
        ] {
            let mut invalid = good.clone();
            invalid.lines[0].words[1] = changed;
            assert!(validate(&invalid).is_err(), "{changed:?}");
            let _ = invalid.to_enhanced_lrc();
        }
        let mut missing = good;
        missing.lines[0].words.pop();
        assert!(validate(&missing).is_err());
    }

    #[test]
    fn empty_and_metadata_only_inputs_are_not_word_lyrics() {
        for input in ["", "  \n", "[ar:架空]\n[offset:0]", "[0,1000]"] {
            assert!(parse_yrc(input).is_err());
            assert!(parse_krc(input).is_err());
        }
        assert!(parse_qrc(&qrc("[ar:架空]")).is_err());
        assert!(parse_ttml(&ttml("")).is_err());
        assert!(parse_ttml(&ttml("<p m:role='x-translation'>翻訳だけ</p>")).is_err());
    }

    #[test]
    fn malformed_generated_inputs_never_panic() {
        let atoms = [
            "𠮷", "😀", "\u{3099}", "[", "]", "(", ")", "<", ">", "0", "-1", ",", ":", "&", "\"",
            "\n", " ",
        ];
        let mut state = 7u64;
        for _ in 0..300 {
            let mut input = String::new();
            for _ in 0..40 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                input.push_str(atoms[(state >> 32) as usize % atoms.len()]);
            }
            let _ = parse_yrc(&input);
            let _ = parse_krc(&input);
            let _ = parse_qrc(&input);
            let _ = parse_ttml(&input);
        }
    }
}
