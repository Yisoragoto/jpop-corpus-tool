//! AnkiConnect 客户端。
//!
//! AnkiConnect 是 Anki 的一个插件，在 `127.0.0.1:8765` 上开一个 HTTP 接口。
//! 它只收 POST，请求和响应都是 JSON：
//!
//! ```json
//! {"action": "deckNames", "version": 6, "params": {}}
//! → {"result": ["Default", "日本語"], "error": null}
//! ```
//!
//! **错误要分成两类，因为用户能做的事完全不同：**
//!
//! * 连不上 → Anki 没开，或者没装 AnkiConnect 插件。这条要说人话。
//! * 连上了但 `error` 非空 → 请求本身有问题（牌组不存在、重复卡片……），
//!   照原样报出来，那是 Anki 自己的措辞，比我转述准。

use jp_scraper::http::{HttpClient, HttpConfig};
use jp_scraper::{ErrorType, ProviderError};
use serde_json::{Value, json};

pub const DEFAULT_URL: &str = "http://127.0.0.1:8765";
pub const API_VERSION: u32 = 6;

/// AnkiConnect 的一次调用失败。
#[derive(Debug, Clone, PartialEq)]
pub enum AnkiError {
    /// 连不上：Anki 没开，或者没装 AnkiConnect。
    NotRunning(String),
    /// 连上了，但 Anki 说这个请求不行。原文照抄。
    Rejected(String),
    /// 响应结构不对。
    BadResponse(String),
}

impl AnkiError {
    /// 给用户看的一句话，带「该怎么办」。
    pub fn advice(&self) -> String {
        match self {
            Self::NotRunning(detail) => format!(
                "连不上 Anki（{detail}）。\
                 请确认 Anki 已经打开，并且装了 AnkiConnect 插件（代码 2055492159）。"
            ),
            Self::Rejected(message) => message.clone(),
            Self::BadResponse(detail) => format!("AnkiConnect 返回了看不懂的东西：{detail}"),
        }
    }

    /// 是不是「Anki 没开」这一类。UI 据此决定要不要显示安装指引。
    pub fn is_not_running(&self) -> bool {
        matches!(self, Self::NotRunning(_))
    }
}

impl std::fmt::Display for AnkiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.advice())
    }
}

impl std::error::Error for AnkiError {}

impl From<ProviderError> for AnkiError {
    fn from(err: ProviderError) -> Self {
        match err.error_type {
            // 本机服务连不上、超时，都是「Anki 没开」
            ErrorType::Network | ErrorType::Timeout | ErrorType::Unavailable => {
                Self::NotRunning(err.message)
            }
            ErrorType::InvalidResponse => Self::BadResponse(err.message),
            _ => Self::NotRunning(err.message),
        }
    }
}

pub type Result<T> = std::result::Result<T, AnkiError>;

/// AnkiConnect 客户端。
pub struct AnkiConnect {
    url: String,
    http: HttpClient,
}

impl AnkiConnect {
    pub fn new(http: HttpClient) -> Self {
        Self {
            url: DEFAULT_URL.to_string(),
            // 本机服务：连不上就是连不上，没必要退避重试半天
            http: http.with_provider_name("anki").with_config(HttpConfig {
                timeout: std::time::Duration::from_secs(30),
                connect_timeout: std::time::Duration::from_secs(3),
                retries: 0,
                ..Default::default()
            }),
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// 调一个 action，返回 `result` 字段。
    pub fn call(&self, action: &str, params: Value) -> Result<Value> {
        let payload = json!({
            "action": action,
            "version": API_VERSION,
            "params": params,
        });
        let response = self.http.post_json(&self.url, &payload)?;
        let object = response
            .as_object()
            .ok_or_else(|| AnkiError::BadResponse("响应不是 JSON 对象".into()))?;

        // AnkiConnect 的约定：出错时 error 是字符串，成功时是 null
        match object.get("error") {
            Some(Value::String(message)) if !message.is_empty() => {
                Err(AnkiError::Rejected(message.clone()))
            }
            _ => Ok(object.get("result").cloned().unwrap_or(Value::Null)),
        }
    }

    /// Anki 在不在。UI 一进页面就问这个。
    pub fn ping(&self) -> Result<u32> {
        let version = self.call("version", json!({}))?;
        version
            .as_u64()
            .map(|v| v as u32)
            .ok_or_else(|| AnkiError::BadResponse(format!("version 不是数字：{version}")))
    }

    pub fn deck_names(&self) -> Result<Vec<String>> {
        let value = self.call("deckNames", json!({}))?;
        Ok(as_string_list(&value))
    }

    pub fn model_names(&self) -> Result<Vec<String>> {
        let value = self.call("modelNames", json!({}))?;
        Ok(as_string_list(&value))
    }

    pub fn model_field_names(&self, model: &str) -> Result<Vec<String>> {
        let value = self.call("modelFieldNames", json!({ "modelName": model }))?;
        Ok(as_string_list(&value))
    }

    /// 查笔记 id。查询语法就是 Anki 搜索栏那套。
    pub fn find_notes(&self, query: &str) -> Result<Vec<i64>> {
        let value = self.call("findNotes", json!({ "query": query }))?;
        Ok(value
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
            .unwrap_or_default())
    }

    /// 取笔记详情。返回 (note_id, 字段名 → 值)。
    pub fn notes_info(&self, ids: &[i64]) -> Result<Vec<(i64, std::collections::BTreeMap<String, String>)>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let value = self.call("notesInfo", json!({ "notes": ids }))?;
        let mut out = Vec::new();
        for item in value.as_array().unwrap_or(&Vec::new()) {
            let Some(object) = item.as_object() else { continue };
            let id = object.get("noteId").and_then(|v| v.as_i64()).unwrap_or(0);
            let mut fields = std::collections::BTreeMap::new();
            if let Some(map) = object.get("fields").and_then(|v| v.as_object()) {
                for (name, slot) in map {
                    let text = slot
                        .get("value")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    fields.insert(name.clone(), text);
                }
            }
            out.push((id, fields));
        }
        Ok(out)
    }

    /// 查卡片 id。查询语法就是 Anki 搜索栏那套。
    pub fn find_cards(&self, query: &str) -> Result<Vec<i64>> {
        let value = self.call("findCards", json!({ "query": query }))?;
        Ok(value
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
            .unwrap_or_default())
    }

    /// 卡片属于哪张笔记、在哪个牌组。返回 (card_id, note_id, 牌组名)。
    pub fn cards_info(&self, ids: &[i64]) -> Result<Vec<(i64, i64, String)>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let value = self.call("cardsInfo", json!({ "cards": ids }))?;
        Ok(value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let object = item.as_object()?;
                        let card = object.get("cardId").and_then(Value::as_i64).unwrap_or(0);
                        let note = object
                            .get("note")
                            .or_else(|| object.get("noteId"))
                            .and_then(Value::as_i64)
                            .unwrap_or(0);
                        let deck = object
                            .get("deckName")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        Some((card, note, deck))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// 这张笔记能不能加（主要是查重）。Anki 没给出明确答案时返回 true，交给 addNote 决定。
    pub fn can_add_note(&self, note: &Value) -> Result<bool> {
        let value = self.call("canAddNotes", json!({ "notes": [note] }))?;
        Ok(value
            .as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_bool)
            .unwrap_or(true))
    }

    /// 把一个媒体文件放进 Anki 的媒体文件夹，返回**实际存下的文件名**。卡片里必须用这个名字。
    ///
    /// **`deleteExisting: false`**：AnkiConnect 默认是 true，同名就先删再写——
    /// 别的卡片引用着的那个文件会悄悄换成另一段内容。传 false 之后，Anki 遇到同名
    /// 不同内容会改名再存（`MediaManager.write_data`：「renaming if not unique.
    /// Returns possibly-renamed filename」），内容相同则直接复用。
    /// 老版本 AnkiConnect 不回名字时，就当它用了我们要的名字。
    pub fn store_media_file(&self, filename: &str, bytes: &[u8]) -> Result<String> {
        use base64::Engine as _;
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        let stored = self.call(
            "storeMediaFile",
            json!({ "filename": filename, "data": data, "deleteExisting": false }),
        )?;
        Ok(stored
            .as_str()
            .filter(|name| !name.is_empty())
            .unwrap_or(filename)
            .to_string())
    }

    pub fn update_note_fields(
        &self,
        note_id: i64,
        fields: &std::collections::BTreeMap<String, String>,
    ) -> Result<()> {
        self.call(
            "updateNoteFields",
            json!({ "note": { "id": note_id, "fields": fields } }),
        )?;
        Ok(())
    }
}

fn as_string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use jp_scraper::http::{HttpResponse, Timeouts, Transport};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// 按 action 名回应的假 AnkiConnect。
    pub struct FakeAnki {
        /// action → 要回的 `result`
        pub results: Mutex<BTreeMap<String, Value>>,
        /// action → 要回的 `error`
        pub errors: Mutex<BTreeMap<String, String>>,
        /// 收到过的 (action, params)，测试用来断言发了什么
        pub calls: Mutex<Vec<(String, Value)>>,
        /// 连不上时设成 true
        pub offline: bool,
    }

    impl FakeAnki {
        pub fn new(results: &[(&str, Value)]) -> Self {
            Self {
                results: Mutex::new(
                    results
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.clone()))
                        .collect(),
                ),
                errors: Mutex::new(BTreeMap::new()),
                calls: Mutex::new(Vec::new()),
                offline: false,
            }
        }

        pub fn offline() -> Self {
            let mut fake = Self::new(&[]);
            fake.offline = true;
            fake
        }

        pub fn with_error(self, action: &str, message: &str) -> Self {
            self.errors
                .lock()
                .unwrap()
                .insert(action.to_string(), message.to_string());
            self
        }

        pub fn actions(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|(a, _)| a.clone())
                .collect()
        }

        /// 某个 action 收到过的所有参数，按调用顺序
        pub fn all_params_of(&self, action: &str) -> Vec<Value> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(a, _)| a == action)
                .map(|(_, p)| p.clone())
                .collect()
        }

        pub fn params_of(&self, action: &str) -> Option<Value> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .find(|(a, _)| a == action)
                .map(|(_, p)| p.clone())
        }
    }

    /// 包成 AnkiConnect；测试里留着 Arc，事后看它收到了什么。
    pub fn client(fake: std::sync::Arc<FakeAnki>) -> AnkiConnect {
        struct Shared(std::sync::Arc<FakeAnki>);
        impl Transport for Shared {
            fn get(
                &self,
                url: &str,
                headers: &[(String, String)],
                timeouts: Timeouts,
            ) -> std::result::Result<HttpResponse, ProviderError> {
                self.0.get(url, headers, timeouts)
            }
            fn post(
                &self,
                url: &str,
                body: &[u8],
                headers: &[(String, String)],
                timeouts: Timeouts,
            ) -> std::result::Result<HttpResponse, ProviderError> {
                self.0.post(url, body, headers, timeouts)
            }
        }
        AnkiConnect::new(jp_scraper::http::HttpClient::new(Box::new(Shared(fake))))
    }

    impl Transport for FakeAnki {
        fn get(
            &self,
            _url: &str,
            _headers: &[(String, String)],
            _timeouts: Timeouts,
        ) -> std::result::Result<HttpResponse, ProviderError> {
            Err(ProviderError::network("AnkiConnect 只收 POST"))
        }

        fn post(
            &self,
            _url: &str,
            body: &[u8],
            _headers: &[(String, String)],
            _timeouts: Timeouts,
        ) -> std::result::Result<HttpResponse, ProviderError> {
            if self.offline {
                return Err(ProviderError::network("连接被拒绝"));
            }
            let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            let action = request
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let params = request.get("params").cloned().unwrap_or(Value::Null);
            self.calls.lock().unwrap().push((action.clone(), params));

            let response = if let Some(message) = self.errors.lock().unwrap().get(&action) {
                json!({ "result": null, "error": message })
            } else {
                let result = self
                    .results
                    .lock()
                    .unwrap()
                    .get(&action)
                    .cloned()
                    .unwrap_or(Value::Null);
                json!({ "result": result, "error": null })
            };
            Ok(HttpResponse {
                status: 200,
                body: serde_json::to_vec(&response).unwrap(),
                headers: BTreeMap::new(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::FakeAnki;
    use super::*;
    use std::sync::Arc;

    fn client(fake: Arc<FakeAnki>) -> AnkiConnect {
        struct Shared(Arc<FakeAnki>);
        impl jp_scraper::http::Transport for Shared {
            fn get(
                &self,
                u: &str,
                h: &[(String, String)],
                t: jp_scraper::http::Timeouts,
            ) -> std::result::Result<jp_scraper::http::HttpResponse, ProviderError> {
                self.0.get(u, h, t)
            }
            fn post(
                &self,
                u: &str,
                b: &[u8],
                h: &[(String, String)],
                t: jp_scraper::http::Timeouts,
            ) -> std::result::Result<jp_scraper::http::HttpResponse, ProviderError> {
                self.0.post(u, b, h, t)
            }
        }
        AnkiConnect::new(HttpClient::new(Box::new(Shared(fake))))
    }

    #[test]
    fn a_successful_call_returns_the_result_field() {
        let fake = Arc::new(FakeAnki::new(&[("version", json!(6))]));
        assert_eq!(client(fake).ping().unwrap(), 6);
    }

    /// 不许覆盖 Anki 里已有的同名文件：AnkiConnect 的 deleteExisting 默认是 true，
    /// 同名就先删再写——别的卡片引用着的那个文件会悄悄换成另一段内容。
    /// 传 false 之后，Anki 遇到同名不同内容会改名并把实际的名字返回来，必须用那个名字。
    #[test]
    fn storing_media_never_replaces_an_existing_file_and_uses_the_name_anki_chose() {
        let fake = Arc::new(FakeAnki::new(&[("storeMediaFile", json!("jpop_clip_ab-1.mp3"))]));
        let stored = client(fake.clone()).store_media_file("jpop_clip_ab.mp3", b"mp3").unwrap();
        assert_eq!(stored, "jpop_clip_ab-1.mp3", "Anki 改了名，要用它给的名字");
        let params = fake.params_of("storeMediaFile").unwrap();
        assert_eq!(params["deleteExisting"], json!(false), "{params}");
        assert_eq!(params["filename"], "jpop_clip_ab.mp3");

        // 老版本的 AnkiConnect 不回名字（null）：就用我们要的那个
        let quiet = Arc::new(FakeAnki::new(&[("storeMediaFile", json!(null))]));
        assert_eq!(client(quiet).store_media_file("x.mp3", b"mp3").unwrap(), "x.mp3");
    }

    #[test]
    fn the_request_shape_matches_ankiconnect() {
        // action / version / params 三件套，版本号写错整个接口不认
        let fake = Arc::new(FakeAnki::new(&[("deckNames", json!(["Default"]))]));
        client(fake.clone()).deck_names().unwrap();
        assert_eq!(fake.actions(), vec!["deckNames"]);
    }

    #[test]
    fn anki_not_running_says_what_to_do() {
        // 「连不上」和「请求不对」对用户是完全不同的两件事
        let err = client(Arc::new(FakeAnki::offline())).ping().unwrap_err();
        assert!(err.is_not_running(), "{err:?}");
        let advice = err.advice();
        assert!(advice.contains("Anki"), "{advice}");
        assert!(advice.contains("AnkiConnect"), "要提示装插件：{advice}");
    }

    #[test]
    fn an_anki_side_error_is_passed_through_verbatim() {
        // Anki 自己的措辞比我转述准
        let fake = Arc::new(FakeAnki::new(&[]).with_error("addNote", "cannot create note because it is a duplicate"));
        let err = client(fake).call("addNote", json!({})).unwrap_err();
        assert_eq!(
            err,
            AnkiError::Rejected("cannot create note because it is a duplicate".into())
        );
        assert!(!err.is_not_running());
    }

    #[test]
    fn a_null_error_field_is_not_an_error() {
        // AnkiConnect 成功时 error 是 null，不能当成失败
        let fake = Arc::new(FakeAnki::new(&[("deckNames", json!(["A", "B"]))]));
        assert_eq!(client(fake).deck_names().unwrap(), vec!["A", "B"]);
    }

    #[test]
    fn notes_info_flattens_the_nested_field_shape() {
        // AnkiConnect 的字段是 {"Expression": {"value": "...", "order": 0}}
        let fake = Arc::new(FakeAnki::new(&[(
            "notesInfo",
            json!([{
                "noteId": 1234,
                "fields": {
                    "Expression": {"value": "夜", "order": 0},
                    "Sentence": {"value": "<div>…</div>", "order": 3}
                }
            }]),
        )]));
        let notes = client(fake).notes_info(&[1234]).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, 1234);
        assert_eq!(notes[0].1.get("Expression").unwrap(), "夜");
    }

    #[test]
    fn an_empty_id_list_does_not_hit_the_network() {
        let fake = Arc::new(FakeAnki::new(&[]));
        assert!(client(fake.clone()).notes_info(&[]).unwrap().is_empty());
        assert!(fake.actions().is_empty());
    }
}
