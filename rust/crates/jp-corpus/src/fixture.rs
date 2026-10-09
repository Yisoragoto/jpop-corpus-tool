//! 一个**小而真形状**的语料库，给测试用。
//!
//! **为什么需要**：`jp-app/tests/commands.rs`（81 个测试）和
//! `jp-corpus/tests/real_corpus.rs`（36 个）都挂在作者本机那个 991MB 的
//! `corpus.db` 上，找不到就整组静默跳过**并算通过**。于是 CI 上这 117 个
//! 测试一行断言都没执行——「测试全绿」在 CI 的含义比在本机弱得多。
//!
//! 这里补的是「**永远跑得了**」的那一半：四首歌、十三行歌词、五十来个 token，
//! 建在内存里或临时目录里，不需要任何外部文件。
//!
//! 前三首是最普通的形状。第四首（`004`）是为了让真库那两套里**不依赖具体数据的断言**
//! 也能在这里跑而加的，每一样都对着一条断言：
//!
//! | 004 里有什么 | 给哪条断言用 |
//! |---|---|
//! | 一行副歌重复了两次 | KWIC 的折叠：折叠后的条数少了，`repeat_count` 加起来还是原来的条数 |
//! | 歌词里有 `%` 和 `_` | 短查询走 LIKE 兜底时，这两个字符不能被当成通配符 |
//! | 标点和符号的 token | 词频表不该把它们算进去 |
//! | 曲目有封面、专辑自己没有 | 专辑封面回落到成员曲目的封面 |
//! | 歌手在 `artists` 表里有照片 | 人物查询要把照片带出来 |
//! | 歌手名是拉丁字母 | 人物检索走折过大小写的 `normalized_name` |
//! | 四种信用齐全（作词、作曲、编曲、演唱） | 信用按角色排序 |
//! | 「夜」在 001 里也有 | 一个词出现在两首歌里，而且在其中一首里不止一次 |
//!
//! 另外有两处是刻意留的「不完整」：`003` 有歌词没分词（见 [`UNTOKENIZED_SONG`]），而且**没有时长**
//! ——还没读过时长的歌，`durationSec` 得是 `null` 而不是缺字段。
//!
//! **它不替代真库那两套。** 内存里造几行假数据能通过的查询，在 209 首歌 /
//! 58,628 个 token 上未必对（GROUP BY 漏字段、JOIN 放大、索引没走上都只在
//! 真数据上暴露）。真库那两套测的是「SQL 在量上对不对」，这里测的是
//! 「这条路通不通」——两件事都要。
//!
//! **歌词和曲目都是编的。** 不往仓库里放任何真实歌词：曲名、歌手、歌词行
//! 全是为测试写的普通句子，只保证形状像（有时间轴、有分词、有参与者、有专辑）。

use anyhow::Result;
use rusqlite::{Connection, params};

/// 曲目数
pub const TRACKS: i64 = 4;
/// 专辑数
pub const ALBUMS: i64 = 3;
/// 歌词行数
pub const LYRIC_LINES: i64 = 13;
/// token 数。001 的四行 21 个 + 002 的三行 15 个 + 004 的四行 23 个；003 一个都没有
pub const TOKENS: i64 = 59;
/// 形状最全的那首——`004`。每样东西是给哪条断言用的见模块开头的表。
pub const RICH_SONG: &str = "004";
/// `004` 的演唱者：拉丁字母的名字，`artists` 表里有照片
pub const ARTIST_WITH_PHOTO: &str = "Akari";
/// 有歌词但一个 token 都没有的歌——`003`。
///
/// 刻意留的：「有的歌开了振假名既查不了词也不显示振假名」就是这个状态，
/// 曲库维护里的「补齐缺失分词」要能把它找出来。
pub const UNTOKENIZED_SONG: &str = "003";

/// 一行歌词：(行号, 秒, 文本, 分词)。分词是 (surface, lemma, UPOS)。
type Line<'a> = (i64, f64, &'a str, &'a [(&'a str, &'a str, &'a str)]);

const SONG_001: &[Line] = &[
    (
        0,
        12.4,
        "夜が明ける前に",
        &[
            ("夜", "夜", "NOUN"),
            ("が", "が", "ADP"),
            ("明ける", "明ける", "VERB"),
            ("前", "前", "NOUN"),
            ("に", "に", "ADP"),
        ],
    ),
    (
        1,
        18.0,
        "街の灯りが消えて",
        &[
            ("街", "街", "NOUN"),
            ("の", "の", "ADP"),
            ("灯り", "灯り", "NOUN"),
            ("が", "が", "ADP"),
            ("消え", "消える", "VERB"),
            ("て", "て", "SCONJ"),
        ],
    ),
    (
        2,
        24.6,
        "君の声を思い出す",
        &[
            ("君", "君", "PRON"),
            ("の", "の", "ADP"),
            ("声", "声", "NOUN"),
            ("を", "を", "ADP"),
            ("思い出す", "思い出す", "VERB"),
        ],
    ),
    (
        3,
        31.2,
        "風が吹いている",
        &[
            ("風", "風", "NOUN"),
            ("が", "が", "ADP"),
            ("吹い", "吹く", "VERB"),
            ("て", "て", "SCONJ"),
            ("いる", "いる", "AUX"),
        ],
    ),
];

const SONG_002: &[Line] = &[
    (
        0,
        9.8,
        "朝の光が差す",
        &[
            ("朝", "朝", "NOUN"),
            ("の", "の", "ADP"),
            ("光", "光", "NOUN"),
            ("が", "が", "ADP"),
            ("差す", "差す", "VERB"),
        ],
    ),
    (
        1,
        15.5,
        "窓を開けて歩く",
        &[
            ("窓", "窓", "NOUN"),
            ("を", "を", "ADP"),
            ("開け", "開ける", "VERB"),
            ("て", "て", "SCONJ"),
            ("歩く", "歩く", "VERB"),
        ],
    ),
    (
        2,
        21.1,
        // 「風」在 001 里也有：`word_in_corpus` 要能跨歌找到它
        "遠くで風が鳴る",
        &[
            ("遠く", "遠く", "NOUN"),
            ("で", "で", "ADP"),
            ("風", "風", "NOUN"),
            ("が", "が", "ADP"),
            ("鳴る", "鳴る", "VERB"),
        ],
    ),
];

/// 003 **有歌词、没有分词**。见 [`UNTOKENIZED_SONG`]。
const SONG_003: &[Line] = &[
    (0, 5.0, "海の底に沈む", &[]),
    (1, 11.3, "青い月を見ている", &[]),
];

/// 004 的形状见模块开头的表。第 1、3 行是同一句副歌。
const SONG_004: &[Line] = &[
    (
        0,
        6.0,
        // 「夜」在 001 里也有：一个词跨两首歌，而且在这首里不止一次
        "雨が降る夜に",
        &[
            ("雨", "雨", "NOUN"),
            ("が", "が", "ADP"),
            ("降る", "降る", "VERB"),
            ("夜", "夜", "NOUN"),
            ("に", "に", "ADP"),
        ],
    ),
    (
        1,
        12.5,
        "夜はまだ長い、",
        &[
            ("夜", "夜", "NOUN"),
            ("は", "は", "ADP"),
            ("まだ", "まだ", "ADV"),
            ("長い", "長い", "ADJ"),
            ("、", "、", "PUNCT"),
        ],
    ),
    (
        2,
        19.0,
        "100%の_夢を見た",
        &[
            ("100", "100", "NUM"),
            ("%", "%", "SYM"),
            ("の", "の", "ADP"),
            ("_", "_", "SYM"),
            ("夢", "夢", "NOUN"),
            ("を", "を", "ADP"),
            ("見", "見る", "VERB"),
            ("た", "た", "AUX"),
        ],
    ),
    (
        3,
        25.5,
        "夜はまだ長い、",
        &[
            ("夜", "夜", "NOUN"),
            ("は", "は", "ADP"),
            ("まだ", "まだ", "ADV"),
            ("長い", "長い", "ADJ"),
            ("、", "、", "PUNCT"),
        ],
    ),
];

/// 把 fixture 数据写进一个**已经建好表**的库。
///
/// 调用方自己负责先 [`crate::Corpus::ensure_schema`]／[`seed_new`]。
pub fn seed(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "INSERT INTO people (id, name, normalized_name, sort_name) VALUES
            (1, '夜明バンド', '夜明バンド', '夜明バンド'),
            (2, '海野 透',    '海野透',     '海野透'),
            (3, 'みなも',     'みなも',     'みなも'),
            (4, '白鳥 奏',    '白鳥奏',     '白鳥奏'),
            (5, 'Akari',      'akari',      'akari');

         INSERT INTO albums (id, title, normalized_title, album_artist, normalized_album_artist, year) VALUES
            (1, '夜明けの記録', '夜明けの記録', '夜明バンド', '夜明バンド', '2019'),
            (2, '海の手紙',     '海の手紙',     'みなも',     'みなも',     '2021'),
            (3, 'Rain Notes',   'rain notes',   'Akari',      'akari',      '2023');

         INSERT INTO songs (id, title, artist, year, album, genre, audio_path, duration_sec, album_id) VALUES
            ('001', '街の灯', '夜明バンド', '2019', '夜明けの記録', 'J-Pop', 'E:/music/001.flac', 262.0, 1),
            ('002', '朝の窓', '夜明バンド', '2019', '夜明けの記録', 'J-Pop', 'E:/music/002.flac', 198.5, 1),
            ('003', '海の底', 'みなも',     '2021', '海の手紙',     'J-Pop', 'E:/music/003.flac', NULL,  2),
            ('004', '雨の歌', 'Akari',      '2023', 'Rain Notes',   'J-Pop', 'E:/music/004.flac', 215.0, 3);

         -- 刮削只写曲目的封面；专辑 3 自己的 artwork_path 是空的，要靠回落才有图
         UPDATE songs SET cover_path = 'E:/music/covers/004.jpg' WHERE id = '004';

         INSERT INTO artists (name, image_path) VALUES ('Akari', 'E:/music/artists/Akari.jpg');

         -- 2 号在 001 和 002 上都挂着作曲：`collaborators` 要能看见这条关系
         INSERT INTO track_credits (song_id, person_id, role, position, source) VALUES
            ('001', 1, 'performer', 0, 'library'),
            ('001', 2, 'composer',  0, 'lrc'),
            ('001', 2, 'lyricist',  0, 'lrc'),
            ('002', 1, 'performer', 0, 'library'),
            ('002', 2, 'composer',  0, 'lrc'),
            ('003', 3, 'performer', 0, 'library'),
            ('003', 4, 'composer',  0, 'lrc'),
            -- 004 四种角色齐全；作曲还是 2 号，所以「作曲家」仍然只有 2 号和 4 号两位
            ('004', 5, 'performer', 0, 'library'),
            ('004', 5, 'lyricist',  0, 'lrc'),
            ('004', 2, 'composer',  0, 'lrc'),
            ('004', 4, 'arranger',  0, 'lrc');

         -- 类型是 `song`：前端发的就是它，收藏列表也只认它。
         -- 这里原来写的是 `track`，于是合成库里「收藏了却不在收藏列表里」，而没有一条测试看得出来
         INSERT INTO favorites (entity_type, entity_id, created_at)
            VALUES ('song', '001', '2026-01-01T00:00:00Z');

         -- 等级有带前缀和不带前缀两种写法，统计表都要认
         INSERT INTO jlpt_cache (lemma, level) VALUES ('夜', 'JLPT-N5'), ('風', 'N4');

         INSERT INTO play_history (song_id, played_at, listened_sec, position_sec, completed, source) VALUES
            ('001', '2026-01-01T10:00:00Z', 260.0, 262.0, 1, 'library'),
            ('001', '2026-01-02T10:00:00Z', 240.0, 250.0, 1, 'library'),
            ('002', '2026-01-02T11:00:00Z',  30.0,  30.0, 0, 'search');",
    )?;

    for (song_id, lines) in [("001", SONG_001), ("002", SONG_002), ("003", SONG_003), ("004", SONG_004)] {
        for (line_idx, time_sec, text, tokens) in lines {
            conn.execute(
                "INSERT INTO utterances (song_id, line_idx, time_sec, text) VALUES (?1,?2,?3,?4)",
                params![song_id, line_idx, time_sec, text],
            )?;
            let utterance_id = conn.last_insert_rowid();
            for (token_idx, (surface, lemma, pos)) in tokens.iter().enumerate() {
                conn.execute(
                    "INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) \
                     VALUES (?1,?2,?3,?4,?5)",
                    params![utterance_id, token_idx as i64, surface, lemma, pos],
                )?;
            }
        }
    }

    // `utterances_fts` 是 content=utterances 的外部内容表，**没有触发器**——
    // 和 `jp_import::import` 里一样，必须自己同步，否则全文检索一条都搜不到。
    conn.execute_batch("INSERT INTO utterances_fts (rowid, text) SELECT id, text FROM utterances;")?;
    Ok(())
}

/// 建表 + 塞数据，一步到位。给内存库和临时目录库共用。
pub fn seed_new(conn: &Connection) -> Result<()> {
    crate::Corpus::ensure_schema_on(conn)?;
    seed(conn)
}

/// 一个装好数据的内存库。
pub fn corpus() -> Result<crate::Corpus> {
    let corpus = crate::Corpus::open_in_memory()?;
    seed_new(corpus.connection())?;
    Ok(corpus)
}

/// 在 `dir` 里建一个装好数据的 `corpus.db`，返回它的路径。
///
/// 要真文件的场景用它：`AppState::new` 认的是「目录里有 corpus.db」。
pub fn write_to(dir: &std::path::Path) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("corpus.db");
    let conn = Connection::open(&path)?;
    // 真库是 WAL 的，这里也要是。否则之后用 `Corpus::open`（只读）打开它时，
    // `tune()` 里那句 `PRAGMA journal_mode=WAL` 会在只读连接上报
    // 「attempt to write a readonly database」——真库碰不到是因为它早就是 WAL 了。
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    seed_new(&conn)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 公开的那几个常量就是被断言用的，所以它们自己也要对得上。
    #[test]
    fn the_advertised_counts_are_what_actually_lands_in_the_database() {
        let corpus = corpus().unwrap();
        let conn = corpus.connection();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };

        assert_eq!(count("SELECT COUNT(*) FROM songs"), TRACKS);
        assert_eq!(count("SELECT COUNT(*) FROM utterances"), LYRIC_LINES);
        assert_eq!(count("SELECT COUNT(*) FROM tokens"), TOKENS);
        assert_eq!(
            count("SELECT COUNT(*) FROM utterances_fts"),
            LYRIC_LINES,
            "外部内容表没同步，全文检索会一条都搜不到"
        );
    }

    /// 刻意留的那首没分词的歌，要真的被「补齐缺失分词」找得到。
    #[test]
    fn exactly_one_song_has_lyrics_but_no_tokens() {
        let corpus = corpus().unwrap();
        let mut stmt = corpus
            .connection()
            .prepare(
                "SELECT s.id FROM songs s
                 WHERE EXISTS (SELECT 1 FROM utterances u WHERE u.song_id = s.id)
                   AND NOT EXISTS (
                       SELECT 1 FROM tokens t
                       JOIN utterances u ON u.id = t.utterance_id
                       WHERE u.song_id = s.id)",
            )
            .unwrap();
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(ids, vec![UNTOKENIZED_SONG.to_string()]);
    }

    /// 形状像不像真库，不靠眼看，靠这几条路都走得通。
    #[test]
    fn the_ordinary_queries_all_return_something_on_it() {
        use crate::{KwicQuery, MatchField};
        let corpus = corpus().unwrap();

        assert_eq!(corpus.tracks(10).unwrap().len(), TRACKS as usize);
        assert_eq!(corpus.overview().unwrap().tracks, TRACKS);
        assert_eq!(corpus.albums(None, 10).unwrap().len(), ALBUMS as usize);
        assert_eq!(corpus.lyrics("001").unwrap().len(), 4);
        assert_eq!(corpus.credits_for_track("001").unwrap().len(), 3);
        assert!(!corpus.search_lyrics("灯り", 10).unwrap().is_empty());
        assert!(corpus.is_favorite("song", "001").unwrap());
        assert_eq!(corpus.favorite_tracks(10).unwrap().len(), 1, "收藏了就该在收藏列表里");
        assert_eq!(corpus.recently_played(10).unwrap().len(), 2);

        // 「風」在两首歌里都出现过——跨歌的那条路也要通
        let hits = corpus
            .kwic(&KwicQuery {
                keywords: vec!["風".into()],
                field: MatchField::Lemma,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 2, "「風」应当在 001 和 002 里各一次");
        let word = corpus.word_in_corpus("風", None, 5).unwrap();
        assert_eq!(word.occurrences, 2);
        assert_eq!(word.song_count, 2);
    }

    /// 同一个库连建两次不该出事（测试里会反复调）。
    #[test]
    fn writing_it_to_a_directory_produces_a_usable_database() {
        let dir = std::env::temp_dir().join(format!("jp-fixture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = write_to(&dir).unwrap();
        {
            let corpus = crate::Corpus::open(&path).unwrap();
            corpus.check_schema().unwrap();
            assert_eq!(corpus.tracks(10).unwrap().len(), TRACKS as usize);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
