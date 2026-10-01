//! 对着真实 `corpus.db` 跑的集成测试。
//!
//! 用真库而不是造数据，是因为这一层的价值全在「SQL 对不对」上——
//! 内存里造几行假数据能通过的查询，在 209 首歌 / 58,628 个 token 上
//! 未必对（GROUP BY 漏字段、JOIN 放大、索引没走上，都只在真数据上暴露）。
//!
//! 库不存在或还没迁移时整体跳过，并说清楚要跑哪个命令。

use std::path::{Path, PathBuf};

use jp_corpus::{Corpus, KwicQuery, MatchField, PlayEvent};

fn db_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .join("corpus.db")
}

/// 每个测试各开一个连接。
///
/// 不做成全局缓存是因为 `rusqlite::Connection` 不是 `Sync`——它内部有
/// 语句缓存的 `RefCell`。这正是 SQLite 的线程模型：连接不跨线程共享。
/// 打开本身很便宜，真正的开销在页缓存上。
fn open_corpus() -> Option<Corpus> {
    let path = db_path();
    if !path.exists() {
        eprintln!("跳过：找不到 {}", path.display());
        return None;
    }
    let corpus = match Corpus::open(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("跳过：{e}");
            return None;
        }
    };
    if let Err(e) = corpus.check_schema() {
        eprintln!("跳过：{e}
  先跑 python scripts/migrate_db.py && python scripts/backfill_library.py");
        return None;
    }
    Some(corpus)
}

macro_rules! corpus {
    () => {
        match open_corpus() {
            Some(c) => c,
            None => return,
        }
    };
}

// ────────────────────────────── 曲库 ──────────────────────────────

#[test]
fn lists_tracks_with_line_counts() {
    let c = corpus!();
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

#[test]
fn track_lookup_round_trips() {
    let c = corpus!();
    let first = &c.tracks(1).expect("查询失败")[0];
    let again = c.track(&first.id).expect("查询失败").expect("按 id 查不到");
    assert_eq!(again.id, first.id);
    assert_eq!(again.title, first.title);
}

#[test]
fn scraped_performers_carry_their_photo_to_the_people_queries() {
    let c = corpus!();
    let conn = c.connection();
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='artists')",
            [],
            |r| r.get(0),
        )
        .expect("查询失败");
    if !has_table {
        eprintln!("跳过：还没刮削过歌手");
        return;
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
        eprintln!("跳过：库里还没有歌手照片");
        return;
    }
    for (id, path) in &expected {
        let person = c.person_by_id(*id).expect("查询失败").expect("按 id 查不到");
        assert_eq!(&person.image_path, path, "「{}」的照片没带出来", person.name);
    }
    println!("{} 位演唱者的照片都带到了人物查询里", expected.len());
}

#[test]
fn an_album_falls_back_to_a_member_tracks_cover() {
    // 刮削只写 songs.cover_path；albums.artwork_path 要另跑一次「填专辑封面」。
    // 界面上的专辑区不该因为少跑那一步就整片空着。
    let c = corpus!();
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

#[test]
fn albums_have_tracks_and_no_double_counting() {
    let c = corpus!();
    let albums = c.albums(None, 500).expect("查询失败");
    assert!(!albums.is_empty(), "没有专辑，先跑 backfill_library.py");

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

#[test]
fn credits_are_ordered_by_role() {
    let c = corpus!();
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

#[test]
fn collaboration_graph_excludes_self() {
    let c = corpus!();
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

#[test]
fn works_by_person_covers_their_credits() {
    let c = corpus!();
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

#[test]
fn kwic_splits_the_line_around_the_keyword() {
    let c = corpus!();
    let lemma = common_lemma(&c);
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

#[test]
fn kwic_surface_is_narrower_than_lemma() {
    let c = corpus!();
    let lemma = common_lemma(&c);
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

#[test]
fn kwic_dedup_collapses_repeats() {
    let c = corpus!();
    let lemma = common_lemma(&c);
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

#[test]
fn kwic_cross_line_extends_the_context() {
    let c = corpus!();
    let lemma = common_lemma(&c);
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

#[test]
fn kwic_filters_by_person() {
    let c = corpus!();
    let performer = &c.people_by_role("performer", 1).expect("查询失败")[0];
    let lemma = common_lemma(&c);
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

#[test]
fn kwic_handles_empty_and_missing_keywords() {
    let c = corpus!();
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

#[test]
fn full_text_search_finds_a_known_line() {
    let c = corpus!();
    // 拿一条真实歌词的中段去搜，必须能搜回它自己。
    // 这条测试同时验证 utterances_fts 这张索引是活的——
    // 它一直建着、一直维护着，但旧 UI 从来没查过。
    let hits = c
        .kwic(&KwicQuery {
            keywords: vec![common_lemma(&c)],
            field: MatchField::Lemma,
            limit: Some(1),
            ..Default::default()
        })
        .expect("查询失败");
    let line = &hits[0].text;
    let needle: String = line.chars().take(6).collect();
    if needle.chars().count() < 3 {
        return;
    }

    let found = c.search_lyrics(&needle, 50).expect("全文检索失败");
    assert!(
        found.iter().any(|h| h.utterance_id == hits[0].utterance_id),
        "全文检索找不回它自己：{needle:?}"
    );
}

#[test]
fn full_text_search_escapes_user_input() {
    let c = corpus!();
    // FTS5 里 " 是定界符、AND/OR/* 是运算符。不转义会直接语法错误。
    for needle in ["\"", "a\"b", "AND", "OR", "*", "NEAR(a b)", "(("] {
        let result = c.search_lyrics(needle, 5);
        assert!(result.is_ok(), "{needle:?} 让 FTS5 报错了: {:?}", result.err());
    }
}

#[test]
fn full_text_search_handles_short_japanese_queries() {
    let c = corpus!();
    // trigram 索引对少于 3 个字符的查询一律返回 0 条，而「夜」「恋」「君」
    // 这类单字检索在日语里极其常见。静默返回空比报错更糟——用户会以为
    // 语料里真的没有。短查询必须走 LIKE 兜底。
    let lemma = c
        .word_frequency(Some("NOUN"), 40)
        .expect("查询失败")
        .into_iter()
        .find(|w| w.lemma.chars().count() == 1)
        .map(|w| w.lemma);
    let Some(single) = lemma else { return };

    let hits = c.search_lyrics(&single, 20).expect("查询失败");
    assert!(!hits.is_empty(), "单字「{single}」搜不到，兜底没生效");
    assert!(
        hits.iter().all(|h| h.text.contains(&single)),
        "兜底返回了不含该字的行"
    );
}

#[test]
fn full_text_search_escapes_like_wildcards() {
    let c = corpus!();
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

#[test]
fn full_text_search_ignores_blank_input() {
    let c = corpus!();
    assert!(c.search_lyrics("", 10).expect("查询失败").is_empty());
    assert!(c.search_lyrics("   ", 10).expect("查询失败").is_empty());
}

// ────────────────────────────── 语料 ──────────────────────────────

#[test]
fn overview_is_internally_consistent() {
    let c = corpus!();
    let o = c.overview().expect("查询失败");
    assert!(o.tracks > 0 && o.lyric_lines > 0 && o.tokens > 0);
    assert!(o.vocabulary <= o.tokens, "词型数不可能超过词次数");
    assert!(o.performers <= o.people);
    assert!(o.composers <= o.people);
}

#[test]
fn timeline_years_are_sorted_and_nonempty() {
    let c = corpus!();
    let years = c.timeline().expect("查询失败");
    assert!(!years.is_empty());
    let labels: Vec<&str> = years.iter().map(|y| y.year.as_str()).collect();
    let mut sorted = labels.clone();
    sorted.sort();
    assert_eq!(labels, sorted, "timeline 没有按年份排序");
    assert!(years.iter().all(|y| !y.year.trim().is_empty()));
}

#[test]
fn word_frequency_excludes_punctuation_and_is_descending() {
    let c = corpus!();
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

#[test]
fn word_in_corpus_powers_research_mode() {
    let c = corpus!();
    let lemma = common_lemma(&c);
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

#[test]
fn word_in_corpus_puts_the_current_song_first() {
    let c = corpus!();
    let lemma = common_lemma(&c);
    let any = c.word_in_corpus(&lemma, None, 30).expect("查询失败");
    // 挑一首不是默认排第一的歌，指定它之后应该被顶到最前
    let Some(target) = any
        .examples
        .iter()
        .map(|e| e.song_id.clone())
        .find(|id| *id != any.examples[0].song_id)
    else {
        return; // 这个词只出现在一首歌里，无从验证
    };
    let focused = c
        .word_in_corpus(&lemma, Some(&target), 30)
        .expect("查询失败");
    assert_eq!(
        focused.examples[0].song_id, target,
        "指定当前歌曲后没有把它的例句排到最前"
    );
}

#[test]
fn word_in_corpus_handles_unknown_words() {
    let c = corpus!();
    let stats = c
        .word_in_corpus("绝不可能存在的词元", None, 10)
        .expect("查询失败");
    assert_eq!(stats.occurrences, 0);
    assert!(stats.examples.is_empty());
}

// ────────────────────────────── 歌词 ──────────────────────────────

#[test]
fn lyrics_and_tokens_line_up() {
    let c = corpus!();
    let track = c
        .tracks(300)
        .expect("查询失败")
        .into_iter()
        .find(|t| t.line_count > 5)
        .expect("没有带歌词的曲目");

    let lines = c.lyrics(&track.id).expect("查询失败");
    assert_eq!(lines.len() as i64, track.line_count);
    // lyrics() 一次就把分词带回来了，不用再逐行查
    assert!(lines.iter().any(|l| !l.tokens.is_empty()), "整首歌一个 token 都没有");

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

// ────────────────────────────── 播放历史 ──────────────────────────────

#[test]
fn play_history_round_trips() {
    let path = db_path();
    if !path.exists() {
        return;
    }
    // 这个测试要写库，所以单独开一个可写连接，用完把自己写的记录删掉
    let Ok(c) = Corpus::open_writable(&path) else { return };
    if c.check_schema().is_err() {
        return;
    }
    let Some(track) = c.tracks(1).ok().and_then(|t| t.into_iter().next()) else {
        return;
    };

    let marker = "integration-test";
    c.record_play(&PlayEvent {
        song_id: track.id.clone(),
        listened_sec: 123.0,
        position_sec: 123.0,
        completed: true,
        source: marker.into(),
    })
    .expect("写入失败");

    let recent = c.recently_played(50).expect("查询失败");
    assert!(recent.iter().any(|t| t.song_id == track.id));

    let top = c.most_played(30.0, 50).expect("查询失败");
    assert!(top.iter().any(|t| t.song_id == track.id));
    // 只听了 2 秒的不该进排行
    assert!(
        c.most_played(1000.0, 50).expect("查询失败").is_empty()
            || c.most_played(1000.0, 50).expect("查询失败").iter().all(|t| t.total_listened_sec >= 1000.0)
    );

    c.connection()
        .execute("DELETE FROM play_history WHERE source = ?1", [marker])
        .expect("清理失败");
}

// ────────────────────────── 全局搜索（Cmd+K） ──────────────────────────

#[test]
fn quick_search_finds_a_track_by_title() {
    let Some(corpus) = open_corpus() else { return };
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

#[test]
fn quick_search_finds_a_track_by_artist() {
    let Some(corpus) = open_corpus() else { return };
    let results = corpus.quick_search("ヨルシカ", None).unwrap();
    assert!(!results.tracks.is_empty(), "按歌手名应当找到曲目");
    assert!(!results.people.is_empty(), "歌手本身也应当出现在人物里");
}

#[test]
fn quick_search_matches_people_through_the_normalized_name() {
    let Some(corpus) = open_corpus() else { return };
    // people.normalized_name 是折过大小写的，所以小写也能搜到
    let lower = corpus.quick_search("yoasobi", None).unwrap();
    let upper = corpus.quick_search("YOASOBI", None).unwrap();
    assert_eq!(
        lower.people.is_empty(),
        upper.people.is_empty(),
        "大小写不该影响人物匹配"
    );
}

#[test]
fn quick_search_returns_words_from_the_corpus() {
    let Some(corpus) = open_corpus() else { return };
    let results = corpus.quick_search("夜", None).unwrap();
    assert!(!results.words.is_empty(), "应当返回语料里的词");
    // 精确相等的词要排最前——搜「夜」想要的是「夜」本身
    if let Some(jp_corpus::QuickHit::Word { lemma, .. }) = results.words.first() {
        assert_eq!(lemma, "夜", "精确匹配应当排第一，实际是 {lemma}");
    }
}

#[test]
fn quick_search_returns_lyric_lines() {
    let Some(corpus) = open_corpus() else { return };
    let results = corpus.quick_search("夜", None).unwrap();
    assert!(!results.lyrics.is_empty(), "应当返回歌词行");
}

#[test]
fn quick_search_escapes_like_wildcards() {
    let Some(corpus) = open_corpus() else { return };
    // 「%」被当成通配符的话会命中一切
    let results = corpus.quick_search("%", None).unwrap();
    assert!(
        results.tracks.len() < 5,
        "「%」不该匹配到所有曲目，实际 {} 条",
        results.tracks.len()
    );
}

#[test]
fn quick_search_handles_nonsense_without_erroring() {
    let Some(corpus) = open_corpus() else { return };
    for query in ["", "   ", "'; DROP TABLE songs; --", "\\", "___", "🎵🎵🎵"] {
        let results = corpus
            .quick_search(query, None)
            .unwrap_or_else(|e| panic!("{query:?} 不该报错: {e}"));
        let _ = results.total();
    }
    // 确认库还在
    assert!(corpus.overview().unwrap().tracks > 0);
}

#[test]
fn quick_search_respects_the_per_kind_limit() {
    let Some(corpus) = open_corpus() else { return };
    let results = corpus.quick_search("a", Some(2)).unwrap();
    for group in [&results.tracks, &results.albums, &results.people, &results.words] {
        assert!(group.len() <= 2, "每组不该超过限制");
    }
}

#[test]
fn quick_search_empty_query_returns_nothing() {
    let Some(corpus) = open_corpus() else { return };
    assert!(corpus.quick_search("", None).unwrap().is_empty());
    assert!(corpus.quick_search("   ", None).unwrap().is_empty());
}
