use std::sync::{Arc,Mutex};
use jp_scraper::{HttpClient,HttpResponse,MetadataProvider,MetadataResolver,TrackFile};
use jp_scraper::http::{Timeouts,Transport};
use serde_json::Value;

struct FixtureTransport { fixture: Value, urls: Arc<Mutex<Vec<String>>> }
impl Transport for FixtureTransport {
    fn get(&self, url: &str, _headers: &[(String,String)], _timeouts: Timeouts) -> Result<HttpResponse,jp_scraper::ProviderError> {
        self.urls.lock().unwrap().push(url.into());
        let key = if url.contains("/release/") { "release" } else if url.contains("/track/") { "track" } else if url.contains("/album/") { "album" } else { "search" };
        assert!(!self.fixture[key].is_null(), "不该发请求：{url}");
        Ok(HttpResponse {status:200, body:serde_json::to_vec(&self.fixture[key]).unwrap(), headers: Default::default()})
    }
}

fn transport(fixture: &str) -> (HttpClient,Arc<Mutex<Vec<String>>>) {
    let urls = Arc::new(Mutex::new(Vec::new()));
    (HttpClient::new(Box::new(FixtureTransport {fixture:serde_json::from_str(fixture).unwrap(),urls:urls.clone()})),urls)
}

fn persist(candidate: &jp_scraper::ScrapeCandidate, song_id: &str) -> jp_corpus::Corpus {
    let corpus = jp_corpus::fixture::corpus().unwrap();
    jp_corpus::enrichment::fill_missing(corpus.connection(),song_id,&jp_corpus::enrichment::Metadata {
        extra:&candidate.extra,album:&candidate.album,year:&candidate.year,genre:&candidate.genre,
        release_date:&candidate.release_date,provider:&candidate.provider,provider_id:&candidate.provider_id,confidence:Some(0.98),
    }).unwrap();
    corpus
}

#[test]
fn one_musicbrainz_detail_request_saves_the_correct_recording_and_album_provenance() {
    let (http,urls) = transport(include_str!("data/musicbrainz-identity.json"));
    let resolver = MetadataResolver::new(vec![Box::new(jp_scraper::providers::musicbrainz::MusicBrainzProvider::new(http))]).unwrap();
    let result = resolver.resolve(&TrackFile {embedded_title:"街の灯".into(),embedded_artist:"夜明バンド".into(),embedded_album:"夜明けの記録".into(),duration_sec:Some(262.0),..Default::default()});
    assert!(result.ok(),"{result:?}");
    let corpus = persist(result.candidate.as_ref().unwrap(),"001");
    let fields = jp_corpus::identity::fields(corpus.connection(),"001").unwrap();
    assert_eq!(fields["isrc"],"JPTEST000001"); assert_eq!(fields["label"],"Fixture Records");
    assert_eq!(fields["track_count"],"2"); assert_eq!(fields["release_group_mbid"],"aaaaaaaa-0000-0000-0000-000000000003");
    assert!(jp_corpus::lineage::provenance(corpus.connection(),"song","001").unwrap().iter().any(|p|p.field=="isrc" && p.source=="musicbrainz"));
    assert!(jp_corpus::lineage::provenance(corpus.connection(),"album",&fields["album_id"]).unwrap().iter().any(|p|p.field=="label" && p.source=="musicbrainz" && p.confidence==Some(0.98)));
    let urls = urls.lock().unwrap(); assert_eq!(urls.len(),2);
    assert!(urls[1].contains("isrcs") && urls[1].contains("labels") && urls[1].contains("release-groups"));
}

#[test]
fn deezer_reads_track_and_album_details_instead_of_only_the_cover() {
    let (http,urls) = transport(include_str!("data/deezer-identity.json"));
    let provider = jp_scraper::providers::deezer::DeezerProvider::new(http);
    let candidate = provider.search_tracks(&jp_scraper::SearchQuery{title:"雨の歌".into(),..Default::default()},1).unwrap().remove(0);
    let candidate = provider.enrich(&candidate).unwrap();
    assert_eq!(candidate.release_date,"2023-04-05");
    let corpus = persist(&candidate,"004");
    let fields = jp_corpus::identity::fields(corpus.connection(),"004").unwrap();
    assert_eq!(fields["isrc"],"JPTEST000042"); assert_eq!(fields["label"],"Fixture Deezer Label"); assert_eq!(fields["track_count"],"9");
    assert!(jp_corpus::lineage::provenance(corpus.connection(),"album",&fields["album_id"]).unwrap().iter().any(|p|p.field=="label" && p.source=="deezer"));
    assert_eq!(urls.lock().unwrap().len(),3);
}
