//! 把各层串起来：Query → Provider → Candidates → Score → Status。
//!
//! 这一层是整个刮削的大脑，但它刻意**不做**三件事：
//!
//! * 不碰数据库（落库是 [`crate::store`] 的事）
//! * 不碰 UI
//! * 不做分词（corpus pipeline 的事）
//!
//! 它只回答一个问题：**「这个本地文件到底是哪首歌？」**
//! 并且在回答不确定时如实说不确定，而不是自信地猜错。

use std::collections::{BTreeMap, BTreeSet};

use crate::error::ProviderError;
use crate::matching::MatchScorer;
use crate::models::{NormalizedTrack, ResolvedTrack, ScrapeCandidate, TrackFile, utc_now};
use crate::providers::MetadataProvider;
use crate::query::QueryBuilder;
use crate::status::{ErrorType, ScrapeStatus};

/// 判定阈值。
///
/// `accept` 之上自动采纳，`review` 到 `accept` 之间交给用户确认，
/// `review` 之下算没匹配上。宁可留空也不要写错——写错的元数据比空的更难发现。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolverConfig {
    pub accept_threshold: f64,
    /// review 阈值回答的不是「这条对不对」，而是「值不值得让人看一眼」。
    /// 给低了用户多瞟一眼，给高了用户得自己去搜。所以它比 accept 宽松得多。
    pub review_threshold: f64,
    /// 每条查询向 provider 要多少候选
    pub per_query_limit: usize,
    /// 最终保留多少候选给 UI 展示
    pub keep_candidates: usize,
    /// 拿到这个分数就不再发后续查询，省请求
    pub early_stop_threshold: f64,
    /// 每问完**一个** provider 就判断够不够好，而不是问完这一轮的全部。
    ///
    /// 关掉时和 Python 侧一致：一条查询会把所有 provider 都问一遍。
    /// 打开时，iTunes 已经给出 0.99 的话就不再问 MusicBrainz——
    /// 而 MusicBrainz 限每秒一次，是整条链路里最慢的一环。
    ///
    /// 代价：候选列表会少几条备选。对自动采纳（≥0.95）的歌无所谓，
    /// 那些歌的候选列表本来也不会展示给用户看。
    pub stop_after_each_provider: bool,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            accept_threshold: 0.90,
            review_threshold: 0.55,
            per_query_limit: 25,
            keep_candidates: 8,
            early_stop_threshold: 0.95,
            stop_after_each_provider: false,
        }
    }
}

/// 按 provider 顺序、按查询由准到宽，找出最可能的那一条。
pub struct MetadataResolver {
    providers: Vec<Box<dyn MetadataProvider>>,
    pub scorer: MatchScorer,
    pub query_builder: QueryBuilder,
    pub config: ResolverConfig,
}

impl MetadataResolver {
    pub fn new(providers: Vec<Box<dyn MetadataProvider>>) -> anyhow::Result<Self> {
        anyhow::ensure!(!providers.is_empty(), "至少需要一个 provider");
        Ok(Self {
            providers,
            scorer: MatchScorer::default(),
            query_builder: QueryBuilder::default(),
            config: ResolverConfig::default(),
        })
    }

    pub fn with_scorer(mut self, scorer: MatchScorer) -> Self {
        self.scorer = scorer;
        self
    }
    pub fn with_query_builder(mut self, builder: QueryBuilder) -> Self {
        self.query_builder = builder;
        self
    }
    pub fn with_config(mut self, config: ResolverConfig) -> Self {
        self.config = config;
        self
    }

    /// 解析单个文件。**任何情况下都返回 `ResolvedTrack`，不抛异常。**
    pub fn resolve(&self, track: &TrackFile) -> ResolvedTrack {
        if !track.has_identity() {
            return ResolvedTrack {
                file_path: track.path.clone(),
                status: ScrapeStatus::Failed,
                error_type: ErrorType::NoResults,
                error_message: "文件既没有内嵌曲名，也无法从文件名解析出曲名".into(),
                matched_at: utc_now(),
                ..Default::default()
            };
        }

        let queries = self.query_builder.build(track);
        if queries.is_empty() {
            return ResolvedTrack {
                file_path: track.path.clone(),
                status: ScrapeStatus::Failed,
                error_type: ErrorType::NoResults,
                error_message: "无法构造查询".into(),
                matched_at: utc_now(),
                ..Default::default()
            };
        }

        let want = NormalizedTrack::from_file(track);
        // BTreeMap 而不是 HashMap：候选池的遍历顺序要稳定，
        // 否则同分候选的排序会在两次运行之间跳。
        let mut pool: BTreeMap<(String, String, String), ScrapeCandidate> = BTreeMap::new();
        let mut queries_tried: Vec<String> = Vec::new();
        let mut providers_tried: Vec<String> = Vec::new();
        let mut errors: Vec<ProviderError> = Vec::new();
        // 已经明确在限流的 provider 不再问第二遍：查询阶梯有 5 级，
        // 继续问只会把限流拖得更久，而且每一级还各带 3 次重试。
        let mut throttled: BTreeSet<&str> = BTreeSet::new();

        for query in &queries {
            queries_tried.push(format!("{}:{}", query.kind, query.term()));
            for provider in &self.providers {
                if throttled.contains(provider.name()) {
                    continue;
                }
                if !providers_tried.iter().any(|n| n == provider.name()) {
                    providers_tried.push(provider.name().to_string());
                }
                match provider.search_tracks(query, self.config.per_query_limit) {
                    Ok(found) => {
                        for candidate in found {
                            // 跨 provider 去重：同一首歌两边都有时只留先到的那条
                            pool.entry(candidate.identity_key()).or_insert(candidate);
                        }
                    }
                    Err(err) => {
                        if err.error_type == ErrorType::RateLimit {
                            throttled.insert(provider.name());
                        }
                        errors.push(err);
                    }
                }

                // 已经够好就别再问下一个源了。MusicBrainz 限每秒一次，
                // 而 iTunes 通常一问就中——白问一遍的代价是整条链路慢一倍。
                if self.config.stop_after_each_provider && !pool.is_empty() {
                    let ranked = self.scorer.rank_normalized(
                        &want,
                        pool.values().cloned().collect(),
                        track.duration_sec,
                        track.track_number,
                    );
                    if ranked
                        .first()
                        .is_some_and(|b| b.score() >= self.config.early_stop_threshold)
                    {
                        return self.decide(track, ranked, queries_tried, providers_tried);
                    }
                }
            }

            if !pool.is_empty() {
                let ranked = self.scorer.rank_normalized(
                    &want,
                    pool.values().cloned().collect(),
                    track.duration_sec,
                    track.track_number,
                );
                if ranked
                    .first()
                    .is_some_and(|b| b.score() >= self.config.early_stop_threshold)
                {
                    return self.decide(track, ranked, queries_tried, providers_tried);
                }
            }

            if throttled.len() == self.providers.len() {
                break; // 所有源都在限流，剩下的查询没有意义
            }
        }

        if pool.is_empty() {
            return self.no_candidates(track, &errors, queries_tried, providers_tried);
        }
        let ranked = self.scorer.rank_normalized(
            &want,
            pool.values().cloned().collect(),
            track.duration_sec,
            track.track_number,
        );
        self.decide(track, ranked, queries_tried, providers_tried)
    }

    fn decide(
        &self,
        track: &TrackFile,
        ranked: Vec<ScrapeCandidate>,
        queries_tried: Vec<String>,
        providers_tried: Vec<String>,
    ) -> ResolvedTrack {
        let best = ranked[0].clone();
        let total = ranked.len();
        let kept: Vec<ScrapeCandidate> = ranked
            .into_iter()
            .take(self.config.keep_candidates)
            .collect();

        let (status, error_type, message) = if best.score() >= self.config.accept_threshold {
            (ScrapeStatus::Success, ErrorType::None, String::new())
        } else if best.score() >= self.config.review_threshold {
            (
                ScrapeStatus::LowConfidence,
                ErrorType::None,
                format!(
                    "最佳候选置信度 {:.2}，低于自动采纳阈值 {:.2}，需要人工确认",
                    best.score(),
                    self.config.accept_threshold
                ),
            )
        } else {
            (
                ScrapeStatus::Failed,
                ErrorType::NoMatch,
                format!("共 {total} 条候选，最高分只有 {:.2}", best.score()),
            )
        };

        ResolvedTrack {
            file_path: track.path.clone(),
            status,
            confidence: best.score(),
            // 判定失败时不给出 candidate：宁可留空，也不要让调用方
            // 顺手把一条没过阈值的结果写进库
            candidate: (status != ScrapeStatus::Failed).then_some(best),
            candidates: kept,
            error_type,
            error_message: message,
            queries_tried,
            providers_tried,
            matched_at: utc_now(),
        }
    }

    /// 一条候选都没有：要区分「搜过了没有」和「根本没搜成」。
    fn no_candidates(
        &self,
        track: &TrackFile,
        errors: &[ProviderError],
        queries_tried: Vec<String>,
        providers_tried: Vec<String>,
    ) -> ResolvedTrack {
        let (error_type, message) = if errors.is_empty() {
            (ErrorType::NoResults, "所有查询都没有返回结果".to_string())
        } else {
            // 优先报可重试的错误：用户看到「限流」会知道等会儿再来，
            // 看到「无结果」就不会再试了。
            let primary = errors.iter().find(|e| e.retryable()).unwrap_or(&errors[0]);
            (primary.error_type, primary.to_string())
        };

        ResolvedTrack {
            file_path: track.path.clone(),
            status: ScrapeStatus::Failed,
            confidence: 0.0,
            error_type,
            error_message: message,
            queries_tried,
            providers_tried,
            matched_at: utc_now(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SearchQuery;
    use std::sync::Mutex;

    /// 按调用次数吐出预设结果的假 provider。
    struct FakeProvider {
        name: &'static str,
        confidence: f64,
        steps: Mutex<std::collections::VecDeque<Result<Vec<ScrapeCandidate>, ProviderError>>>,
        pub calls: Mutex<Vec<String>>,
    }

    impl FakeProvider {
        fn new(
            name: &'static str,
            steps: Vec<Result<Vec<ScrapeCandidate>, ProviderError>>,
        ) -> Self {
            Self {
                name,
                confidence: 1.0,
                steps: Mutex::new(steps.into()),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl MetadataProvider for FakeProvider {
        fn name(&self) -> &'static str {
            self.name
        }
        fn confidence(&self) -> f64 {
            self.confidence
        }
        fn search_tracks(
            &self,
            query: &SearchQuery,
            _limit: usize,
        ) -> Result<Vec<ScrapeCandidate>, ProviderError> {
            self.calls.lock().unwrap().push(query.kind.clone());
            self.steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(Vec::new()))
        }
    }

    fn track() -> TrackFile {
        TrackFile {
            path: "D:/a/1.flac".into(),
            embedded_title: "夜に駆ける".into(),
            embedded_artist: "YOASOBI".into(),
            embedded_album: "THE BOOK".into(),
            duration_sec: Some(261.0),
            ..Default::default()
        }
    }

    fn perfect() -> ScrapeCandidate {
        ScrapeCandidate {
            provider: "fake".into(),
            provider_id: "1".into(),
            title: "夜に駆ける".into(),
            artist: "YOASOBI".into(),
            album: "THE BOOK".into(),
            duration_sec: Some(261.0),
            provider_confidence: 1.0,
            ..Default::default()
        }
    }

    fn resolver(providers: Vec<Box<dyn MetadataProvider>>) -> MetadataResolver {
        MetadataResolver::new(providers).unwrap()
    }

    #[test]
    fn a_perfect_match_is_accepted_automatically() {
        let r = resolver(vec![Box::new(FakeProvider::new(
            "fake",
            vec![Ok(vec![perfect()])],
        ))]);
        let out = r.resolve(&track());
        assert_eq!(out.status, ScrapeStatus::Success);
        assert!(out.confidence >= 0.90, "{}", out.confidence);
        assert_eq!(out.provider(), "fake");
        // 判定要能解释
        assert!(out.breakdown().is_some());
    }

    #[test]
    fn a_mediocre_match_goes_to_review_not_to_the_database() {
        // 曲名对上但歌手完全不同：不该自动采纳，也不该丢掉
        let mut c = perfect();
        c.artist = "まったく別の人".into();
        c.album = String::new();
        c.duration_sec = None;
        let r = resolver(vec![Box::new(FakeProvider::new("fake", vec![Ok(vec![c])]))]);
        let out = r.resolve(&track());
        assert_eq!(
            out.status,
            ScrapeStatus::LowConfidence,
            "{}",
            out.confidence
        );
        assert!(out.candidate.is_some(), "要留着给用户看");
        assert!(out.error_message.contains("需要人工确认"));
    }

    #[test]
    fn a_bad_match_yields_no_candidate_at_all() {
        // 宁可留空，也不要让调用方顺手把没过阈值的结果写进库
        let mut c = perfect();
        c.title = "まったく別の曲".into();
        c.artist = "別の人".into();
        c.album = String::new();
        c.duration_sec = Some(30.0);
        let r = resolver(vec![Box::new(FakeProvider::new("fake", vec![Ok(vec![c])]))]);
        let out = r.resolve(&track());
        assert_eq!(out.status, ScrapeStatus::Failed);
        assert_eq!(out.error_type, ErrorType::NoMatch);
        assert!(out.candidate.is_none());
        // 但候选列表要留着，用户可能想手动挑
        assert!(!out.candidates.is_empty());
    }

    #[test]
    fn a_track_without_a_title_never_hits_the_network() {
        let p = FakeProvider::new("fake", vec![Ok(vec![perfect()])]);
        let calls = std::sync::Arc::new(());
        let _ = calls;
        let r = resolver(vec![Box::new(p)]);
        let out = r.resolve(&TrackFile {
            path: "D:/a/x.flac".into(),
            ..Default::default()
        });
        assert_eq!(out.status, ScrapeStatus::Failed);
        assert!(out.queries_tried.is_empty());
        assert!(out.providers_tried.is_empty());
    }

    #[test]
    fn a_high_score_stops_the_query_ladder_early() {
        // 第一条查询就命中，剩下四级不该再发
        let p = FakeProvider::new("fake", vec![Ok(vec![perfect()])]);
        let r = resolver(vec![Box::new(p)]);
        let out = r.resolve(&track());
        assert_eq!(out.status, ScrapeStatus::Success);
        assert_eq!(out.queries_tried.len(), 1, "{:?}", out.queries_tried);
    }

    #[test]
    fn a_throttled_provider_is_not_asked_again() {
        // 查询阶梯有 5 级，每级还带 3 次重试——被限流后继续问
        // 只会把限流拖得更久
        let p = FakeProvider::new(
            "fake",
            vec![Err(ProviderError::rate_limited("429").with_provider("fake"))],
        );
        let r = resolver(vec![Box::new(p)]);
        let out = r.resolve(&track());
        assert_eq!(out.status, ScrapeStatus::Failed);
        assert_eq!(out.error_type, ErrorType::RateLimit);
        // 所有 provider 都在限流时应当立刻停下
        assert_eq!(out.queries_tried.len(), 1, "{:?}", out.queries_tried);
    }

    #[test]
    fn a_retryable_error_is_reported_over_a_dead_end_one() {
        // 用户看到「限流」会知道等会儿再来，看到「无结果」就不会再试了
        let p1 = FakeProvider::new(
            "a",
            vec![Err(
                ProviderError::new(ErrorType::NoResults, "空").with_provider("a")
            )],
        );
        let p2 = FakeProvider::new(
            "b",
            vec![Err(ProviderError::timeout("超时").with_provider("b"))],
        );
        let r = resolver(vec![Box::new(p1), Box::new(p2)]);
        let out = r.resolve(&track());
        assert_eq!(out.error_type, ErrorType::Timeout);
    }

    #[test]
    fn one_broken_provider_does_not_stop_the_others() {
        let broken = FakeProvider::new("broken", vec![Err(ProviderError::network("炸了"))]);
        let good = FakeProvider::new("good", vec![Ok(vec![perfect()])]);
        let r = resolver(vec![Box::new(broken), Box::new(good)]);
        let out = r.resolve(&track());
        assert_eq!(out.status, ScrapeStatus::Success);
        assert_eq!(out.providers_tried, vec!["broken", "good"]);
    }

    #[test]
    fn the_same_song_from_two_providers_is_deduplicated() {
        let mut b = perfect();
        b.provider = "b".into();
        let p1 = FakeProvider::new("a", vec![Ok(vec![perfect()])]);
        let p2 = FakeProvider::new("b", vec![Ok(vec![b])]);
        let r = resolver(vec![Box::new(p1), Box::new(p2)])
            // 关掉提前停止，逼它两个源都问
            .with_config(ResolverConfig {
                early_stop_threshold: 2.0,
                ..Default::default()
            });
        let out = r.resolve(&track());
        assert_eq!(out.candidates.len(), 1, "{:?}", out.candidates);
    }

    #[test]
    fn the_candidate_list_is_capped() {
        let many: Vec<ScrapeCandidate> = (0..30)
            .map(|i| ScrapeCandidate {
                provider: "fake".into(),
                title: format!("曲{i}"),
                artist: "YOASOBI".into(),
                provider_confidence: 1.0,
                ..Default::default()
            })
            .collect();
        let r = resolver(vec![Box::new(FakeProvider::new("fake", vec![Ok(many)]))]).with_config(
            ResolverConfig {
                early_stop_threshold: 2.0,
                keep_candidates: 8,
                ..Default::default()
            },
        );
        let out = r.resolve(&track());
        assert_eq!(out.candidates.len(), 8);
    }

    #[test]
    fn every_result_records_what_was_tried() {
        // 「为什么这首歌搜不到」要能回答
        let r = resolver(vec![Box::new(FakeProvider::new("fake", vec![Ok(vec![])]))]);
        let out = r.resolve(&track());
        assert!(!out.queries_tried.is_empty());
        assert!(out.queries_tried[0].starts_with("full:"));
        assert_eq!(out.providers_tried, vec!["fake"]);
        assert!(!out.matched_at.is_empty());
    }

    #[test]
    fn a_confident_first_provider_stops_before_the_second_one() {
        // 这一条是整条链路最大的提速来源：iTunes 一问就中的时候
        // 不再问 MusicBrainz。A/B 实测 10.61 → 0.29 秒/首。
        let fast = FakeProvider::new("fast", vec![Ok(vec![perfect()])]);
        let slow = FakeProvider::new("slow", vec![Ok(vec![perfect()])]);
        let r = resolver(vec![Box::new(fast), Box::new(slow)]).with_config(ResolverConfig {
            stop_after_each_provider: true,
            ..Default::default()
        });
        let out = r.resolve(&track());
        assert_eq!(out.status, ScrapeStatus::Success);
        assert_eq!(out.providers_tried, vec!["fast"], "第二个源不该被问到");
    }

    #[test]
    fn an_unconfident_first_provider_still_falls_through() {
        // iTunes 不够确定时仍要问 MusicBrainz——那正是它有价值的场合
        let mut weak = perfect();
        weak.title = "差得远的曲名".into();
        weak.artist = "别人".into();
        weak.album = String::new();
        weak.duration_sec = None;
        let fast = FakeProvider::new("fast", vec![Ok(vec![weak])]);
        let slow = FakeProvider::new("slow", vec![Ok(vec![perfect()])]);
        let r = resolver(vec![Box::new(fast), Box::new(slow)]).with_config(ResolverConfig {
            stop_after_each_provider: true,
            ..Default::default()
        });
        let out = r.resolve(&track());
        assert_eq!(out.providers_tried, vec!["fast", "slow"]);
        assert_eq!(out.status, ScrapeStatus::Success);
    }

    #[test]
    fn without_the_flag_every_provider_is_asked() {
        // 关掉时和 Python 侧一致
        let fast = FakeProvider::new("fast", vec![Ok(vec![perfect()])]);
        let slow = FakeProvider::new("slow", vec![Ok(vec![perfect()])]);
        let r = resolver(vec![Box::new(fast), Box::new(slow)]).with_config(ResolverConfig {
            stop_after_each_provider: false,
            ..Default::default()
        });
        assert_eq!(r.resolve(&track()).providers_tried, vec!["fast", "slow"]);
    }

    #[test]
    fn an_empty_provider_list_is_rejected_at_construction() {
        assert!(MetadataResolver::new(vec![]).is_err());
    }
}
