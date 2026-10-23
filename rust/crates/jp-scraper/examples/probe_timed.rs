//! Public-track network diagnostic; no database, files, credential discovery,
//! lyric text, request bodies, headers or query strings in the output.
//! cargo run -p jp-scraper --example probe_timed --jobs 2 -- SOURCE [TITLE ARTIST [SECONDS]]
//! SOURCE: netease | qq | kugou | amll | lrclib | apple | apple-metadata | all
//! apple verifies the unsupported-lyrics boundary, without a request.
//! apple-metadata probes ONLY the official Catalog API WITHOUT credentials:
//! HTTP 401 is expected and does not validate authenticated catalog access.

#[cfg(feature = "network")]
mod diagnostic {
    use jp_scraper::http::{HttpResponse, Timeouts, Transport};
    use jp_scraper::timed_sources::{SourceOptions, search_with_transport};
    use jp_scraper::{ProviderError, UreqTransport};
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    type Requests = Arc<Mutex<Vec<Value>>>;
    struct Observed {
        requests: Requests,
    }
    impl Observed {
        fn record(
            &self,
            method: &str,
            url: &str,
            start: Instant,
            result: &Result<HttpResponse, ProviderError>,
        ) {
            let endpoint = url.split('?').next().unwrap_or(url);
            let value = match result {
                Ok(r) => {
                    json!({"method":method,"endpoint":endpoint,"status":r.status,"responseBytes":r.body.len(),"elapsedSec":start.elapsed().as_secs_f64()})
                }
                Err(e) => {
                    // Classify locally; never emit a raw transport error.
                    let message = e.message.to_ascii_lowercase();
                    let category = if ["tls", "certificate", "unknownissuer", "unknown issuer"]
                        .iter()
                        .any(|s| message.contains(s))
                    {
                        "tls"
                    } else if message.contains("timeout") || message.contains("超时") {
                        "timeout"
                    } else {
                        "transport"
                    };
                    json!({"method":method,"endpoint":endpoint,"errorType":format!("{:?}",e.error_type),"category":category,"elapsedSec":start.elapsed().as_secs_f64()})
                }
            };
            self.requests.lock().unwrap().push(value);
        }
    }
    impl Transport for Observed {
        fn get(
            &self,
            url: &str,
            headers: &[(String, String)],
            timeouts: Timeouts,
        ) -> Result<HttpResponse, ProviderError> {
            let start = Instant::now();
            let result = UreqTransport.get(url, headers, timeouts);
            self.record("GET", url, start, &result);
            result
        }
        fn post(
            &self,
            url: &str,
            body: &[u8],
            headers: &[(String, String)],
            timeouts: Timeouts,
        ) -> Result<HttpResponse, ProviderError> {
            let start = Instant::now();
            let result = UreqTransport.post(url, body, headers, timeouts);
            self.record("POST", url, start, &result);
            result
        }
    }

    fn options(source: &str) -> Result<SourceOptions, &'static str> {
        if !["all", "netease", "qq", "kugou", "amll", "lrclib", "apple"].contains(&source) {
            return Err(
                "unknown source; expected netease/qq/kugou/amll/lrclib/apple/apple-metadata/all",
            );
        }
        Ok(SourceOptions {
            netease: ["all", "netease"].contains(&source),
            qq: ["all", "qq"].contains(&source),
            kugou: ["all", "kugou"].contains(&source),
            amll: ["all", "amll"].contains(&source),
            lrclib: ["all", "lrclib"].contains(&source),
            apple: source == "apple",
            ..Default::default()
        })
    }
    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let source = args.first().map(String::as_str).unwrap_or("all");
        let title = args.get(1).map(String::as_str).unwrap_or("夜に駆ける");
        let artist = args.get(2).map(String::as_str).unwrap_or("YOASOBI");
        let duration = args.get(3).map(|s| s.parse::<f64>()).transpose()?;
        if duration.is_some_and(|d| !d.is_finite() || d <= 0.0) {
            return Err("duration must be finite positive seconds".into());
        }
        let requests = Requests::default();
        let observed = Observed {
            requests: requests.clone(),
        };
        let start = Instant::now();
        if source == "apple-metadata" {
            let _ = observed.get(
                "https://api.music.apple.com/v1/catalog/jp/search?term=YOASOBI&types=songs&limit=2",
                &[],
                Timeouts::new(Duration::from_secs(4), Duration::from_secs(8)),
            );
            println!(
                "{}",
                json!({"source":source,"elapsedSec":start.elapsed().as_secs_f64(),"authenticated":false,"expectedStatus":401,"note":"Official Catalog only; authenticated provider needs an explicitly configured developer token and was not tested.","requests":*requests.lock().unwrap()})
            );
            return Ok(());
        }
        let result = search_with_transport(
            title,
            artist,
            duration,
            &options(source)?,
            Box::new(observed),
        );
        let candidates:Vec<_>=result.candidates.iter().map(|c|json!({"source":c.source,"format":c.format,"lines":c.lyrics.lines.len(),"timedLines":c.lyrics.lines.iter().filter(|l|!l.words.is_empty()).count(),"words":c.lyrics.lines.iter().map(|l|l.words.len()).sum::<usize>()})).collect();
        let fallback:Vec<_>=result.fallback.iter().map(|c|json!({"source":c.source,"format":c.format,"mode":c.mode,"lines":c.raw_text.lines().count()})).collect();
        println!(
            "{}",
            json!({"source":source,"elapsedSec":start.elapsed().as_secs_f64(),"candidateCount":candidates.len(),"fallbackCount":fallback.len(),"candidates":candidates,"fallback":fallback,"warnings":result.warnings,"requests":*requests.lock().unwrap()})
        );
        Ok(())
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn single_source_probes_do_not_inherit_other_defaults() {
            for source in ["netease", "qq", "kugou", "amll", "lrclib", "apple"] {
                let o = options(source).unwrap();
                assert_eq!(
                    [o.netease, o.qq, o.kugou, o.amll, o.lrclib, o.apple]
                        .iter()
                        .filter(|&&b| b)
                        .count(),
                    1
                );
                assert_eq!(o.lrclib, source == "lrclib");
            }
            assert!(options("unknown").is_err());
        }
    }
}
#[cfg(feature = "network")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    diagnostic::run()
}
#[cfg(not(feature = "network"))]
fn main() {
    eprintln!("probe_timed requires the network feature");
    std::process::exit(2);
}
