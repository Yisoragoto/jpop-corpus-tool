//! 真实的 HTTP 传输层，基于 `ureq`。
//!
//! 选 ureq 而不是 reqwest：刮削是同步阻塞的场景，ureq 不需要 tokio，
//! 也就不会有「在异步运行时里 block_on」那类死锁风险。
//!
//! 这一层只做一件事：把 ureq 的错误翻译成分类过的 [`ProviderError`]。
//! 重试、退避、限流都在 [`crate::http`] 里，那一层能离线测试。

use std::collections::BTreeMap;

use crate::error::ProviderError;
use crate::http::{HttpResponse, Timeouts, Transport};
use crate::status::ErrorType;

/// 基于 ureq 的传输层。
#[derive(Default)]
pub struct UreqTransport;

impl Transport for UreqTransport {
    fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        timeouts: Timeouts,
    ) -> Result<HttpResponse, ProviderError> {
        // 分两段：连不上的主机在 connect 就失败，而下一张几百 KB 的封面
        // 允许用满 total。只给一个整体超时的话，这两个要求没法同时满足。
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(timeouts.connect))
            .timeout_recv_response(Some(timeouts.connect))
            .timeout_global(Some(timeouts.total))
            .build()
            .into();

        let mut request = agent.get(url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }

        match request.call() {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let headers = collect_headers(response.headers());
                // 限制体积：搜索响应正常是几十 KB，几 MB 的东西一定不对劲
                let body = response
                    .body_mut()
                    .with_config()
                    .limit(16 * 1024 * 1024)
                    .read_to_vec()
                    .map_err(|e| ProviderError::network(format!("读取响应体失败: {e}")))?;
                Ok(HttpResponse {
                    status,
                    body,
                    headers,
                })
            }
            // 非 2xx 也走 Ok 分支交给上层分类——上层要看 Retry-After，
            // 而且 403/429 的判定逻辑集中在一处才好维护。
            Err(ureq::Error::StatusCode(code)) => Ok(HttpResponse {
                status: code,
                body: Vec::new(),
                headers: BTreeMap::new(),
            }),
            Err(err) => Err(classify(&err)),
        }
    }

    fn post(
        &self,
        url: &str,
        body: &[u8],
        headers: &[(String, String)],
        timeouts: Timeouts,
    ) -> Result<HttpResponse, ProviderError> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(timeouts.connect))
            .timeout_recv_response(Some(timeouts.connect))
            .timeout_global(Some(timeouts.total))
            .build()
            .into();
        let mut request = agent.post(url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        match request.send(body) {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let headers = collect_headers(response.headers());
                let body = response
                    .body_mut()
                    .with_config()
                    .limit(64 * 1024 * 1024)
                    .read_to_vec()
                    .map_err(|e| ProviderError::network(format!("读取响应体失败: {e}")))?;
                Ok(HttpResponse {
                    status,
                    body,
                    headers,
                })
            }
            Err(ureq::Error::StatusCode(code)) => Ok(HttpResponse {
                status: code,
                body: Vec::new(),
                headers: BTreeMap::new(),
            }),
            Err(err) => Err(classify(&err)),
        }
    }
}

fn collect_headers(headers: &ureq::http::HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.as_str().to_ascii_lowercase(), v.to_string()))
        })
        .collect()
}

/// 把 ureq 的错误翻译成分类过的错误。
///
/// 这个分类决定「要不要重试」——超时和连不上要重试，
/// 协议错误重试一万次也一样。
fn classify(err: &ureq::Error) -> ProviderError {
    match err {
        ureq::Error::Timeout(_) => ProviderError::timeout(format!("请求超时: {err}")),
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            ProviderError::timeout(format!("请求超时: {io}"))
        }
        ureq::Error::Io(io) => ProviderError::network(format!("网络错误: {io}")),
        ureq::Error::HostNotFound => ProviderError::network("DNS 解析失败".to_string()),
        ureq::Error::ConnectionFailed => ProviderError::network("连接失败".to_string()),
        ureq::Error::TooManyRedirects => {
            ProviderError::new(ErrorType::InvalidResponse, "重定向过多")
        }
        other => ProviderError::network(format!("未预期的错误: {other}")),
    }
}

/// 生产用的默认 provider 组合。
///
/// iTunes 打头（快、JP 区覆盖好），MusicBrainz 兜底（结构化、有时长）。
/// 顺序有意义：resolver 按顺序问，先问到的先进候选池。
pub fn default_providers() -> Vec<Box<dyn crate::providers::MetadataProvider>> {
    use crate::http::HttpClient;
    use crate::providers::{itunes::ITunesProvider, musicbrainz::MusicBrainzProvider};
    vec![
        Box::new(ITunesProvider::production(HttpClient::new(Box::new(
            UreqTransport,
        )))),
        Box::new(MusicBrainzProvider::production(HttpClient::new(Box::new(
            UreqTransport,
        )))),
    ]
}

/// 查艺人用的组合。
///
/// **顺序有意义**：先问 MusicBrainz 拿别名，再拿别名去 Deezer 找照片。
/// 大量日本歌手在 Deezer 上按罗马字收录（ずっと真夜中でいいのに。→
/// ZUTOMAYO），不带别名去查基本找不到。
///
/// iTunes 不在里面——它没有艺人条目，也不提供艺人照片。
pub fn artist_providers() -> (
    crate::providers::musicbrainz::MusicBrainzProvider,
    crate::providers::deezer::DeezerProvider,
) {
    use crate::http::HttpClient;
    (
        crate::providers::musicbrainz::MusicBrainzProvider::production(HttpClient::new(Box::new(
            UreqTransport,
        ))),
        crate::providers::deezer::DeezerProvider::new(HttpClient::new(Box::new(UreqTransport))),
    )
}

/// 带默认 provider 的解析器。
///
/// `for_artwork` 为真时启用「没封面就扣分」，供封面刮削路径使用。
pub fn default_resolver(for_artwork: bool) -> anyhow::Result<crate::resolver::MetadataResolver> {
    use crate::matching::{MatchScorer, ScorerConfig};
    use crate::resolver::ResolverConfig;
    let config = if for_artwork {
        ScorerConfig::for_artwork()
    } else {
        ScorerConfig::default()
    };
    Ok(crate::resolver::MetadataResolver::new(default_providers())?
        .with_scorer(MatchScorer::new(config))
        // iTunes 一问就中的时候不再问 MusicBrainz。
        //
        // A/B 实测（同一批歌交替跑，避开网络漂移）：
        // 全问一遍 10.61 秒/首、够好就早停 0.29 秒/首，**提速 37×**，
        // 而采纳结果一条都没变。MusicBrainz 限每秒一次而且响应慢，
        // 在 iTunes 已经给出 0.99 的情况下纯属白等。
        //
        // iTunes 不够确定时仍然会问 MusicBrainz——那正是它结构化数据
        // 有价值的场合。所以这不只是快，逻辑上也更对。
        .with_config(ResolverConfig {
            stop_after_each_provider: true,
            ..Default::default()
        }))
}
