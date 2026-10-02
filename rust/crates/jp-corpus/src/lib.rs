//! JPOP Corpus Tool 的数据层：曲库 · 检索 · 语料 · 播放历史。
//!
//! 这一层不认识 UI，也不认识 Tauri——所有方法都是「进 SQL 出结构体」，
//! 可以离线测试。Tauri command 只是薄薄一层转发。
//!
//! Schema 的事实来源是 `schema.sql`（从迁移完成的真实库里导出来的），
//! 结构版本和前向迁移在 [`migrations`]：`PRAGMA user_version` 由那个模块
//! 统一维护，编号和 Python 侧 `scripts/migrate_db.py` 共用一套，
//! 两边建出来的库才不会互相看成「另一版」。
//!
//! ```no_run
//! # use jp_corpus::{Corpus, KwicQuery, MatchField};
//! let corpus = Corpus::open("corpus.db")?;
//! let hits = corpus.kwic(&KwicQuery {
//!     keywords: vec!["夜".into()],
//!     field: MatchField::Lemma,
//!     ..Default::default()
//! })?;
//! # Ok::<(), anyhow::Error>(())
//! ```

mod corpus;
mod library;
mod models;
mod search;
mod quick;
pub mod corrections;
pub mod fixture;
pub mod migrations;
pub mod stats;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};

pub use models::*;
pub use corrections::{CorrectionView, TokenEdit};

/// 语料库句柄。
///
/// 一个连接一个实例。SQLite 的连接不是线程安全的，多线程场景请每个线程
/// 各开一个（`Corpus::open` 很便宜，真正的开销在页缓存上）。
pub struct Corpus {
    conn: Connection,
}

impl Corpus {
    /// 以只读方式打开。
    ///
    /// 默认只读是有意的：这一层负责查询，写入（导入、刮削、分词）目前
    /// 还在 Python 侧。要写就用 [`Corpus::open_writable`]。
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .with_context(|| format!("打不开数据库 {}", path.display()))?;
        Self::tune(&conn)?;
        Ok(Self { conn })
    }

    /// 可写地打开。目前只有播放历史需要写。
    pub fn open_writable(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let conn = Connection::open(path)
            .with_context(|| format!("打不开数据库 {}", path.display()))?;
        Self::tune(&conn)?;
        Ok(Self { conn })
    }

    /// 内存库，仅测试用。
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Ok(Self { conn })
    }

    fn tune(conn: &Connection) -> Result<()> {
        // WAL 让读写不互相阻塞——迁移期 Python 侧还会同时写这个库
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;")?;
        Ok(())
    }

    /// 底层连接。给还没包装成方法的临时查询用。
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// 可变的底层连接。开事务需要它（`Connection::transaction` 要 `&mut`）。
    ///
    /// 导入走的是 `jp-import`，那一层要自己控制事务边界——一首歌一个事务，
    /// 中途失败只回滚那一首。所以这里把连接借出去，而不是在 `Corpus` 上
    /// 再包一层导入方法。
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// 把库建齐并升到当前结构版本。**空库用得上**：新装的程序第一次启动时，
    /// 数据目录里还什么都没有，`Connection::open` 只会生成一个 0 表的空文件，
    /// 接着 [`Corpus::check_schema`] 就会失败。
    ///
    /// 对已有的库这一步也要跑：老版本建的库可能缺表、缺列，
    /// 由 [`migrations::apply`] 按版本补齐。什么都不缺时是空操作。
    pub fn ensure_schema(&self) -> Result<()> {
        self.migrate().map(|_| ())
    }

    /// 同上，但作用在一个裸连接上。迁移和测试要在自己打开的连接上建表。
    pub fn ensure_schema_on(conn: &Connection) -> Result<()> {
        migrations::apply(conn).map(|_| ())
    }

    /// 同 [`Corpus::ensure_schema`]，但把「这次做了什么」带回来，给日志和诊断用。
    pub fn migrate(&self) -> Result<migrations::Applied> {
        migrations::apply(&self.conn)
    }

    /// 只建表，不碰版本戳。**只给 [`migrations`] 用**——别处要建表请走
    /// [`Corpus::ensure_schema`]，否则库会立起来却没有版本戳。
    ///
    /// `schema.sql` 是从迁移完成的真实库里导出来的（事实来源是库本身，不是手写的副本），
    /// 全是 `IF NOT EXISTS`，对已有的库重复跑不会动任何数据——但**它只补表和索引，
    /// 补不了已有表上缺的列**，那是 `migrations` 里步骤的活儿。
    pub(crate) fn create_tables_on(conn: &Connection) -> Result<()> {
        conn.execute_batch(include_str!("../schema.sql"))
            .context("建表失败")?;
        Ok(())
    }

    /// 这个库是不是已经建齐了。缺表时要给出可操作的提示，
    /// 而不是让调用方撞上一个 "no such table"。
    pub fn check_schema(&self) -> Result<()> {
        for table in ["songs", "utterances", "tokens", "people", "track_credits", "albums"] {
            let exists: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![table],
                |row| row.get(0),
            )?;
            // 不要再提 `python scripts/migrate_db.py`：装了的用户手上只有 jp-app.exe，
            // 没有 Python，也没有仓库。能走到这儿说明建表那步失败了。
            anyhow::ensure!(
                exists > 0,
                "语料库缺少表 `{table}`。这个 corpus.db 可能坏了或者不是本程序的库——\
                 换一个数据目录（设置 → 语料库目录）再试。"
            );
        }
        Ok(())
    }

    // ── 曲库 ──

    pub fn tracks(&self, limit: i64) -> Result<Vec<Track>> {
        library::tracks(&self.conn, limit)
    }

    pub fn track(&self, song_id: &str) -> Result<Option<Track>> {
        library::track(&self.conn, song_id)
    }

    pub fn albums(&self, artist_key: Option<&str>, limit: i64) -> Result<Vec<Album>> {
        library::albums(&self.conn, artist_key, limit)
    }

    pub fn album_tracks(&self, album_id: i64) -> Result<Vec<Track>> {
        library::album_tracks(&self.conn, album_id)
    }

    pub fn find_person(&self, normalized_name: &str) -> Result<Option<Person>> {
        library::find_person(&self.conn, normalized_name)
    }

    pub fn credits_for_track(&self, song_id: &str) -> Result<Vec<Credit>> {
        library::credits_for_track(&self.conn, song_id)
    }

    pub fn people_by_role(&self, role: &str, limit: i64) -> Result<Vec<PersonSummary>> {
        library::people_by_role(&self.conn, role, limit)
    }

    pub fn works_by_person(&self, person_id: i64) -> Result<Vec<Track>> {
        library::works_by_person(&self.conn, person_id)
    }

    pub fn collaborators(&self, person_id: i64, limit: i64) -> Result<Vec<Collaborator>> {
        library::collaborators(&self.conn, person_id, limit)
    }

    /// 一首歌的全部歌词行，带分词。整首歌只发 2 次查询。
    pub fn lyrics(&self, song_id: &str) -> Result<Vec<LyricLine>> {
        library::lyrics(&self.conn, song_id)
    }

    /// 一行歌词的分词。
    pub fn line_tokens(&self, utterance_id: i64) -> Result<Vec<LineToken>> {
        library::line_tokens(&self.conn, utterance_id)
    }

    /// 批量取多行的分词，一次查询。
    pub fn tokens_for_lines(
        &self,
        utterance_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, Vec<LineToken>>> {
        library::tokens_for_lines(&self.conn, utterance_ids)
    }

    // ── 检索 ──

    /// KWIC 检索。无论多少条结果都只发 3 次查询（旧实现是 N+1）。
    pub fn kwic(&self, query: &KwicQuery) -> Result<Vec<KwicHit>> {
        search::kwic(&self.conn, query)
    }

    /// 歌词全文检索，走 FTS5 的 trigram 索引。
    pub fn search_lyrics(&self, text: &str, limit: i64) -> Result<Vec<LyricHit>> {
        search::search_lyrics(&self.conn, text, limit)
    }

    // ── 语料 ──

    pub fn overview(&self) -> Result<Overview> {
        corpus::overview(&self.conn)
    }

    pub fn timeline(&self) -> Result<Vec<YearStats>> {
        corpus::timeline(&self.conn)
    }

    pub fn word_frequency(&self, pos: Option<&str>, limit: i64) -> Result<Vec<WordFrequency>> {
        corpus::word_frequency(&self.conn, pos, limit)
    }

    /// Research Mode 的核心：一个词在整个语料里的样子。
    pub fn word_in_corpus(
        &self,
        lemma: &str,
        current_song_id: Option<&str>,
        example_limit: i64,
    ) -> Result<WordInCorpus> {
        corpus::word_in_corpus(&self.conn, lemma, current_song_id, example_limit)
    }

    // ── 播放历史 ──

    pub fn record_play(&self, event: &PlayEvent) -> Result<()> {
        corpus::record_play(&self.conn, event)
    }

    pub fn recently_played(&self, limit: i64) -> Result<Vec<PlayedTrack>> {
        corpus::recently_played(&self.conn, limit)
    }

    pub fn most_played(&self, min_listened_sec: f64, limit: i64) -> Result<Vec<PlayedTrack>> {
        corpus::most_played(&self.conn, min_listened_sec, limit)
    }

    // ── 时长回填 ──

    /// 待回填的曲目：(song_id, audio_path)。只返回 `duration_sec` 为空的，
    /// 所以重跑幂等。
    pub fn tracks_missing_duration(&self) -> Result<Vec<(String, String)>> {
        corpus::tracks_missing_duration(&self.conn)
    }

    /// 批量写时长。需要可写连接（`open_writable`）。
    pub fn set_durations(&mut self, rows: &[(String, f64)]) -> Result<usize> {
        corpus::set_durations(&mut self.conn, rows)
    }

    /// 按 id 取人。命令面板定位到具体人物时用。
    pub fn person_by_id(&self, person_id: i64) -> Result<Option<PersonSummary>> {
        library::person_by_id(&self.conn, person_id)
    }

    // ── 收藏 ──

    pub fn is_favorite(&self, entity_type: &str, entity_id: &str) -> Result<bool> {
        library::is_favorite(&self.conn, entity_type, entity_id)
    }

    /// 切换收藏，返回切换后的状态。需要可写连接。
    pub fn toggle_favorite(&self, entity_type: &str, entity_id: &str) -> Result<bool> {
        library::toggle_favorite(&self.conn, entity_type, entity_id)
    }

    pub fn favorite_tracks(&self, limit: i64) -> Result<Vec<Track>> {
        library::favorite_tracks(&self.conn, limit)
    }

    // ── 分词校正 ──

    /// 编辑器打开一行时用。行不存在返回 `None`。
    pub fn token_correction(&self, utterance_id: i64) -> Result<Option<CorrectionView>> {
        corrections::load(&self.conn, utterance_id)
    }

    /// 保存校正：改写 `tokens` 并记下校正，下次检索立即生效。
    pub fn save_token_correction(&self, utterance_id: i64, tokens: &[TokenEdit]) -> Result<()> {
        corrections::save(&self.conn, utterance_id, tokens)
    }

    /// 撤销校正。返回是否真的撤销了（没校正过的行返回 `false`）。
    pub fn revert_token_correction(&self, utterance_id: i64) -> Result<bool> {
        corrections::revert(&self.conn, utterance_id)
    }

    // ── 分面 ──

    /// 全局搜索（Cmd+K）。跨曲目/专辑/人物/词汇/歌词，分组返回。
    pub fn quick_search(&self, query: &str, per_kind: Option<i64>) -> Result<QuickSearchResults> {
        quick::quick_search(&self.conn, query, per_kind)
    }

    pub fn genres(&self, limit: i64) -> Result<Vec<FacetCount>> {
        library::genres(&self.conn, limit)
    }

    pub fn decades(&self, limit: i64) -> Result<Vec<FacetCount>> {
        library::decades(&self.conn, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 新装的程序第一次启动：数据目录里只有一个 0 表的空库。
    /// 建表之后应当能直接用，而不是报「数据库缺少表 songs」。
    #[test]
    fn a_brand_new_database_gets_its_tables() {
        let corpus = Corpus::open_in_memory().unwrap();
        assert!(corpus.check_schema().is_err(), "空库本来就该是缺表的");
        corpus.ensure_schema().unwrap();
        corpus.check_schema().unwrap();
        assert!(corpus.tracks(10).unwrap().is_empty());
        assert_eq!(corpus.overview().unwrap().tracks, 0);
        corpus.ensure_schema().unwrap();
    }

    /// 建完要**戳上版本号**。不戳的话下次打开分不清这是新库还是远古库，
    /// 将来的迁移就无从判断「从哪一版升」。
    #[test]
    fn a_brand_new_database_is_stamped_with_the_current_schema_version() {
        let corpus = Corpus::open_in_memory().unwrap();
        assert_eq!(migrations::version(corpus.connection()).unwrap(), 0);

        let applied = corpus.migrate().unwrap();
        assert!(applied.created);
        assert_eq!(
            migrations::version(corpus.connection()).unwrap(),
            migrations::CURRENT
        );
        assert!(applied.summary().contains("新建"), "{}", applied.summary());
    }
}
