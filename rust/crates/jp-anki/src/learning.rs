//! 从 Anki 的 `collection.anki2` 读学习状态。
//!
//! 为什么不走 AnkiConnect：要的是「这个词学到什么程度」，
//! 一个词一次请求的话几千个词就是几千次往返。直接读库一次拿完。
//!
//! **必须先复制再读。** Anki 运行时以独占模式握着这个库（别的进程连只读都打不开），
//! 而且开着 WAL；复制时 `-wal` 也要带上，否则快照缺最近的事务。
//! 复制怎么防撕裂、怎么缓存，见 [`snapshot`]。

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

/// 一份读得放心的副本。持有它的时候临时目录不会被删。
#[derive(Clone)]
pub(crate) struct Snapshot {
    _dir: std::sync::Arc<tempdir::TempDir>,
    path: PathBuf,
}

impl Snapshot {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

/// 「这份库变没变」的指纹：主文件和 `-wal` 的大小、修改时间，加上主文件头里的
/// 变更计数（偏移 24，回滚日志模式下每个事务加一；修改时间在 Windows 上可能更新得晚）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    main: (u64, Option<std::time::SystemTime>),
    change_counter: [u8; 4],
    wal: Option<(u64, Option<std::time::SystemTime>)>,
}

fn fingerprint(source: &Path) -> Result<Fingerprint> {
    use std::io::Read;
    let stat = |p: &Path| std::fs::metadata(p).map(|m| (m.len(), m.modified().ok()));
    let main = stat(source).with_context(|| format!("读不到 {}", source.display()))?;
    let mut header = [0u8; 28];
    std::fs::File::open(source)
        .and_then(|mut f| f.read_exact(&mut header))
        .with_context(|| format!("读不到 {} 的文件头", source.display()))?;
    let mut change_counter = [0u8; 4];
    change_counter.copy_from_slice(&header[24..28]);
    let wal = stat(&with_suffix(source, "-wal")).ok();
    Ok(Fingerprint { main, change_counter, wal })
}

/// 复制一份再读。**不要直接连用户的库。**
///
/// ## 为什么是复制文件，而不是 SQLite 的 backup / `VACUUM INTO`
///
/// Anki 用 `locking_mode=exclusive` 打开 collection：它开着的时候，别的进程哪怕只读打开
/// 也是「database is locked」（2026-10-04 在用户真实的 Anki 上实测，`-shm` 都不存在）。
/// 而读学习状态的时候 Anki 多半正开着（AnkiConnect 要它开着），所以只能复制文件。
///
/// ## 怎么保证复制出来的不是撕裂的
///
/// 主文件和 `-wal` 分两次复制，中间 Anki 可能做了 checkpoint 或者提交了事务。
/// 所以复制前后各取一次 [`Fingerprint`]，对不上就丢掉重来（见 [`copy_stable`]）。
///
/// ## 缓存
///
/// 用户真实的 collection 75 MB，以前每次 `anki_words` 都整库复制一遍。
/// 现在按指纹缓存：Anki 没写过就直接用上一份。每个 collection 只留一份。
pub(crate) fn snapshot(source: &Path) -> Result<Snapshot> {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};
    static CACHE: LazyLock<Mutex<HashMap<PathBuf, (Fingerprint, Snapshot)>>> =
        LazyLock::new(Default::default);

    // 缓存挂在 static 上，进程退出时不会析构，临时目录删不掉。
    // 所以每个进程第一次用的时候，把以前的进程留下的快照清掉（正被别的进程开着的删不动，跳过）
    static SWEPT: std::sync::Once = std::sync::Once::new();
    SWEPT.call_once(tempdir::sweep_stale);

    let current = fingerprint(source)?;
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((seen, snap)) = cache.get(source)
        && *seen == current
        && snap.path().is_file()
    {
        return Ok(snap.clone());
    }
    let (snap, _) = copy_stable(source, &|| {})?;
    // 记的是复制开始前的指纹：复制期间要是又变了，下次自然会重拍
    cache.insert(source.to_path_buf(), (current, snap.clone()));
    Ok(snap)
}

/// 复制前后指纹一致才算数，最多试这么多次
const COPY_ATTEMPTS: usize = 4;

/// 复制主文件和 `-wal`，前后指纹不一致就重来。返回快照和用了几次。
///
/// `between` 在两次复制之间调用，只给测试模拟「复制途中 Anki 写了库」用。
///
/// 复制完用读写方式打开**副本**做一次 checkpoint，把 `-wal` 并进主文件：
/// 之后只读打开副本不再依赖 `-wal` / `-shm`。`-shm` 不复制——那是 Anki 进程里的
/// 内存索引，拷过来的旧索引反而可能对不上；没有它 SQLite 会从 `-wal` 重建。
fn copy_stable(source: &Path, between: &dyn Fn()) -> Result<(Snapshot, usize)> {
    let name = source
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "collection.anki2".into());
    for attempt in 1..=COPY_ATTEMPTS {
        let before = fingerprint(source)?;
        let dir = tempdir::TempDir::new(tempdir::PREFIX)?;
        let target = dir.path().join(&name);
        std::fs::copy(source, &target)
            .with_context(|| format!("复制 {} 失败", source.display()))?;
        between();
        let wal = with_suffix(source, "-wal");
        if wal.is_file() {
            std::fs::copy(&wal, with_suffix(&target, "-wal"))
                .with_context(|| format!("复制 {} 失败", wal.display()))?;
        }
        if fingerprint(source)? != before {
            // 复制途中变了：这份可能是撕裂的。稍等一下再来，给 Anki 写完的时间
            drop(dir);
            std::thread::sleep(std::time::Duration::from_millis(50 * attempt as u64));
            continue;
        }
        let conn = Connection::open(&target)
            .with_context(|| format!("打不开快照 {}", target.display()))?;
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .with_context(|| format!("快照 {} 合并 WAL 失败", target.display()))?;
        drop(conn);
        return Ok((Snapshot { _dir: std::sync::Arc::new(dir), path: target }, attempt));
    }
    anyhow::bail!(
        "Anki 正在写 {}（连着复制 {COPY_ATTEMPTS} 次，文件都在中途变了），稍后再试",
        source.display()
    )
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
    let snap = snapshot(&path)?;
    let snapshot_path = snap.path();

    let conn = Connection::open_with_flags(
        snapshot_path,
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

    /// 快照目录的前缀：`{PREFIX}-{进程号}-{序号}`
    pub const PREFIX: &str = "jpop-anki-state";

    pub struct TempDir(PathBuf);

    /// 删掉别的进程留下的快照目录。
    ///
    /// 快照缓存在 static 上，进程退出时不析构，目录就留在 %TEMP% 里——一份 75 MB，
    /// 不清的话每启动一次多一份。只动带本前缀、进程号不是自己的；
    /// 另一个还开着的实例正在读的那份，Windows 上删不动，失败就算了。
    pub fn sweep_stale() {
        let own = format!("{PREFIX}-{}-", std::process::id());
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("{PREFIX}-")) && !name.starts_with(&own) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

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

    /// 没变就不再整库复制：用户真实的 collection 75 MB，以前每调一次 anki_words 复制一遍
    #[test]
    fn a_snapshot_is_reused_until_the_collection_changes() {
        let dir = tmp("cache");
        let path = fake_collection(&dir);
        add_note(&path, 1, "夜", 10, &[1]);

        let first = snapshot(&path).unwrap();
        let second = snapshot(&path).unwrap();
        assert_eq!(first.path(), second.path(), "没变却又复制了一份");

        add_note(&path, 2, "朝", 10, &[1]);
        let third = snapshot(&path).unwrap();
        assert_ne!(first.path(), third.path(), "变了却还在用旧快照");
        assert!(load(Some(&path), "").unwrap().words.contains_key("朝"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 复制主文件和 -wal 之间 Anki 写了库：这份快照可能是撕裂的，要丢掉重来
    #[test]
    fn a_copy_taken_while_the_collection_changes_is_retried() {
        let dir = tmp("torn");
        let path = fake_collection(&dir);
        add_note(&path, 1, "夜", 10, &[1]);

        let writes = std::cell::Cell::new(0);
        let (snap, attempts) = copy_stable(&path, &|| {
            // 只在第一次复制的间隙写一次
            if writes.get() == 0 {
                add_note(&path, 2, "朝", 10, &[1]);
            }
            writes.set(writes.get() + 1);
        })
        .unwrap();
        assert_eq!(attempts, 2, "文件在复制中途变了却没有重来");
        let conn = Connection::open(snap.path()).unwrap();
        let notes: i64 = conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0)).unwrap();
        assert_eq!(notes, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 一直在变（Anki 在大量写）：试几次放弃，并说清楚原因，而不是交出一份撕裂的快照
    #[test]
    fn a_collection_that_never_holds_still_is_an_error_not_a_torn_copy() {
        let dir = tmp("busy");
        let path = fake_collection(&dir);
        let n = std::cell::Cell::new(10);
        let err = copy_stable(&path, &|| {
            add_note(&path, n.get(), "夜", 10, &[1]);
            n.set(n.get() + 1);
        })
        .err()
        .expect("文件一直在变却交出了快照");
        assert!(format!("{err}").contains("正在写"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 缓存的快照进程退出时不析构：下一个进程要把它清掉，别在 %TEMP% 里一次攒一份
    #[test]
    fn snapshots_left_by_other_processes_are_swept() {
        let temp = std::env::temp_dir();
        let stale = temp.join(format!("{}-999999999-0", tempdir::PREFIX));
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("collection.anki2"), b"old").unwrap();
        let own = tempdir::TempDir::new(tempdir::PREFIX).unwrap();
        let unrelated = temp.join(format!("jp-anki-learn-{}-unrelated", std::process::id()));
        std::fs::create_dir_all(&unrelated).unwrap();

        tempdir::sweep_stale();
        assert!(!stale.exists(), "别的进程留下的快照没清掉");
        assert!(own.path().exists(), "把自己正在用的删了");
        assert!(unrelated.exists(), "删了不是快照的目录");
        let _ = std::fs::remove_dir_all(&unrelated);
    }

    /// Anki 开着 WAL：最近的事务还在 -wal 里没回写，快照里也要看得到
    #[test]
    fn transactions_still_in_the_wal_are_in_the_snapshot() {
        let dir = tmp("wal");
        let path = fake_collection(&dir);
        let holder = Connection::open(&path).unwrap();
        holder
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        holder
            .execute("INSERT INTO notes VALUES (7, 1, ?1)", ["鳥\u{1f}とり\u{1f}鸟"])
            .unwrap();
        holder.execute("INSERT INTO cards VALUES (700, 7, 10, 2)", []).unwrap();
        assert!(with_suffix(&path, "-wal").metadata().unwrap().len() > 0, "前提：数据还在 WAL 里");

        let state = load(Some(&path), "").unwrap();
        assert!(state.words["鳥"].studied);
        drop(holder);
        let _ = std::fs::remove_dir_all(&dir);
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
