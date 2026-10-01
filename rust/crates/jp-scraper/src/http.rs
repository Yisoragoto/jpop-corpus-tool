//! 带超时、重试、指数退避和限流的 HTTP 客户端。
//!
//! 两个必须做到的事，都是原来那套 `_http_bytes` 做不到的：
//!
//! 1. **异常要分类。** 所有失败一律返回 None 的话，上层分不清超时、404、
//!    限流和 DNS 挂了，也就没法决定「要不要重试」。
//! 2. **能离线测试。** 传输层是可注入的 trait，测试完全不碰网络；
//!    时钟和 sleep 也是注入的，所以退避和限流的行为能在毫秒内验完。

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::error::ProviderError;

pub const DEFAULT_UA: &str = "Mozilla/5.0";

/// 一次 HTTP 响应。
#[derive(Debug, Clone, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    /// 头名一律小写，免得各处再纠结大小写
    pub headers: BTreeMap<String, String>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }
}

/// 一次请求的两段超时。
///
/// **分成两段是必须的。** 只有一个「整体超时」时，它既要短到让连不上的
/// 主机快点失败，又要长到能下完一张 290KB 的封面——这两个要求互相矛盾。
/// 实测 Apple 的 CDN 上一张 600x600 封面要 27 秒，而 10 秒的整体超时
/// 让**所有封面都下不下来**。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeouts {
    /// 建连 + 拿到响应头。连不上的主机应当在这里就失败。
    pub connect: Duration,
    /// 整个请求，包括读完响应体。
    pub total: Duration,
}

impl Timeouts {
    pub fn new(connect: Duration, total: Duration) -> Self {
        Self { connect, total }
    }
}

/// 真正发请求的东西。测试注入一个假的，就完全不碰网络。
pub trait Transport: Send + Sync {
    fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        timeouts: Timeouts,
    ) -> Result<HttpResponse, ProviderError>;

    /// 发一个带 JSON body 的 POST。
    ///
    /// 刮削全是 GET，但 AnkiConnect 只收 POST。默认实现直接报错，
    /// 只有真正用得上的传输层才需要实现它。
    fn post(
        &self,
        _url: &str,
        _body: &[u8],
        _headers: &[(String, String)],
        _timeouts: Timeouts,
    ) -> Result<HttpResponse, ProviderError> {
        Err(ProviderError::new(
            crate::ErrorType::Unknown,
            "这个传输层不支持 POST",
        ))
    }
}

/// 时钟和 sleep。注入进来是为了让退避和限流的测试不真的等。
pub trait Clock: Send + Sync {
    /// 单调时间，起点任意。
    fn now(&self) -> Duration;
    fn sleep(&self, duration: Duration);
}

pub struct SystemClock {
    start: std::time::Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// 最简单的「两次请求至少间隔 N 秒」限流器，线程安全。
///
/// MusicBrainz 明确要求每秒最多一次，超了会直接失败。
/// 进程内共享一个实例即可——所有线程在这里排队。
#[derive(Debug)]
pub struct RateLimiter {
    min_interval: Duration,
    /// None 表示「还没发过任何请求」。
    ///
    /// **不能用 0 当哨兵**：单调时钟刚开机时本来就接近 0，
    /// 第一次请求会白等一个间隔。Python 侧踩过这个坑。
    last: Mutex<Option<Duration>>,
}

impl RateLimiter {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last: Mutex::new(None),
        }
    }

    /// 必要时阻塞，返回实际等待的时间（测试用）。
    pub fn acquire(&self, clock: &dyn Clock) -> Duration {
        if self.min_interval.is_zero() {
            return Duration::ZERO;
        }
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let now = clock.now();
        let Some(previous) = *last else {
            *last = Some(now);
            return Duration::ZERO;
        };
        let elapsed = now.saturating_sub(previous);
        let wait = self.min_interval.saturating_sub(elapsed);
        if !wait.is_zero() {
            clock.sleep(wait);
        }
        *last = Some(clock.now());
        wait
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HttpConfig {
    /// 整个请求的上限，包括读完响应体
    pub timeout: Duration,
    /// 建连 + 响应头的上限。比 `timeout` 短，好让连不上的主机快点失败。
    pub connect_timeout: Duration,
    pub retries: u32,
    pub backoff: Duration,
    pub backoff_factor: f64,
    /// 尊重 429/503 的 Retry-After，但别听它说等一小时
    pub max_retry_after: Duration,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(10),
            retries: 2,
            backoff: Duration::from_millis(600),
            backoff_factor: 2.0,
            max_retry_after: Duration::from_secs(30),
        }
    }
}

impl HttpConfig {
    /// 下图片用的配置。
    ///
    /// 搜索接口返回几十 KB 的 JSON，10 秒绰绰有余；一张 600x600 封面
    /// 是几百 KB，实测这条链路上要 27 秒。用同一套超时的结果是
    /// **一张封面都下不下来**，而且日志里只有一句 timeout。
    ///
    /// 建连超时仍然短：真正连不上的主机不该拖满一分钟。
    pub fn for_artwork() -> Self {
        Self {
            timeout: Duration::from_secs(90),
            connect_timeout: Duration::from_secs(10),
            // 图片超时后重试代价很大（又是 90 秒），退到小尺寸更划算
            retries: 1,
            ..Self::default()
        }
    }
}

/// 一个 provider 一个实例：各自的 UA、超时和限流互不影响。
pub struct HttpClient {
    pub user_agent: String,
    pub config: HttpConfig,
    pub provider_name: String,
    rate_limiter: Option<std::sync::Arc<RateLimiter>>,
    transport: Box<dyn Transport>,
    clock: Box<dyn Clock>,
}

impl HttpClient {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            user_agent: DEFAULT_UA.to_string(),
            config: HttpConfig::default(),
            provider_name: String::new(),
            rate_limiter: None,
            transport,
            clock: Box::new(SystemClock::default()),
        }
    }

    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }
    pub fn with_config(mut self, config: HttpConfig) -> Self {
        self.config = config;
        self
    }
    pub fn with_provider_name(mut self, name: impl Into<String>) -> Self {
        self.provider_name = name.into();
        self
    }
    pub fn with_rate_limiter(mut self, limiter: std::sync::Arc<RateLimiter>) -> Self {
        self.rate_limiter = Some(limiter);
        self
    }
    pub fn with_clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// 取回响应体。任何失败都是分类过的 [`ProviderError`]。
    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>, ProviderError> {
        self.get_bytes_with(url, &[])
    }

    pub fn get_bytes_with(
        &self,
        url: &str,
        extra_headers: &[(String, String)],
    ) -> Result<Vec<u8>, ProviderError> {
        let mut headers = vec![("User-Agent".to_string(), self.user_agent.clone())];
        headers.extend_from_slice(extra_headers);

        let mut delay = self.config.backoff;
        let mut last: Option<ProviderError> = None;

        for attempt in 0..=self.config.retries {
            if let Some(limiter) = &self.rate_limiter {
                limiter.acquire(self.clock.as_ref());
            }
            match self.transport.get(
                url,
                &headers,
                Timeouts::new(self.config.connect_timeout, self.config.timeout),
            ) {
                Ok(response) if (200..300).contains(&response.status) => return Ok(response.body),
                Ok(response) => {
                    // **必须把响应头传下去**：429/503 的 Retry-After 就在里面，
                    // 丢了它就只能用自己的退避节奏，会被 provider 继续拒。
                    last = Some(self.classify_status(response.status, &response.headers));
                }
                Err(err) => last = Some(err),
            }

            let err = last.as_ref().expect("循环里一定设过");
            if attempt >= self.config.retries || !err.retryable() {
                break;
            }
            let wait = err
                .retry_after
                .map(Duration::from_secs_f64)
                .unwrap_or(delay)
                .min(self.config.max_retry_after);
            self.clock.sleep(wait);
            delay = delay.mul_f64(self.config.backoff_factor);
        }

        Err(last
            .unwrap_or_else(|| ProviderError::network("未预期的失败"))
            .with_provider(self.provider_name.clone()))
    }

    /// POST 一个 JSON，收一个 JSON。
    ///
    /// 和 [`get_bytes_with`](Self::get_bytes_with) 共用同一套重试和分类，
    /// 但**不走限流器**——AnkiConnect 在本机，没有配额可言。
    pub fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ProviderError> {
        let raw = serde_json::to_vec(body).map_err(|e| {
            ProviderError::invalid_response(format!("请求体序列化失败: {e}"))
                .with_provider(self.provider_name.clone())
        })?;
        let headers = vec![
            ("User-Agent".to_string(), self.user_agent.clone()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let timeouts = Timeouts::new(self.config.connect_timeout, self.config.timeout);

        let mut delay = self.config.backoff;
        let mut last: Option<ProviderError> = None;
        for attempt in 0..=self.config.retries {
            match self.transport.post(url, &raw, &headers, timeouts) {
                Ok(response) if (200..300).contains(&response.status) => {
                    return serde_json::from_slice(&response.body).map_err(|e| {
                        ProviderError::invalid_response(format!("响应不是合法 JSON: {e}"))
                            .with_provider(self.provider_name.clone())
                    });
                }
                Ok(response) => {
                    last = Some(self.classify_status(response.status, &response.headers));
                }
                Err(err) => last = Some(err),
            }
            let err = last.as_ref().expect("循环里一定设过");
            if attempt >= self.config.retries || !err.retryable() {
                break;
            }
            self.clock.sleep(delay.min(self.config.max_retry_after));
            delay = delay.mul_f64(self.config.backoff_factor);
        }
        Err(last
            .unwrap_or_else(|| ProviderError::network("未预期的失败"))
            .with_provider(self.provider_name.clone()))
    }

    pub fn get_json(&self, url: &str) -> Result<serde_json::Value, ProviderError> {
        let raw = self.get_bytes(url)?;
        serde_json::from_slice(&raw).map_err(|e| {
            ProviderError::invalid_response(format!("响应不是合法 JSON: {e}"))
                .with_provider(self.provider_name.clone())
        })
    }

    /// 拼查询串。空值不带上——provider 对空参数的处理各不相同。
    pub fn build_url(&self, base: &str, params: &[(&str, String)]) -> String {
        let query: Vec<String> = params
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
            .collect();
        if query.is_empty() {
            base.to_string()
        } else {
            format!("{base}?{}", query.join("&"))
        }
    }

    fn classify_status(&self, status: u16, headers: &BTreeMap<String, String>) -> ProviderError {
        let retry_after = parse_retry_after(headers);
        match status {
            404 => ProviderError::not_found("404"),
            429 => ProviderError::rate_limited("被限流 (429)").with_retry_after(retry_after),
            // iTunes 限流时返回的是 403 + 空 body，既没有 429 也没有 Retry-After。
            // 这些接口都是免鉴权的公开搜索，403 实际上只可能是限流或地区封锁，
            // 不会是「请求写错了」。归成 INVALID_RESPONSE 的话它就不可重试，
            // 实测 209 首里有 70 首因此被判成永久失败。
            403 => ProviderError::rate_limited("被拒绝 (403)，多半是限流")
                .with_retry_after(retry_after),
            408 | 504 => ProviderError::timeout(format!("超时 ({status})")),
            s if s >= 500 => ProviderError::unavailable(format!("服务端错误 ({s})"))
                .with_retry_after(retry_after),
            s if s >= 400 => ProviderError::invalid_response(format!("请求被拒 ({s})")),
            s => ProviderError::invalid_response(format!("未预期的状态码 ({s})")),
        }
        .with_provider(self.provider_name.clone())
    }
}

fn parse_retry_after(headers: &BTreeMap<String, String>) -> Option<f64> {
    // HTTP-date 形式的 Retry-After 很少见，忽略即可
    headers
        .get("retry-after")?
        .trim()
        .parse::<f64>()
        .ok()
        .map(|v| v.max(0.0))
}

/// 百分号编码。只保留 RFC 3986 的 unreserved 字符。
fn urlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 按顺序吐出预设响应的假传输层。
    pub struct ScriptedTransport {
        steps: Mutex<std::collections::VecDeque<Result<HttpResponse, ProviderError>>>,
        pub calls: AtomicUsize,
        pub urls: Mutex<Vec<String>>,
    }

    impl ScriptedTransport {
        pub fn new(steps: Vec<Result<HttpResponse, ProviderError>>) -> Self {
            Self {
                steps: Mutex::new(steps.into()),
                calls: AtomicUsize::new(0),
                urls: Mutex::new(Vec::new()),
            }
        }

        pub fn ok(body: &str) -> Result<HttpResponse, ProviderError> {
            Ok(HttpResponse {
                status: 200,
                body: body.as_bytes().to_vec(),
                headers: BTreeMap::new(),
            })
        }

        pub fn status(code: u16, headers: &[(&str, &str)]) -> Result<HttpResponse, ProviderError> {
            Ok(HttpResponse {
                status: code,
                body: Vec::new(),
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
                    .collect(),
            })
        }
    }

    impl Transport for ScriptedTransport {
        fn get(
            &self,
            url: &str,
            _headers: &[(String, String)],
            _timeouts: Timeouts,
        ) -> Result<HttpResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.urls.lock().unwrap().push(url.to_string());
            self.steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(ProviderError::network("脚本用完了")))
        }
    }

    /// 不真的等待的时钟。记录每次 sleep，并把假时间往前推。
    #[derive(Default)]
    pub struct FakeClock {
        now: Mutex<Duration>,
        pub slept: Mutex<Vec<Duration>>,
    }

    impl FakeClock {
        pub fn total_slept(&self) -> Duration {
            self.slept.lock().unwrap().iter().sum()
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            *self.now.lock().unwrap()
        }
        fn sleep(&self, duration: Duration) {
            self.slept.lock().unwrap().push(duration);
            *self.now.lock().unwrap() += duration;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use std::sync::Arc;

    fn client(steps: Vec<Result<HttpResponse, ProviderError>>) -> (HttpClient, Arc<FakeClock>) {
        let clock = Arc::new(FakeClock::default());
        let c = HttpClient::new(Box::new(ScriptedTransport::new(steps)))
            .with_provider_name("test")
            .with_config(HttpConfig {
                backoff: Duration::from_millis(600),
                ..Default::default()
            });
        // Box<dyn Clock> 要独立一份，测试拿 Arc 观察
        struct Shared(Arc<FakeClock>);
        impl Clock for Shared {
            fn now(&self) -> Duration {
                self.0.now()
            }
            fn sleep(&self, d: Duration) {
                self.0.sleep(d)
            }
        }
        (c.with_clock(Box::new(Shared(clock.clone()))), clock)
    }

    #[test]
    fn a_successful_response_comes_back_as_bytes() {
        let (c, _) = client(vec![ScriptedTransport::ok("hello")]);
        assert_eq!(c.get_bytes("https://x").unwrap(), b"hello");
    }

    #[test]
    fn a_404_is_not_retried() {
        // 「这条记录不存在」重试没有意义
        let (c, clock) = client(vec![
            ScriptedTransport::status(404, &[]),
            ScriptedTransport::ok("late"),
        ]);
        let err = c.get_bytes("https://x").unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::NotFound);
        assert_eq!(clock.total_slept(), Duration::ZERO, "不该等待");
    }

    #[test]
    fn a_403_is_treated_as_throttling_and_retried() {
        // 这条钉住的是一个真实事故：iTunes 限流返回 403 + 空 body，
        // 归成不可重试的话 209 首里有 70 首会被判成永久失败。
        let (c, _) = client(vec![
            ScriptedTransport::status(403, &[]),
            ScriptedTransport::ok("ok"),
        ]);
        assert_eq!(c.get_bytes("https://x").unwrap(), b"ok");
    }

    #[test]
    fn retry_after_is_honoured_not_dropped() {
        // 这条钉住的另一个真实 bug：分类时没把响应头传下去，
        // Retry-After 被丢掉，只能用自己的退避节奏，继续被拒。
        let (c, clock) = client(vec![
            ScriptedTransport::status(429, &[("Retry-After", "5")]),
            ScriptedTransport::ok("ok"),
        ]);
        assert_eq!(c.get_bytes("https://x").unwrap(), b"ok");
        assert_eq!(
            clock.slept.lock().unwrap().as_slice(),
            &[Duration::from_secs(5)]
        );
    }

    #[test]
    fn an_absurd_retry_after_is_capped() {
        let (c, clock) = client(vec![
            ScriptedTransport::status(429, &[("Retry-After", "3600")]),
            ScriptedTransport::ok("ok"),
        ]);
        c.get_bytes("https://x").unwrap();
        assert_eq!(clock.total_slept(), Duration::from_secs(30));
    }

    #[test]
    fn backoff_grows_between_retries() {
        let (c, clock) = client(vec![
            ScriptedTransport::status(500, &[]),
            ScriptedTransport::status(500, &[]),
            ScriptedTransport::status(500, &[]),
        ]);
        let err = c.get_bytes("https://x").unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::Unavailable);
        let slept = clock.slept.lock().unwrap().clone();
        assert_eq!(
            slept,
            vec![Duration::from_millis(600), Duration::from_millis(1200)],
            "重试两次，退避翻倍"
        );
    }

    #[test]
    fn a_rate_limiter_does_not_throttle_the_first_request() {
        // 用 0 当「还没发过」的哨兵会让第一次请求白等一个间隔。
        // 真时钟上这个 bug 只在刚开机时暴露——假时钟从 0 开始，必然暴露。
        let limiter = RateLimiter::new(Duration::from_millis(1100));
        let clock = FakeClock::default();
        assert_eq!(limiter.acquire(&clock), Duration::ZERO);
        assert_eq!(clock.total_slept(), Duration::ZERO);
    }

    #[test]
    fn a_rate_limiter_spaces_out_later_requests() {
        let limiter = RateLimiter::new(Duration::from_millis(1100));
        let clock = FakeClock::default();
        limiter.acquire(&clock);
        let waited = limiter.acquire(&clock);
        assert_eq!(waited, Duration::from_millis(1100));
        // 再来一次：时间已经推进了，还要再等一个间隔
        let waited = limiter.acquire(&clock);
        assert_eq!(waited, Duration::from_millis(1100));
    }

    #[test]
    fn a_zero_interval_limiter_never_waits() {
        let limiter = RateLimiter::new(Duration::ZERO);
        let clock = FakeClock::default();
        for _ in 0..5 {
            assert_eq!(limiter.acquire(&clock), Duration::ZERO);
        }
    }

    #[test]
    fn bad_json_is_an_invalid_response_not_a_panic() {
        let (c, _) = client(vec![ScriptedTransport::ok("{not json")]);
        let err = c.get_json("https://x").unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::InvalidResponse);
    }

    #[test]
    fn build_url_skips_empty_params_and_encodes() {
        let (c, _) = client(vec![]);
        let url = c.build_url(
            "https://x/search",
            &[
                ("term", "夜に駆ける".to_string()),
                ("country", "JP".to_string()),
                ("empty", String::new()),
            ],
        );
        assert!(url.starts_with("https://x/search?term=%E5%A4%9C"), "{url}");
        assert!(url.contains("country=JP"));
        assert!(!url.contains("empty"), "空参数不该带上：{url}");
    }

    #[test]
    fn the_provider_name_is_attached_to_every_error() {
        let (c, _) = client(vec![ScriptedTransport::status(404, &[])]);
        assert_eq!(c.get_bytes("https://x").unwrap_err().provider, "test");
    }
}
