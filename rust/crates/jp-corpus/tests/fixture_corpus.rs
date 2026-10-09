//! 对着合成库（`jp_corpus::fixture`）跑的集成测试。**哪台机器都跑，CI 上也跑。**
//!
//! 两部分：
//!
//! 1. `invariants/` 里那份和真库共用的断言——同一份函数，`real_corpus.rs` 在真库上再跑一遍；
//! 2. 只有合成库能做的：数据是已知的，断言可以精确到字，还可以放心写库。
//!
//! 合成库建在内存里，每个测试各建一个，互相看不见对方写的东西。

#[macro_use]
mod invariants;

use jp_corpus::{Corpus, KwicQuery, MatchField, PlayEvent, fixture};

fn corpus() -> Corpus {
    fixture::corpus().expect("建不出合成库")
}

// 合成库就是为这些断言造的：前提不成立不许跳过（`true`）
invariant_tests!(Some((corpus(), true)));

fn kwic(c: &Corpus, lemma: &str, change: impl FnOnce(&mut KwicQuery)) -> Vec<jp_corpus::KwicHit> {
    let mut query = KwicQuery {
        keywords: vec![lemma.into()],
        field: MatchField::Lemma,
        ..Default::default()
    };
    change(&mut query);
    c.kwic(&query).expect("查询失败")
}

// ────────────────────────── 精确到字的断言 ──────────────────────────

#[test]
fn kwic_cuts_the_line_exactly_at_the_keyword() {
    let c = corpus();
    let mut cut: Vec<(String, String, String, String)> = kwic(&c, "風", |_| {})
        .into_iter()
        .map(|h| (h.song_id, h.left, h.keyword, h.right))
        .collect();
    cut.sort();
    let own = |a: &str, b: &str, c: &str, d: &str| (a.to_string(), b.to_string(), c.to_string(), d.to_string());
    assert_eq!(cut, vec![own("001", "", "風", "が吹いている"), own("002", "遠くで", "風", "が鳴る")]);
}

#[test]
fn kwic_matches_the_inflected_form_through_the_lemma_only() {
    let c = corpus();
    // 歌词里写的是「消え」，词典形是「消える」
    let by_lemma = kwic(&c, "消える", |_| {});
    assert_eq!(by_lemma.len(), 1);
    assert_eq!(by_lemma[0].keyword, "消え");
    assert!(kwic(&c, "消える", |q| q.field = MatchField::Surface).is_empty());
    assert_eq!(kwic(&c, "消え", |q| q.field = MatchField::Surface).len(), 1);
}

#[test]
fn kwic_folds_the_repeated_chorus_line_into_one_hit() {
    let c = corpus();
    // 「夜」：001 一次，004 三次，其中两次是同一句副歌
    let folded = kwic(&c, "夜", |_| {});
    assert_eq!(folded.len(), 3);
    let chorus: Vec<_> = folded.iter().filter(|h| h.text == "夜はまだ長い、").collect();
    assert_eq!(chorus.len(), 1, "同一首歌里的同一句应当折成一条");
    assert_eq!(chorus[0].repeat_count, 2);
    assert!(folded.iter().filter(|h| h.text != "夜はまだ長い、").all(|h| h.repeat_count == 1));

    let raw = kwic(&c, "夜", |q| q.dedup = false);
    assert_eq!(raw.len(), 4);
    assert!(raw.iter().all(|h| h.repeat_count == 1));
}

#[test]
fn kwic_cross_line_brings_in_the_neighbouring_lines() {
    let c = corpus();
    // 001 的第 2 行「君の声を思い出す」夹在「街の灯りが消えて」和「風が吹いている」中间
    let plain = &kwic(&c, "声", |_| {})[0];
    assert_eq!((plain.left.as_str(), plain.right.as_str()), ("君の", "を思い出す"));
    let wide = &kwic(&c, "声", |q| q.cross_line = true)[0];
    assert!(wide.left.contains("街の灯りが消えて") && wide.left.ends_with("君の"), "{:?}", wide.left);
    assert!(wide.right.starts_with("を思い出す") && wide.right.contains("風が吹いている"), "{:?}", wide.right);
    assert_eq!(wide.keyword, "声");
}

#[test]
fn kwic_by_person_leaves_out_everyone_elses_songs() {
    let c = corpus();
    // 5 号（Akari）只唱了 004；3 号（みなも）的歌里没有「夜」
    let hers = kwic(&c, "夜", |q| q.person_ids = vec![5]);
    assert_eq!(hers.len(), 2);
    assert!(hers.iter().all(|h| h.song_id == fixture::RICH_SONG));
    assert!(kwic(&c, "夜", |q| q.person_ids = vec![3]).is_empty());
    // 2 号给 001 和 004 都作了曲：按人筛选看的是信用，不是 songs.artist 那个字符串
    assert_eq!(kwic(&c, "夜", |q| q.person_ids = vec![2]).len(), 3);
}

#[test]
fn kwic_part_of_speech_filter_is_applied() {
    let c = corpus();
    assert_eq!(kwic(&c, "夜", |q| q.pos = Some("NOUN".into())).len(), 3);
    assert!(kwic(&c, "夜", |q| q.pos = Some("VERB".into())).is_empty());
}

#[test]
fn a_percent_sign_or_an_underscore_finds_only_the_line_that_has_one() {
    let c = corpus();
    // 短查询走 LIKE 兜底。没转义的话「%」会命中全部 13 行，「_」会命中所有不是空的行
    for needle in ["%", "_"] {
        let hits = c.search_lyrics(needle, 50).expect("查询失败");
        let texts: Vec<&str> = hits.iter().map(|h| h.text.as_str()).collect();
        assert_eq!(texts, vec!["100%の_夢を見た"], "搜 {needle:?}");
    }
}

#[test]
fn full_text_search_finds_long_and_short_queries_alike() {
    let c = corpus();
    // 三个字以上走 trigram 索引，一两个字走 LIKE 兜底；两条路都要找得到，而且只找到该找的
    let long = c.search_lyrics("灯りが消え", 50).expect("查询失败");
    assert_eq!(long.iter().map(|h| h.text.as_str()).collect::<Vec<_>>(), vec!["街の灯りが消えて"]);
    let short = c.search_lyrics("風", 50).expect("查询失败");
    let mut songs: Vec<&str> = short.iter().map(|h| h.song_id.as_str()).collect();
    songs.sort_unstable();
    assert_eq!(songs, vec!["001", "002"]);
    // 没分词的那首照样搜得到：全文检索看的是歌词原文，不是 token
    let untokenized = c.search_lyrics("青い月", 50).expect("查询失败");
    assert_eq!(untokenized.len(), 1);
    assert_eq!(untokenized[0].song_id, fixture::UNTOKENIZED_SONG);
}

#[test]
fn punctuation_and_symbols_stay_out_of_the_word_frequency_table() {
    let c = corpus();
    let words = c.word_frequency(None, 200).expect("查询失败");
    for dropped in ["、", "%", "_"] {
        assert!(words.iter().all(|w| w.lemma != dropped), "词频表里混进了 {dropped:?}");
    }
    let night = words.iter().find(|w| w.lemma == "夜").expect("词频表里应当有「夜」");
    assert_eq!((night.freq, night.song_count), (4, 2));
    // 活用形归到词典形下面，表层形都列出来
    let see = words.iter().find(|w| w.lemma == "見る").expect("词频表里应当有「見る」");
    assert_eq!(see.surfaces, vec!["見".to_string()]);
}

#[test]
fn the_album_without_artwork_shows_its_tracks_cover() {
    let c = corpus();
    let albums = c.albums(None, 10).expect("查询失败");
    assert_eq!(albums.len() as i64, fixture::ALBUMS);
    let by_title = |title: &str| albums.iter().find(|a| a.title == title).expect("专辑不在");
    assert_eq!(by_title("Rain Notes").artwork_path, "E:/music/covers/004.jpg");
    assert_eq!(by_title("Rain Notes").track_count, 1);
    // 曲目也都没有封面的专辑：如实给空，不去别的专辑借
    assert_eq!(by_title("夜明けの記録").artwork_path, "");
    assert_eq!(by_title("夜明けの記録").track_count, 2);
}

#[test]
fn the_performer_photo_comes_from_the_artists_table() {
    let c = corpus();
    let with_photo = c.person_by_id(5).expect("查询失败").expect("5 号不在");
    assert_eq!(with_photo.name, fixture::ARTIST_WITH_PHOTO);
    assert_eq!(with_photo.image_path, "E:/music/artists/Akari.jpg");
    let without = c.person_by_id(1).expect("查询失败").expect("1 号不在");
    assert_eq!(without.image_path, "");
    assert!(c.person_by_id(999).expect("查询失败").is_none());
}

#[test]
fn credits_come_back_in_the_order_the_page_shows_them() {
    let c = corpus();
    let roles: Vec<String> = c
        .credits_for_track(fixture::RICH_SONG)
        .expect("查询失败")
        .into_iter()
        .map(|credit| credit.role)
        .collect();
    assert_eq!(roles, vec!["lyricist", "composer", "arranger", "performer"]);
}

#[test]
fn the_collaboration_graph_counts_shared_tracks() {
    let c = corpus();
    // 2 号（作曲）和 1 号（演唱）在 001、002 上合作过，和 5 号、4 号在 004 上各一次
    let mut peers: Vec<(i64, i64)> = c
        .collaborators(2, 50)
        .expect("查询失败")
        .into_iter()
        .map(|p| (p.person_id, p.shared_tracks))
        .collect();
    peers.sort_unstable();
    assert_eq!(peers, vec![(1, 2), (4, 1), (5, 1)]);
}

#[test]
fn the_overview_counts_what_the_fixture_says_it_has() {
    let c = corpus();
    let o = c.overview().expect("查询失败");
    assert_eq!((o.tracks, o.lyric_lines, o.tokens), (fixture::TRACKS, fixture::LYRIC_LINES, fixture::TOKENS));
    assert_eq!(o.albums, fixture::ALBUMS);
    assert_eq!((o.people, o.performers, o.composers, o.lyricists, o.arrangers), (5, 3, 2, 2, 1));
    let years: Vec<(String, i64)> = c.timeline().expect("查询失败").into_iter().map(|y| (y.year, y.track_count)).collect();
    assert_eq!(years, vec![("2019".to_string(), 2), ("2021".to_string(), 1), ("2023".to_string(), 1)]);
}

#[test]
fn a_word_seen_from_inside_a_song_lists_that_song_first() {
    let c = corpus();
    let anywhere = c.word_in_corpus("夜", None, 10).expect("查询失败");
    assert_eq!((anywhere.occurrences, anywhere.song_count, anywhere.artist_count), (4, 2, 2));
    // 例句一行一条：副歌那两行文本相同但是两行，都算
    assert_eq!(anywhere.examples.len(), 4);
    for current in ["001", fixture::RICH_SONG] {
        let focused = c.word_in_corpus("夜", Some(current), 10).expect("查询失败");
        assert_eq!(focused.examples[0].song_id, current);
    }
}

// ────────────────────────────── 写库 ──────────────────────────────

/// 以前这条在 `real_corpus.rs` 里，往**用户的真库**插一条播放记录、断言完再删掉；
/// 中途哪个断言失败，那条记录就留在库里了。合成库是内存里的，想怎么写怎么写。
#[test]
fn play_history_round_trips() {
    let c = corpus();
    let before = c.recently_played(50).expect("查询失败").len();
    // 003 在 fixture 里一次都没放过
    c.record_play(&PlayEvent {
        song_id: fixture::UNTOKENIZED_SONG.into(),
        listened_sec: 123.0,
        position_sec: 123.0,
        completed: true,
        source: "integration-test".into(),
    })
    .expect("写入失败");

    let recent = c.recently_played(50).expect("查询失败");
    assert_eq!(recent.len(), before + 1);
    assert_eq!(recent[0].song_id, fixture::UNTOKENIZED_SONG, "刚放的排最前");

    let top = c.most_played(30.0, 50).expect("查询失败");
    assert!(top.iter().any(|t| t.song_id == fixture::UNTOKENIZED_SONG));
    // 门槛是「累计听了多久」：fixture 里 002 只听了 30 秒，够不上 100 秒的门槛
    let serious = c.most_played(100.0, 50).expect("查询失败");
    assert!(serious.iter().all(|t| t.total_listened_sec >= 100.0), "{serious:?}");
    assert!(serious.iter().all(|t| t.song_id != "002"));
    assert!(serious.iter().any(|t| t.song_id == "001"));
}
