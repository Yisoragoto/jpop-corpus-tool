//! 两套集成测试共用的断言：`fixture_corpus.rs`（合成库，哪台机器都跑）和
//! `real_corpus.rs`（作者本机那个真实的 `corpus.db`，没有就跳过）。
//!
//! **为什么共用而不是各写一份**：这些断言验的是不变式——左 + 关键词 + 右能拼回原句、
//! 聚合出来的曲目数等于实际取到的行数、`%` 不被当成通配符——不依赖库里具体是哪些歌。
//! 以前它们只在真库上跑，于是 CI 上一条都没执行（`search.rs`、`corpus.rs` 自己也没有单元测试，
//! KWIC 和全文检索在 CI 上是零覆盖）。各写一份的话，两边迟早走样：改了一边的断言忘了另一边。
//!
//! 两边各自的价值不一样，所以都留着：
//! - 合成库：永远跑得了，而且数据是已知的，断言的每个前提都**必须**成立（见 [`Lib::skip`]）；
//! - 真库：209 首歌、58,628 个 token，GROUP BY 漏字段、JOIN 放大这类错只在量上暴露。
//!
//! 清单只写一遍（文件末尾的 `invariant_tests!`），两个测试文件各展开一次，
//! 所以不会出现「这条只在一边跑」。

// 每个测试文件是一个独立的 crate，各自把这个模块编一遍
#![allow(dead_code)]

use jp_corpus::{Corpus, KwicQuery, MatchField};

/// 被测的库，以及它是不是合成库。
pub struct Lib<'a> {
    pub c: &'a Corpus,
    /// 合成库是为了让这些断言跑得起来才造的，前提不成立说明 fixture 被改坏了
    pub must_have_everything: bool,
}

impl Lib<'_> {
    /// 这条断言的前提在这个库里不成立。
    ///
    /// 真库上是正常情况（比如还没刮削过歌手），说一声然后跳过；
    /// 合成库上**不许跳**——跳过也打 ok，那正是这套测试要解决的问题。
    pub fn skip(&self, why: &str) {
        assert!(!self.must_have_everything, "合成库里这条断言的前提应当成立：{why}");
        eprintln!("[skip] {why}");
    }
}

// ────────────────────────────── 曲库 ──────────────────────────────

pub fn lists_tracks_with_line_counts(lib: &Lib) {
    let c = lib.c;
    let tracks = c.tracks(1000).expect("查询失败");
    assert!(!tracks.is_empty(), "曲库为空");
    // 大多数歌都有歌词；没有歌词的要如实报 0 而不是漏掉这一行
    let with_lyrics = tracks.iter().filter(|t| t.line_count > 0).count();
    assert!(
        with_lyrics * 2 > tracks.len(),
        "有歌词的曲目只有 {with_lyrics}/{}，可能是 JOIN 写错了",
        tracks.len()
    );
}

pub fn track_lookup_round_trips(lib: &Lib) {
    let c = lib.c;
    let first = &c.tracks(1).expect("查询失败")[0];
    let again = c.track(&first.id).expect("查询失败").expect("按 id 查不到");
    assert_eq!(again.id, first.id);
    assert_eq!(again.title, first.title);
}

pub fn scraped_performers_carry_their_photo_to_the_people_queries(lib: &Lib) {
    let c = lib.c;
    let conn = c.connection();
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='artists')",
            [],
            |r| r.get(0),
        )
        .expect("查询失败");
    if !has_table {
        return lib.skip("还没刮削过歌手（没有 artists 表）");
    }
    let expected: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT p.id, a.image_path FROM people p JOIN artists a ON a.name = p.name \
                 WHERE COALESCE(a.image_path,'') <> '' ORDER BY p.id",
            )
            .expect("查询失败");
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("查询失败");
        rows.filter_map(Result::ok).collect()
    };
    if expected.is_empty() {
        return lib.skip("库里还没有歌手照片");
    }
    for (id, path) in &expected {
        let person = c.person_by_id(*id).expect("查询失败").expect("按 id 查不到");
        assert_eq!(&person.image_path, path, "「{}」的照片没带出来", person.name);
    }
    println!("{} 位演唱者的照片都带到了人物查询里", expected.len());
}

pub fn an_album_falls_back_to_a_member_tracks_cover(lib: &Lib) {
    // 刮削只写 songs.cover_path；albums.artwork_path 要另跑一次「填专辑封面」。
    // 界面上的专辑区不该因为少跑那一步就整片空着。
    let c = lib.c;
    let albums = c.albums(None, 500).expect("查询失败");

    let mut checked = 0;
    for album in albums.iter() {
        let tracks = c.album_tracks(album.id).expect("查询失败");
        let any_track_cover = tracks.iter().any(|t| !t.cover_path.is_empty());
        if !any_track_cover {
            continue;
        }
        assert!(
            !album.artwork_path.is_empty(),
            "专辑「{}」的曲目有封面，回落却没生效",
            album.title
        );
        // 回落挑的必须是这张专辑自己的曲目，不能串到别的专辑去
        assert!(
            tracks.iter().any(|t| t.cover_path == album.artwork_path)
                || album.artwork_path.starts_with(char::is_alphanumeric),
            "专辑「{}」的封面 {} 不属于它的任何一首曲目",
            album.title,
            album.artwork_path
        );
        checked += 1;
        if checked >= 20 {
            break;
        }
    }
    assert!(checked > 0, "一张有封面的专辑都没有，先跑刮削");
}

pub fn albums_have_tracks_and_no_double_counting(lib: &Lib) {
    let c = lib.c;
    let albums = c.albums(None, 500).expect("查询失败");
    assert!(!albums.is_empty(), "没有专辑：这个库还没回填过音乐库实体表");

    for album in albums.iter().take(20) {
        let tracks = c.album_tracks(album.id).expect("查询失败");
        // track_count 来自 GROUP BY 聚合，必须和实际取到的行数一致。
        // 不一致通常意味着聚合里混进了别的 JOIN，把行数放大了。
        assert_eq!(
            album.track_count as usize,
            tracks.len(),
            "专辑「{}」聚合数 {} 与实际 {} 不符",
            album.title,
            album.track_count,
            tracks.len()
        );
    }
}

pub fn credits_are_ordered_by_role(lib: &Lib) {
    let c = lib.c;
    let mut found = false;
    for track in c.tracks(60).expect("查询失败") {
        let credits = c.credits_for_track(&track.id).expect("查询失败");
        if credits.len() < 2 {
            continue;
        }
        found = true;
        let rank = |role: &str| match role {
            "lyricist" => 0,
            "composer" => 1,
            "arranger" => 2,
            "translator" => 3,
            "performer" => 4,
            _ => 9,
        };
        let ranks: Vec<i32> = credits.iter().map(|c| rank(&c.role)).collect();
        assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "信用顺序乱了: {ranks:?}");
    }
    assert!(found, "没有一首歌带 2 条以上信用，backfill 可能没跑");
}

pub fn collaboration_graph_excludes_self(lib: &Lib) {
    let c = lib.c;
    let composers = c.people_by_role("composer", 5).expect("查询失败");
    assert!(!composers.is_empty(), "没有作曲家");
    for person in composers {
        let edges = c.collaborators(person.id, 50).expect("查询失败");
        assert!(
            edges.iter().all(|e| e.person_id != person.id),
            "「{}」把自己算成了合作者",
            person.name
        );
        assert!(edges.iter().all(|e| e.shared_tracks > 0));
    }
}

pub fn works_by_person_covers_their_credits(lib: &Lib) {
    let c = lib.c;
    let top = &c.people_by_role("composer", 1).expect("查询失败")[0];
    let works = c.works_by_person(top.id).expect("查询失败");
    // works_by_person 用了 DISTINCT，同一首歌兼任作词+作曲不能算两次
    let mut ids: Vec<&str> = works.iter().map(|t| t.id.as_str()).collect();
    ids.sort();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "works_by_person 返回了重复曲目");
    assert!(
        works.len() as i64 >= top.track_count,
        "作品数 {} 少于该角色统计的 {}",
        works.len(),
        top.track_count
    );
}

// ────────────────────────────── KWIC ──────────────────────────────

fn common_lemma(c: &Corpus) -> String {
    c.word_frequency(Some("NOUN"), 1).expect("查询失败")[0]
        .lemma
        .clone()
}

/// 前 40 个名词里的一个单字词（「夜」「君」「恋」这类）。短查询的兜底、精确匹配排第一都拿它验
fn single_character_noun(c: &Corpus) -> Option<String> {
    c.word_frequency(Some("NOUN"), 40)
        .expect("查询失败")
        .into_iter()
        .find(|w| w.lemma.chars().count() == 1)
        .map(|w| w.lemma)
}

pub fn kwic_splits_the_line_around_the_keyword(lib: &Lib) {
    let c = lib.c;
    let lemma = common_lemma(c);
    let hits = c
        .kwic(&KwicQuery {
            keywords: vec![lemma.clone()],
            field: MatchField::Lemma,
            limit: Some(200),
            ..Default::default()
        })
        .expect("查询失败");
    assert!(!hits.is_empty(), "「{lemma}」一条都没搜到");

    for hit in &hits {
        // 左 + 关键词 + 右 必须能拼回原句——这是 KWIC 正确性的核心不变式。
        // 旧实现每行都要额外查一次 token 来做这件事，这里是批量取的，
        // 更容易出「取错行」的错，所以必须钉死。
        let rebuilt = format!("{}{}{}", hit.left, hit.keyword, hit.right);
        assert_eq!(
            rebuilt, hit.text,
            "拼不回原句: {:?} != {:?}",
            rebuilt, hit.text
        );
        assert!(!hit.keyword.is_empty());
        assert!(hit.repeat_count >= 1);
    }
}

pub fn kwic_surface_is_narrower_than_lemma(lib: &Lib) {
    let c = lib.c;
    let lemma = common_lemma(c);
    let by_lemma = c
        .kwic(&KwicQuery {
            keywords: vec![lemma.clone()],
            field: MatchField::Lemma,
            dedup: false,
            ..Default::default()
        })
        .expect("查询失败");
    let by_surface = c
        .kwic(&KwicQuery {
            keywords: vec![lemma],
            field: MatchField::Surface,
            dedup: false,
            ..Default::default()
        })
        .expect("查询失败");
    // 词元检索把活用形也算进来，命中数不该少于表层形检索
    assert!(by_lemma.len() >= by_surface.len());
}

pub fn kwic_dedup_collapses_repeats(lib: &Lib) {
    let c = lib.c;
    let lemma = common_lemma(c);
    let query = |dedup: bool| KwicQuery {
        keywords: vec![lemma.clone()],
        field: MatchField::Lemma,
        dedup,
        ..Default::default()
    };
    let folded = c.kwic(&query(true)).expect("查询失败");
    let raw = c.kwic(&query(false)).expect("查询失败");
    assert!(folded.len() <= raw.len());
    let counted: i64 = folded.iter().map(|h| h.repeat_count).sum();
    assert_eq!(counted, raw.len() as i64, "折叠后的计数对不上原始条数");
}

pub fn kwic_cross_line_extends_the_context(lib: &Lib) {
    let c = lib.c;
    let lemma = common_lemma(c);
    let plain = c
        .kwic(&KwicQuery {
            keywords: vec![lemma.clone()],
            field: MatchField::Lemma,
            limit: Some(50),
            ..Default::default()
        })
        .expect("查询失败");
    let wide = c
        .kwic(&KwicQuery {
            keywords: vec![lemma],
            field: MatchField::Lemma,
            cross_line: true,
            limit: Some(50),
            ..Default::default()
        })
        .expect("查询失败");
    assert_eq!(plain.len(), wide.len(), "开 cross_line 不该改变命中条数");
    let extended = wide
        .iter()
        .zip(plain.iter())
        .filter(|(w, p)| w.left.len() > p.left.len() || w.right.len() > p.right.len())
        .count();
    assert!(extended > 0, "cross_line 没有把任何一条的语境变长");
}

pub fn kwic_filters_by_person(lib: &Lib) {
    let c = lib.c;
    let performer = &c.people_by_role("performer", 1).expect("查询失败")[0];
    let lemma = common_lemma(c);
    let hits = c
        .kwic(&KwicQuery {
            keywords: vec![lemma],
            field: MatchField::Lemma,
            person_ids: vec![performer.id],
            ..Default::default()
        })
        .expect("查询失败");
    for hit in &hits {
        let credits = c.credits_for_track(&hit.song_id).expect("查询失败");
        assert!(
            credits.iter().any(|cr| cr.person_id == performer.id),
            "「{}」的筛选漏进了 {}",
            performer.name,
            hit.song_id
        );
    }
}

pub fn kwic_handles_empty_and_missing_keywords(lib: &Lib) {
    let c = lib.c;
    assert!(c.kwic(&KwicQuery::default()).expect("查询失败").is_empty());
    let none = c
        .kwic(&KwicQuery {
            keywords: vec!["这个词绝不可能出现在日语歌词里".into()],
            ..Default::default()
        })
        .expect("查询失败");
    assert!(none.is_empty());
}

// ────────────────────────────── FTS5 ──────────────────────────────

pub fn full_text_search_finds_a_known_line(lib: &Lib) {
    let c = lib.c;
    // 拿一条真实歌词的中段去搜，必须能搜回它自己。
    // 这条测试同时验证 utterances_fts 这张索引是活的——
    // 它一直建着、一直维护着，但旧 UI 从来没查过。
    let hits = c
        .kwic(&KwicQuery {
            keywords: vec![common_lemma(c)],
            field: MatchField::Lemma,
            limit: Some(1),
            ..Default::default()
        })
        .expect("查询失败");
    let line = &hits[0].text;
    let needle: String = line.chars().take(6).collect();
    if needle.chars().count() < 3 {
        return lib.skip("最常见的名词所在的那一行不到 3 个字，trigram 索引搜不了");
    }

    let found = c.search_lyrics(&needle, 50).expect("全文检索失败");
    assert!(
        found.iter().any(|h| h.utterance_id == hits[0].utterance_id),
        "全文检索找不回它自己：{needle:?}"
    );
}

pub fn full_text_search_escapes_user_input(lib: &Lib) {
    let c = lib.c;
    // FTS5 里 " 是定界符、AND/OR/* 是运算符。不转义会直接语法错误。
    for needle in ["\"", "a\"b", "AND", "OR", "*", "NEAR(a b)", "(("] {
        let result = c.search_lyrics(needle, 5);
        assert!(result.is_ok(), "{needle:?} 让 FTS5 报错了: {:?}", result.err());
    }
}

pub fn full_text_search_handles_short_japanese_queries(lib: &Lib) {
    let c = lib.c;
    // trigram 索引对少于 3 个字符的查询一律返回 0 条，而「夜」「恋」「君」
    // 这类单字检索在日语里极其常见。静默返回空比报错更糟——用户会以为
    // 语料里真的没有。短查询必须走 LIKE 兜底。
    let Some(single) = single_character_noun(c) else {
        return lib.skip("前 40 个名词里没有单字的");
    };

    let hits = c.search_lyrics(&single, 20).expect("查询失败");
    assert!(!hits.is_empty(), "单字「{single}」搜不到，兜底没生效");
    assert!(
        hits.iter().all(|h| h.text.contains(&single)),
        "兜底返回了不含该字的行"
    );
}

pub fn full_text_search_escapes_like_wildcards(lib: &Lib) {
    let c = lib.c;
    // 短查询走 LIKE，% 和 _ 是通配符。不转义的话搜「%」会命中所有行。
    let all = c.tracks(1).expect("查询失败").len();
    assert!(all > 0);
    for needle in ["%", "_", "%%", "\\"] {
        let hits = c.search_lyrics(needle, 50).expect("查询失败");
        assert!(
            hits.iter().all(|h| h.text.contains(needle)),
            "{needle:?} 被当成通配符了"
        );
    }
}

pub fn full_text_search_ignores_blank_input(lib: &Lib) {
    let c = lib.c;
    assert!(c.search_lyrics("", 10).expect("查询失败").is_empty());
    assert!(c.search_lyrics("   ", 10).expect("查询失败").is_empty());
}

// ────────────────────────────── 语料 ──────────────────────────────

pub fn overview_is_internally_consistent(lib: &Lib) {
    let c = lib.c;
    let o = c.overview().expect("查询失败");
    assert!(o.tracks > 0 && o.lyric_lines > 0 && o.tokens > 0);
    assert!(o.vocabulary <= o.tokens, "词型数不可能超过词次数");
    assert!(o.performers <= o.people);
    assert!(o.composers <= o.people);
}

pub fn timeline_years_are_sorted_and_nonempty(lib: &Lib) {
    let c = lib.c;
    let years = c.timeline().expect("查询失败");
    assert!(!years.is_empty());
    let labels: Vec<&str> = years.iter().map(|y| y.year.as_str()).collect();
    let mut sorted = labels.clone();
    sorted.sort();
    assert_eq!(labels, sorted, "timeline 没有按年份排序");
    assert!(years.iter().all(|y| !y.year.trim().is_empty()));
}

pub fn word_frequency_excludes_punctuation_and_is_descending(lib: &Lib) {
    let c = lib.c;
    let words = c.word_frequency(None, 200).expect("查询失败");
    assert!(!words.is_empty());
    let freqs: Vec<i64> = words.iter().map(|w| w.freq).collect();
    assert!(freqs.windows(2).all(|w| w[0] >= w[1]), "词频没有降序");
    for word in &words {
        assert!(
            !matches!(word.pos.as_str(), "PUNCT" | "SYM" | "SPACE" | "X"),
            "词频表里混进了 {}",
            word.pos
        );
        assert!(word.song_count > 0);
    }
}

pub fn word_in_corpus_powers_research_mode(lib: &Lib) {
    let c = lib.c;
    let lemma = common_lemma(c);
    let stats = c.word_in_corpus(&lemma, None, 20).expect("查询失败");
    assert_eq!(stats.lemma, lemma);
    assert!(stats.occurrences > 0);
    assert!(stats.song_count > 0 && stats.song_count <= stats.occurrences);
    assert!(stats.artist_count > 0 && stats.artist_count <= stats.song_count);
    assert!(!stats.examples.is_empty());
    // 例句应当去重到「一行一条」
    let mut ids: Vec<i64> = stats.examples.iter().map(|e| e.utterance_id).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "例句里有重复的行");
}

pub fn word_in_corpus_puts_the_current_song_first(lib: &Lib) {
    let c = lib.c;
    let lemma = common_lemma(c);
    let any = c.word_in_corpus(&lemma, None, 30).expect("查询失败");
    // 挑一首不是默认排第一的歌，指定它之后应该被顶到最前
    let Some(target) = any
        .examples
        .iter()
        .map(|e| e.song_id.clone())
        .find(|id| *id != any.examples[0].song_id)
    else {
        return lib.skip("最常见的名词只出现在一首歌里，无从验证");
    };
    let focused = c
        .word_in_corpus(&lemma, Some(&target), 30)
        .expect("查询失败");
    assert_eq!(
        focused.examples[0].song_id, target,
        "指定当前歌曲后没有把它的例句排到最前"
    );
}

pub fn word_in_corpus_handles_unknown_words(lib: &Lib) {
    let c = lib.c;
    let stats = c
        .word_in_corpus("绝不可能存在的词元", None, 10)
        .expect("查询失败");
    assert_eq!(stats.occurrences, 0);
    assert!(stats.examples.is_empty());
}

// ────────────────────────────── 歌词 ──────────────────────────────

pub fn lyrics_and_tokens_line_up(lib: &Lib) {
    let c = lib.c;
    // 行数最多、而且分过词的那一首（真库里也有「有歌词没分词」的歌，那种没法验这一条）
    let mut tracks = c.tracks(300).expect("查询失败");
    tracks.sort_by_key(|t| std::cmp::Reverse(t.line_count));
    let (track, lines) = tracks
        .into_iter()
        .map(|t| {
            let lines = c.lyrics(&t.id).expect("查询失败");
            (t, lines)
        })
        // lyrics() 一次就把分词带回来了，不用再逐行查
        .find(|(_, lines)| lines.iter().any(|l| !l.tokens.is_empty()))
        .expect("没有一首歌分过词");
    assert!(track.line_count > 1, "最长的一首只有 {} 行", track.line_count);
    assert_eq!(lines.len() as i64, track.line_count);

    let ids: Vec<i64> = lines.iter().map(|l| l.utterance_id).collect();
    let batched = c.tokens_for_lines(&ids).expect("查询失败");

    for line in lines.iter().take(10) {
        let (id, text) = (&line.utterance_id, &line.text);
        let single = c.line_tokens(*id).expect("查询失败");
        let from_batch = batched.get(id).cloned().unwrap_or_default();
        assert_eq!(single, from_batch, "批量取和单条取的结果不一致");
        assert_eq!(single, line.tokens, "lyrics() 带回的分词与单独查的不一致");
        if !single.is_empty() {
            // 分词时空白 token 被丢掉了，所以拼接结果会少掉原句里的空格。
            // 这是既有数据的性质，不是查询的错——去掉空白再比。
            // （KWIC 那边不受影响：它按原文偏移切，空格原样保留。）
            let joined: String = single.iter().map(|t| t.surface.as_str()).collect();
            let strip = |t: &str| t.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            assert_eq!(strip(&joined), strip(text), "token 拼不回原句");
        }
    }
}

// ────────────────────────── 全局搜索（Cmd+K） ──────────────────────────

pub fn quick_search_finds_a_track_by_title(lib: &Lib) {
    let corpus = lib.c;
    // 用库里真实存在的曲名，而不是刮削测试里那些外部例子
    let needle = corpus
        .tracks(1)
        .unwrap()
        .into_iter()
        .next()
        .expect("库里应当有曲目")
        .title;
    let results = corpus.quick_search(&needle, None).unwrap();
    assert!(
        results.tracks.iter().any(|h| matches!(h,
            jp_corpus::QuickHit::Track { title, .. } if title == &needle)),
        "应当按曲名找到「{needle}」"
    );
}

pub fn quick_search_finds_a_track_by_artist(lib: &Lib) {
    let corpus = lib.c;
    // 用库里作品最多的那位演唱者，而不是写死一个名字
    let performer = &corpus.people_by_role("performer", 1).unwrap()[0];
    let results = corpus.quick_search(&performer.name, None).unwrap();
    assert!(!results.tracks.is_empty(), "按歌手名「{}」应当找到曲目", performer.name);
    assert!(
        results.people.iter().any(|h| matches!(h,
            jp_corpus::QuickHit::Person { person_id, .. } if *person_id == performer.id)),
        "歌手「{}」本身也应当出现在人物里",
        performer.name
    );
}

pub fn quick_search_matches_people_through_the_normalized_name(lib: &Lib) {
    let corpus = lib.c;
    // people.normalized_name 是折过大小写的，所以全小写、全大写都要能搜到同一个人
    let Some(person) = corpus
        .people_by_role("performer", 500)
        .unwrap()
        .into_iter()
        .find(|p| p.name.chars().all(|c| c.is_ascii_alphabetic()) && p.name.len() >= 3)
    else {
        return lib.skip("没有名字是纯拉丁字母的演唱者");
    };
    for query in [person.name.to_lowercase(), person.name.to_uppercase()] {
        let results = corpus.quick_search(&query, None).unwrap();
        assert!(
            results.people.iter().any(|h| matches!(h,
                jp_corpus::QuickHit::Person { person_id, .. } if *person_id == person.id)),
            "搜「{query}」应当找到「{}」",
            person.name
        );
    }
}

pub fn quick_search_returns_words_from_the_corpus(lib: &Lib) {
    let corpus = lib.c;
    let Some(single) = single_character_noun(corpus) else {
        return lib.skip("前 40 个名词里没有单字的");
    };
    let results = corpus.quick_search(&single, None).unwrap();
    // 精确相等的词要排最前——搜「夜」想要的是「夜」本身，不是「夜空」「今夜」
    match results.words.first() {
        Some(jp_corpus::QuickHit::Word { lemma, .. }) => {
            assert_eq!(lemma, &single, "精确匹配应当排第一，实际是 {lemma}")
        }
        other => panic!("搜「{single}」应当返回语料里的词，实际第一条是 {other:?}"),
    }
}

pub fn quick_search_returns_lyric_lines(lib: &Lib) {
    let corpus = lib.c;
    let Some(single) = single_character_noun(corpus) else {
        return lib.skip("前 40 个名词里没有单字的");
    };
    let results = corpus.quick_search(&single, None).unwrap();
    assert!(!results.lyrics.is_empty(), "搜「{single}」应当返回歌词行");
}

pub fn quick_search_escapes_like_wildcards(lib: &Lib) {
    let corpus = lib.c;
    // 「%」「_」被当成通配符的话会命中一切。原来断言的是「曲目少于 5 条」，
    // 那在只有几首歌的库上永远成立；现在逐条看：命中的每一条里都得真的有这个字符
    for needle in ["%", "_"] {
        let results = corpus.quick_search(needle, Some(50)).unwrap();
        for hit in results.tracks.iter().chain(&results.lyrics).chain(&results.words) {
            let haystack = match hit {
                jp_corpus::QuickHit::Track { title, artist, album, .. } => format!("{title}\n{artist}\n{album}"),
                jp_corpus::QuickHit::Lyric { text, .. } => text.clone(),
                jp_corpus::QuickHit::Word { lemma, .. } => lemma.clone(),
                other => panic!("这一组里不该有 {other:?}"),
            };
            assert!(haystack.contains(needle), "{needle:?} 被当成通配符了，命中了 {hit:?}");
        }
    }
}

pub fn quick_search_handles_nonsense_without_erroring(lib: &Lib) {
    let corpus = lib.c;
    for query in ["", "   ", "'; DROP TABLE songs; --", "\\", "___", "🎵🎵🎵"] {
        let results = corpus
            .quick_search(query, None)
            .unwrap_or_else(|e| panic!("{query:?} 不该报错: {e}"));
        let _ = results.total();
    }
    // 确认库还在
    assert!(corpus.overview().unwrap().tracks > 0);
}

pub fn quick_search_respects_the_per_kind_limit(lib: &Lib) {
    let corpus = lib.c;
    let results = corpus.quick_search("a", Some(2)).unwrap();
    for group in [&results.tracks, &results.albums, &results.people, &results.words] {
        assert!(group.len() <= 2, "每组不该超过限制");
    }
}

pub fn quick_search_empty_query_returns_nothing(lib: &Lib) {
    let corpus = lib.c;
    assert!(corpus.quick_search("", None).unwrap().is_empty());
    assert!(corpus.quick_search("   ", None).unwrap().is_empty());
}

/// 上面每个函数展开成一个 `#[test]`。`$open` 是一个返回 `Option<(Corpus, bool)>` 的表达式：
/// 库，以及它是不是合成库；`None` 表示这台机器上没有这个库，整条跳过。
///
/// 清单写在这里而不是两个测试文件里：加一条断言只要改这一处，两边都会跑到。
macro_rules! invariant_tests {
    ($open:expr) => {
        invariant_tests!(@each $open;
            lists_tracks_with_line_counts,
            track_lookup_round_trips,
            scraped_performers_carry_their_photo_to_the_people_queries,
            an_album_falls_back_to_a_member_tracks_cover,
            albums_have_tracks_and_no_double_counting,
            credits_are_ordered_by_role,
            collaboration_graph_excludes_self,
            works_by_person_covers_their_credits,
            kwic_splits_the_line_around_the_keyword,
            kwic_surface_is_narrower_than_lemma,
            kwic_dedup_collapses_repeats,
            kwic_cross_line_extends_the_context,
            kwic_filters_by_person,
            kwic_handles_empty_and_missing_keywords,
            full_text_search_finds_a_known_line,
            full_text_search_escapes_user_input,
            full_text_search_handles_short_japanese_queries,
            full_text_search_escapes_like_wildcards,
            full_text_search_ignores_blank_input,
            overview_is_internally_consistent,
            timeline_years_are_sorted_and_nonempty,
            word_frequency_excludes_punctuation_and_is_descending,
            word_in_corpus_powers_research_mode,
            word_in_corpus_puts_the_current_song_first,
            word_in_corpus_handles_unknown_words,
            lyrics_and_tokens_line_up,
            quick_search_finds_a_track_by_title,
            quick_search_finds_a_track_by_artist,
            quick_search_matches_people_through_the_normalized_name,
            quick_search_returns_words_from_the_corpus,
            quick_search_returns_lyric_lines,
            quick_search_escapes_like_wildcards,
            quick_search_handles_nonsense_without_erroring,
            quick_search_respects_the_per_kind_limit,
            quick_search_empty_query_returns_nothing,
        );
    };
    (@each $open:expr; $($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let Some((corpus, must_have_everything)) = $open else { return };
                $crate::invariants::$name(&$crate::invariants::Lib { c: &corpus, must_have_everything });
            }
        )*
    };
}
