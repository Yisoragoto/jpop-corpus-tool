//! 从 Anki 的 `collection.anki2` 读学习状态。
//!
//! 为什么不走 AnkiConnect：要的是「这个词学到什么程度」，
//! 一个词一次请求的话几千个词就是几千次往返。直接读库一次拿完。
//!
//! **必须先复制再读。** Anki 运行时握着这个库，而且开着 WAL；
//! 直接连上去可能读到写了一半的状态，最坏情况是把用户的复习记录弄坏。
//! 复制的时候 `-wal` 和 `-shm` 也要一起带上，否则快照缺最近的事务。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::model::NOTE_TYPE;

/// Anki 把一条笔记的所有字段用这个字符拼成一列。
const FIELD_SEPARATOR: char = '\u{1f}';

/// 一个词的学习状态。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordStatus {
    /// 牌组里已经有这个词
    pub exists: bool,
    /// 至少复习过一次。**这才是「学过」**——加进去没看过不算。
    pub studied: bool,
    pub note_count: i64,
    pub card_count: i64,
    pub max_reps: i64,
    pub decks: Vec<String>,
}

/// 一次读取的结果。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningState {
    /// 词 → 状态
    pub words: BTreeMap<String, WordStatus>,
    /// 读的是哪个 collection
    pub collection_path: String,
    /// 这个 collection 里 JPOP Corpus 和 Lyrics 两种笔记类型都没有
    pub note_type_missing: bool,
}

impl LearningState {
    pub fn studied_count(&self) -> usize {
        self.words.values().filter(|w| w.studied).count()
    }
}

/// 自动找 `collection.anki2`。
///
/// 找不到不是错误——用户可能还没装 Anki，或者放在别处。
/// 多个 profile 时取**最近改动过**的那个：那多半是在用的。
pub fn find_collection() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Windows: %APPDATA%\Anki2\<profile>\collection.anki2
    if let Some(appdata) = std::env::var_os("APPDATA") {
        collect_profiles(&PathBuf::from(appdata).join("Anki2"), &mut candidates);
    }
    // macOS / Linux
    if let Some(home) = home_dir() {
        collect_profiles(
            &home.join("Library/Application Support/Anki2"),
            &mut candidates,
        );
        collect_profiles(&home.join(".local/share/Anki2"), &mut candidates);
    }
    let local = PathBuf::from("collection.anki2");
    if local.is_file() {
        candidates.push(local);
    }

    candidates.retain(|p| std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false));
    candidates.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    candidates.pop()
}

fn collect_profiles(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let candidate = entry.path().join("collection.anki2");
        if candidate.is_file() {
            out.push(candidate);
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// 复制一份再读。**不要直接连用户的库。**
///
/// 返回临时目录（析构时删掉）和副本路径。
pub(crate) fn snapshot(source: &Path) -> Result<(tempdir::TempDir, PathBuf)> {
    let dir = tempdir::TempDir::new("jpop-anki-state")?;
    let name = source
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "collection.anki2".into());
    let target = dir.path().join(&name);
    std::fs::copy(source, &target)
        .with_context(|| format!("复制 {} 失败", source.display()))?;

    // WAL 和 SHM 一起带上，否则快照缺最近的事务
    for suffix in ["-wal", "-shm"] {
        let sidecar = with_suffix(source, suffix);
        if sidecar.is_file() {
            let _ = std::fs::copy(&sidecar, with_suffix(&target, suffix));
        }
    }
    Ok((dir, target))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// 读学习状态。
///
/// `deck_filter` 非空时只算牌组名里含这个子串的（大小写不敏感）。
pub fn load(collection: Option<&Path>, deck_filter: &str) -> Result<LearningState> {
    let path = match collection {
        Some(p) => p.to_path_buf(),
        None => find_collection().context("找不到 Anki 的 collection.anki2")?,
    };
    let (_guard, snapshot_path) = snapshot(&path)?;

    let conn = Connection::open_with_flags(
        &snapshot_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| format!("打不开 {}", snapshot_path.display()))?;
    register_unicase(&conn)?;

    let mut state = LearningState {
        collection_path: path.display().to_string(),
        ..Default::default()
    };

    // 批量导出做的 JPOP Corpus 卡和一键制卡做的 Lyrics 卡都算「进了 Anki」。
    // 两种卡第一个字段都是 Expression。只认 JPOP Corpus 的话，用户真实的 collection 里
    // （JPOP Corpus 0 张、Lyrics 才有卡）一个词都读不到，「藏起已学过的」形同虚设。
    let model_ids: Vec<i64> = [NOTE_TYPE, crate::lyrics_model::NOTE_TYPE]
        .iter()
        .filter_map(|name| {
            conn.query_row("SELECT id FROM notetypes WHERE name=?1", [name], |r| r.get(0))
                .ok()
        })
        .collect();
    if model_ids.is_empty() {
        state.note_type_missing = true;
        return Ok(state);
    }

    let marks = vec!["?"; model_ids.len()].join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT n.id, n.flds, d.name, COUNT(c.id), COALESCE(MAX(c.reps), 0)
         FROM notes n
         JOIN cards c ON c.nid = n.id
         JOIN decks d ON d.id = c.did
         WHERE n.mid IN ({marks})
         GROUP BY n.id, d.name"
    ))?;
    let filter = deck_filter.trim().to_lowercase();
    let rows = stmt.query_map(rusqlite::params_from_iter(model_ids.iter()), |r| {
        Ok((
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
        ))
    })?;

    for row in rows {
        let (fields, deck, card_count, max_reps) = row?;
        let deck = deck_display_name(&deck);
        if !filter.is_empty() && !deck.to_lowercase().contains(&filter) {
            continue;
        }
        // 第一个字段是 Expression
        let expression = clean_field(fields.split(FIELD_SEPARATOR).next().unwrap_or(""));
        if expression.is_empty() {
            continue;
        }
        let entry = state.words.entry(expression).or_default();
        entry.exists = true;
        entry.note_count += 1;
        entry.card_count += card_count;
        entry.max_reps = entry.max_reps.max(max_reps);
        // 「学过」= 至少复习过一次。加进去没看过不算。
        entry.studied = entry.studied || max_reps > 0;
        if !deck.is_empty() && !entry.decks.contains(&deck) {
            entry.decks.push(deck);
        }
    }
    Ok(state)
}

/// Anki 2.1.28 起，`decks.name` 里的层级分隔符是 `\x1f`，不是 `::`。
/// AnkiConnect 返回的是 `::` 形式，用户在 Anki 界面上看到的也是 `::`。
///
/// **Python 版没做这个转换**——拿真实 collection 对账时才发现：`\x1f` 打印出来是隐形的，
/// 所以「JPOP::ヨルシカ」看着像「JPOPヨルシカ」，而且 `deck_filter` 传 `::` 形式时
/// 永远匹配不上任何子牌组。这是刻意与 Python 不一致的一处。
fn deck_display_name(raw: &str) -> String {
    raw.replace('\u{1f}', "::")
}

/// Anki 的 schema 在某些列上用了自定义排序规则 `unicase`。
/// 不注册的话，只要查询碰到那些列就会报「no such collation sequence」。
pub(crate) fn register_unicase(conn: &Connection) -> Result<()> {
    conn.create_collation("unicase", |a: &str, b: &str| {
        a.to_lowercase().cmp(&b.to_lowercase())
    })?;
    Ok(())
}

/// 字段里存的是 HTML。去标签、解实体，才能和语料里的词对上。
pub fn clean_field(raw: &str) -> String {
    plain_field(raw)
}

/// 字段 HTML → 纯文本。和 Python 的 `clean_field` / `_plain_anki_field` 一致：
/// **先反转义、再去标签**（`<[^>]+>`）。之前是先去标签再反转义，
/// `&lt;b&gt;` 这种转义过的尖括号会被留下来；落单的 `<` 也会把后面的字全吞掉。
pub fn plain_field(raw: &str) -> String {
    static TAG: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
    TAG.replace_all(&unescape_entities(raw), "").trim().to_string()
}

fn unescape_entities(text: &str) -> String {
    // 覆盖 Anki 字段里会出现的那几个就够了
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        // &amp; 必须最后replace，否则 &amp;lt; 会被解成 <
        .replace("&amp;", "&")
}

/// 最小可用的临时目录。只为了让快照在析构时自己清理掉。
pub(crate) mod tempdir {
    use std::path::{Path, PathBuf};

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(prefix: &str) -> std::io::Result<Self> {
            // 进程号 + 单调计数：同一进程里连开两个也不会撞
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path)?;
            Ok(Self(path))
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个和 Anki 同构的最小 collection。
    fn fake_collection(dir: &Path) -> PathBuf {
        let path = dir.join("collection.anki2");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE notetypes (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE notes (id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT);
             CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER, did INTEGER, reps INTEGER);
             CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO notetypes VALUES (1, 'JPOP Corpus'), (2, 'Basic');
             INSERT INTO decks VALUES (10, '日本語'), (20, '日本語' || char(31) || '歌詞'), (30, 'その他');",
        )
        .unwrap();
        drop(conn);
        path
    }

    fn add_note(path: &Path, note_id: i64, expression: &str, deck: i64, reps: &[i64]) {
        let conn = Connection::open(path).unwrap();
        // 字段用 \x1f 拼接，第一个是 Expression
        let fields = format!("{expression}\u{1f}よる\u{1f}晚上");
        conn.execute(
            "INSERT INTO notes VALUES (?1, 1, ?2)",
            rusqlite::params![note_id, fields],
        )
        .unwrap();
        for (i, r) in reps.iter().enumerate() {
            conn.execute(
                "INSERT INTO cards VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![note_id * 100 + i as i64, note_id, deck, r],
            )
            .unwrap();
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jp-anki-learn-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn studied_means_actually_reviewed_not_just_added() {
        // 加进牌组但一次没复习的不算学过——这决定了导出时要不要跳过它
        let dir = tmp("studied");
        let path = fake_collection(&dir);
        add_note(&path, 1, "夜", 10, &[0, 0]);
        add_note(&path, 2, "駆ける", 10, &[3]);

        let state = load(Some(&path), "").unwrap();
        assert!(state.words["夜"].exists);
        assert!(!state.words["夜"].studied, "加了没复习不算学过");
        assert!(state.words["駆ける"].studied);
        assert_eq!(state.studied_count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn html_in_the_expression_field_is_stripped() {
        // Anki 字段存的是 HTML，不清掉就和语料里的词对不上
        assert_eq!(clean_field("<b>夜</b>"), "夜");
        assert_eq!(clean_field("夜&nbsp;"), "夜");
        assert_eq!(clean_field("<div>&amp;</div>"), "&");
        // &amp;lt; 应当解成 &lt; 而不是 <
        assert_eq!(clean_field("&amp;lt;"), "&lt;");
        assert_eq!(clean_field("  <span>  </span> "), "");
    }

    #[test]
    fn cards_and_reps_are_aggregated_across_notes() {
        let dir = tmp("aggregate");
        let path = fake_collection(&dir);
        // 同一个词两条笔记，在两个牌组
        add_note(&path, 1, "夜", 10, &[1, 2]);
        add_note(&path, 2, "夜", 20, &[9]);

        let state = load(Some(&path), "").unwrap();
        let status = &state.words["夜"];
        assert_eq!(status.note_count, 2);
        assert_eq!(status.card_count, 3);
        assert_eq!(status.max_reps, 9);
        assert_eq!(status.decks.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deck_names_come_back_with_the_separator_anki_shows() {
        // Anki 2.1.28 起库里存的是 \x1f，界面和 AnkiConnect 用的是 ::
        assert_eq!(deck_display_name("JPOP\u{1f}ヨルシカ"), "JPOP::ヨルシカ");
        assert_eq!(deck_display_name("JPOP Corpus"), "JPOP Corpus");
        assert_eq!(deck_display_name("a\u{1f}b\u{1f}c"), "a::b::c");
    }

    #[test]
    fn a_sub_deck_reads_back_in_the_form_users_type() {
        let dir = tmp("subdeck");
        let path = fake_collection(&dir);
        add_note(&path, 1, "夜", 20, &[5]);

        let state = load(Some(&path), "").unwrap();
        assert_eq!(state.words["夜"].decks, vec!["日本語::歌詞"]);

        // 用户会照着 Anki 界面上看到的名字填，这个得能匹配上
        let filtered = load(Some(&path), "日本語::歌詞").unwrap();
        assert!(filtered.words.contains_key("夜"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_deck_filter_narrows_the_result() {
        let dir = tmp("filter");
        let path = fake_collection(&dir);
        add_note(&path, 1, "夜", 10, &[5]);
        add_note(&path, 2, "駆ける", 30, &[5]);

        let state = load(Some(&path), "日本語").unwrap();
        assert!(state.words.contains_key("夜"));
        assert!(!state.words.contains_key("駆ける"), "その他 牌组该被滤掉");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_collection_without_our_note_type_says_so() {
        // 「还没导过卡」和「读失败」是两回事，UI 的提示完全不同
        let dir = tmp("nonotetype");
        let path = dir.join("collection.anki2");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE notetypes (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE notes (id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT);
             CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER, did INTEGER, reps INTEGER);
             CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO notetypes VALUES (2, 'Basic');",
        )
        .unwrap();
        drop(conn);

        let state = load(Some(&path), "").unwrap();
        assert!(state.note_type_missing);
        assert!(state.words.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notes_from_other_note_types_are_ignored() {
        let dir = tmp("othertype");
        let path = fake_collection(&dir);
        add_note(&path, 1, "夜", 10, &[5]);
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO notes VALUES (99, 2, '别人的卡\u{1f}x')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO cards VALUES (990, 99, 10, 7)", []).unwrap();
        drop(conn);

        let state = load(Some(&path), "").unwrap();
        assert!(!state.words.contains_key("别人的卡"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lyrics_cards_from_one_click_mining_count_too() {
        let dir = tmp("lyrics");
        let path = fake_collection(&dir);
        add_note(&path, 1, "夜", 10, &[0]);
        let conn = Connection::open(&path).unwrap();
        conn.execute("INSERT INTO notetypes VALUES (3, 'Lyrics')", []).unwrap();
        // Lyrics 的第一个字段也是 Expression
        assert_eq!(crate::lyrics_model::FIELDS[0], "Expression");
        let fields = ["思い出す", "", "おもいだす"].join("\u{1f}");
        conn.execute("INSERT INTO notes VALUES (7, 3, ?1)", [fields]).unwrap();
        conn.execute("INSERT INTO cards VALUES (700, 7, 10, 2)", []).unwrap();
        drop(conn);

        let state = load(Some(&path), "").unwrap();
        assert!(!state.note_type_missing);
        assert!(state.words["思い出す"].studied);
        assert!(!state.words["夜"].studied);
        assert_eq!(state.studied_count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_users_collection_is_never_opened_directly() {
        // 读的是副本。这里验的是「读完之后原库的修改时间没变」——
        // 直接连上去的话 SQLite 会碰它（哪怕只读也可能建 -shm）。
        let dir = tmp("snapshot");
        let path = fake_collection(&dir);
        add_note(&path, 1, "夜", 10, &[1]);
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();

        load(Some(&path), "").unwrap();

        let after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(before, after, "原库被动过了");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_collection_is_an_error_with_the_path() {
        let err = load(Some(Path::new("X:/nope/collection.anki2")), "").unwrap_err();
        assert!(format!("{err:#}").contains("nope"), "{err:#}");
    }
}

#[cfg(test)]
mod plain_field_parity {
    use super::*;

    #[test]
    fn plain_field_unescapes_before_stripping_tags_like_python() {
        assert_eq!(plain_field("<b>夜</b>"), "夜");
        assert_eq!(plain_field("&lt;b&gt;夜&lt;/b&gt;"), "夜");
        // 落单的尖括号不是标签，后面的字不能吞掉
        assert_eq!(plain_field(" a < b "), "a < b");
    }
}
