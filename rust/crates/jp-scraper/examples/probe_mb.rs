//! 单独对 MusicBrainz 发几条真实请求，验最绕的那段解析：
//! artist-credit 拼接、pick_release 挑发行版、字符串形态的音轨号。
fn main() -> anyhow::Result<()> {
    use jp_scraper::http::HttpClient;
    use jp_scraper::providers::{MetadataProvider, musicbrainz::MusicBrainzProvider};
    use jp_scraper::{SearchQuery, UreqTransport};

    let mb = MusicBrainzProvider::production(HttpClient::new(Box::new(UreqTransport)));
    for (title, artist) in [
        ("ダンスホール", "Mrs. GREEN APPLE"),
        ("夜に駆ける", "YOASOBI"),
        ("怪獣", "サカナクション"),
    ] {
        let q = SearchQuery {
            title: title.into(),
            artist: artist.into(),
            ..Default::default()
        };
        match mb.search_tracks(&q, 5) {
            Ok(cands) => {
                println!("\n=== {title} / {artist} → {} 条 ===", cands.len());
                for c in cands.iter().take(3) {
                    println!(
                        "  {:?} / {:?} / 专辑={:?} 年={} 时长={:?} 轨号={:?}",
                        c.title, c.artist, c.album, c.year, c.duration_sec, c.track_number
                    );
                }
            }
            Err(e) => println!("\n=== {title} → 失败: {e} ({:?})", e.error_type),
        }
    }
    Ok(())
}
