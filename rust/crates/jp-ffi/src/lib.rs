//! 给原生 macOS 客户端（SwiftUI）用的 C ABI。
//!
//! ## 为什么是「一个入口 + JSON」
//!
//! 业务逻辑全在 `jp-corpus`，Tauri 客户端经 command 调它，这里给 Swift 开同样的口子。
//! 没有逐个函数导出 C 签名，也没用 UniFFI 之类的代码生成：
//!
//! - **两个客户端看到的是同一份数据形状。** 参数和返回值都是 serde 序列化的 JSON，
//!   字段名就是 Tauri 前端用的那套 camelCase。Rust 结构体加一个字段，两边自动都有，
//!   不会出现「Tauri 有、Swift 的绑定忘了重新生成」。
//! - **C 层只有五个函数，几乎不会变。** 新功能只在 [`dispatch`] 里加一个分支，
//!   头文件、Swift 的模块映射都不用动。
//! - **代价**是每次调用多一次 JSON 编解码。语料库查询本身是毫秒到几十毫秒，
//!   编解码是微秒级，可以忽略；音频播放不走这里（macOS 用 AVFoundation 原生播放）。
//!
//! ## 约定
//!
//! - 所有字符串都是 UTF-8、以 NUL 结尾。Rust 返回的字符串由调用方用 [`jp_string_free`] 释放。
//! - [`jp_call`] 返回 `{"ok": <结果>}` 或 `{"error": "<一句人话>"}`，**从不返回 NULL**（内存分配失败除外）。
//! - 一个会话一个 SQLite 连接，内部加锁；Swift 可以从任意线程调，但同一会话的调用是串行的。
//! - panic 不会跨过 FFI 边界：被接住，变成一条 error。
//!
//! 头文件在 `include/jp_ffi.h`，手写，和这里的五个函数一一对应。

use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow};
use jp_corpus::Corpus;
use serde::Deserialize;
use serde_json::{Value, json};

/// 一个打开的语料库。对 C 来说是不透明指针。
pub struct Session {
    corpus: Mutex<Corpus>,
    /// 语料库目录（`corpus.db` 所在的目录）。相对路径和按文件名找音频都从这里开始
    root: Option<PathBuf>,
    /// 用户另外指定的音频目录。库从别的机器拷来时，音频的绝对路径在这台机器上不存在
    audio_roots: Mutex<Vec<PathBuf>>,
}

impl Session {
    /// 打开（必要时迁移）一个语料库。和 Tauri 客户端启动时做的一样：
    /// 可写打开、补齐结构、检查结构；比本程序新的库直接拒绝（见 `jp_corpus::migrations`）。
    pub fn open(db_path: &str) -> Result<Self> {
        let path = PathBuf::from(db_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let corpus = Corpus::open_writable(&path).with_context(|| format!("打不开 {}", path.display()))?;
        corpus.migrate().with_context(|| format!("{} 的结构升不上来", path.display()))?;
        corpus.check_schema()?;
        Ok(Self {
            corpus: Mutex::new(corpus),
            root: path.parent().map(PathBuf::from),
            audio_roots: Mutex::new(Vec::new()),
        })
    }

    /// 调一个方法。参数和返回值都是 JSON。纯 Rust，测试直接调它
    pub fn call(&self, method: &str, args: Value) -> Result<Value> {
        dispatch(self, method, args)
    }
}

fn arg<T: for<'de> Deserialize<'de>>(args: &Value, key: &str) -> Result<T> {
    let value = args.get(key).cloned().unwrap_or(Value::Null);
    serde_json::from_value(value).with_context(|| format!("参数 {key} 不对"))
}

fn arg_or<T: for<'de> Deserialize<'de>>(args: &Value, key: &str, default: T) -> Result<T> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => serde_json::from_value(v.clone()).with_context(|| format!("参数 {key} 不对")),
    }
}

/// 所有方法。名字和 Tauri command 对齐（camelCase），参数名也一样
fn dispatch(session: &Session, method: &str, args: Value) -> Result<Value> {
    let corpus = session.corpus.lock().map_err(|_| anyhow!("语料库连接已损坏"))?;
    let v = match method {
        "version" => json!({ "core": env!("CARGO_PKG_VERSION"), "schema": jp_corpus::migrations::CURRENT }),
        "health" => {
            let overview = corpus.overview()?;
            json!({
                "tracks": overview.tracks,
                "lyricLines": overview.lyric_lines,
                "tokens": overview.tokens,
                "root": session.root.as_ref().map(|p| p.display().to_string()),
            })
        }
        "overview" => serde_json::to_value(corpus.overview()?)?,
        "snapshot" => serde_json::to_value(corpus.snapshot()?)?,

        // 曲库
        "tracks" => serde_json::to_value(corpus.tracks(arg_or(&args, "limit", 100_000)?)?)?,
        "track" => serde_json::to_value(corpus.track(&arg::<String>(&args, "songId")?)?)?,
        "lyrics" => serde_json::to_value(corpus.lyrics(&arg::<String>(&args, "songId")?)?)?,
        "trackCredits" => serde_json::to_value(corpus.credits_for_track(&arg::<String>(&args, "songId")?)?)?,
        "performers" => serde_json::to_value(corpus.people_by_role("performer", arg_or(&args, "limit", 500)?)?)?,

        // 检索
        "kwic" => serde_json::to_value(corpus.kwic(&arg(&args, "query")?)?)?,
        "searchLyrics" => serde_json::to_value(
            corpus.search_lyrics(&arg::<String>(&args, "text")?, arg_or(&args, "limit", 500)?)?,
        )?,
        "quickSearch" => serde_json::to_value(
            corpus.quick_search(&arg::<String>(&args, "query")?, arg_or(&args, "perKind", None)?)?,
        )?,
        "wordInCorpus" => serde_json::to_value(corpus.word_in_corpus(
            &arg::<String>(&args, "lemma")?,
            arg_or::<Option<String>>(&args, "currentSongId", None)?.as_deref(),
            arg_or(&args, "limit", 24)?,
        )?)?,

        // 定位：检索结果 ↔ 音频
        "lineSpans" => serde_json::to_value(corpus.line_spans(&arg::<String>(&args, "songId")?)?)?,
        "listenWindow" => serde_json::to_value(corpus.listen_window(
            arg(&args, "utteranceId")?,
            arg_or(&args, "before", 0)?,
            arg_or(&args, "after", 0)?,
        )?)?,

        // 分析
        "frequency" => serde_json::to_value(corpus.frequency(&arg(&args, "query")?)?)?,
        "collocations" => serde_json::to_value(corpus.collocations(&arg(&args, "query")?)?)?,
        "keyness" => serde_json::to_value(corpus.keyness(&arg(&args, "query")?)?)?,
        "songProfile" => serde_json::to_value(
            corpus.song_profile(&arg::<String>(&args, "songId")?, arg_or(&args, "limit", 12)?)?,
        )?,

        // 播放历史（和 Tauri 客户端写同一张表，首页「最近播放」两边通用）
        "recordPlay" => {
            corpus.record_play(&arg(&args, "event")?)?;
            Value::Null
        }

        // 媒体路径
        "setAudioRoots" => {
            let roots: Vec<String> = arg_or(&args, "roots", Vec::new())?;
            *session.audio_roots.lock().map_err(|_| anyhow!("锁坏了"))? = roots.into_iter().map(PathBuf::from).collect();
            Value::Null
        }
        "resolveMedia" => {
            let stored: String = arg(&args, "path")?;
            let roots = session.audio_roots.lock().map_err(|_| anyhow!("锁坏了"))?.clone();
            let found = jp_corpus::media::resolve(&stored, session.root.as_deref(), &roots);
            json!(found.map(|p| p.display().to_string()))
        }
        other => return Err(anyhow!("没有这个方法：{other}")),
    };
    Ok(v)
}

// ────────────────────────────── C ABI ──────────────────────────────

fn to_c(s: String) -> *mut c_char {
    // 字符串里不该有 NUL；万一有（歌词里的控制字符），替换掉而不是返回 NULL
    CString::new(s)
        .unwrap_or_else(|e| CString::new(e.into_vec().into_iter().filter(|b| *b != 0).collect::<Vec<u8>>()).unwrap_or_default())
        .into_raw()
}

/// # Safety
/// `ptr` 必须是 NULL 或一个以 NUL 结尾的有效字符串
unsafe fn from_c<'a>(ptr: *const c_char) -> Result<&'a str> {
    if ptr.is_null() {
        return Err(anyhow!("参数是空指针"));
    }
    // SAFETY: 调用方保证以 NUL 结尾且在调用期间有效
    unsafe { CStr::from_ptr(ptr) }.to_str().context("参数不是 UTF-8")
}

fn envelope(result: std::thread::Result<Result<Value>>) -> String {
    match result {
        Ok(Ok(value)) => json!({ "ok": value }).to_string(),
        Ok(Err(err)) => json!({ "error": format!("{err:#}") }).to_string(),
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "未知错误".into());
            json!({ "error": format!("内部错误：{msg}") }).to_string()
        }
    }
}

/// 打开语料库。失败返回 NULL，原因写进 `*out_error`（调用方用 [`jp_string_free`] 释放）。
///
/// # Safety
/// `db_path` 是以 NUL 结尾的 UTF-8；`out_error` 为 NULL 或指向可写的指针
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_open(db_path: *const c_char, out_error: *mut *mut c_char) -> *mut Session {
    let result = std::panic::catch_unwind(|| {
        // SAFETY: 见函数说明
        let path = unsafe { from_c(db_path) }?;
        Session::open(path)
    });
    match result {
        Ok(Ok(session)) => Box::into_raw(Box::new(session)),
        other => {
            if !out_error.is_null() {
                let message = match other {
                    Ok(Err(err)) => format!("{err:#}"),
                    _ => "打开语料库时内部出错".into(),
                };
                // SAFETY: 调用方保证 out_error 可写
                unsafe { *out_error = to_c(message) };
            }
            std::ptr::null_mut()
        }
    }
}

/// 调一个方法，返回 JSON 信封 `{"ok":…}` / `{"error":"…"}`。用 [`jp_string_free`] 释放。
///
/// # Safety
/// `session` 来自 [`jp_open`] 且未关闭；`method`、`args_json` 是以 NUL 结尾的 UTF-8（`args_json` 可为 NULL，等同 `{}`）
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_call(session: *const Session, method: *const c_char, args_json: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value> {
        if session.is_null() {
            return Err(anyhow!("语料库没打开"));
        }
        // SAFETY: 见函数说明
        let session = unsafe { &*session };
        let method = unsafe { from_c(method) }?;
        let args: Value = if args_json.is_null() {
            json!({})
        } else {
            serde_json::from_str(unsafe { from_c(args_json) }?).context("参数不是合法 JSON")?
        };
        session.call(method, args)
    }));
    to_c(envelope(result))
}

/// 关闭语料库。传 NULL 什么都不做。
///
/// # Safety
/// `session` 来自 [`jp_open`]，且之后不再使用
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_close(session: *mut Session) {
    if !session.is_null() {
        // SAFETY: 来自 Box::into_raw，只释放一次
        drop(unsafe { Box::from_raw(session) });
    }
}

/// 释放 Rust 返回的字符串。传 NULL 什么都不做。
///
/// # Safety
/// `s` 来自本库返回的字符串，且只释放一次
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: 来自 CString::into_raw
        drop(unsafe { CString::from_raw(s) });
    }
}

/// 在 `dir` 下写一个合成的小语料库（四首歌，见 `jp_corpus::fixture`），返回 `corpus.db` 的路径。
/// 给 Swift 侧的测试和「先看看效果」用；失败返回 NULL。
///
/// # Safety
/// `dir` 是以 NUL 结尾的 UTF-8
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_write_fixture(dir: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(|| -> Result<String> {
        // SAFETY: 见函数说明
        let dir = unsafe { from_c(dir) }?;
        Ok(jp_corpus::fixture::write_to(std::path::Path::new(dir))?.display().to_string())
    });
    match result {
        Ok(Ok(path)) => to_c(path),
        _ => std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (Session, PathBuf) {
        let dir = std::env::temp_dir().join(format!("jp-ffi-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = jp_corpus::fixture::write_to(&dir).unwrap();
        (Session::open(db.to_str().unwrap()).unwrap(), dir)
    }

    /// 每个测试一个目录。不用时间戳：Windows 上两个线程同一刻拿到的纳秒数会相同
    fn rand_suffix() -> usize {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    #[test]
    fn the_minimal_loop_search_kwic_song_position() {
        let (s, dir) = session();
        // 1. 检索
        let hits = s
            .call("kwic", json!({ "query": { "keywords": ["風"], "field": "lemma" } }))
            .unwrap();
        let hit = &hits[0];
        // 2. 命中带着歌、行、音频区间
        let song = hit["songId"].as_str().unwrap();
        let utterance = hit["utteranceId"].as_i64().unwrap();
        assert!(hit["endSec"].as_f64().unwrap() > hit["timeSec"].as_f64().unwrap());
        // 3. 跳到歌：歌词里有这一行
        let lyrics = s.call("lyrics", json!({ "songId": song })).unwrap();
        assert!(lyrics.as_array().unwrap().iter().any(|l| l["utteranceId"].as_i64() == Some(utterance)));
        // 4. 定位：试听区间包住这一行
        let w = s.call("listenWindow", json!({ "utteranceId": utterance, "before": 1, "after": 1 })).unwrap();
        assert_eq!(w["songId"].as_str(), Some(song));
        assert!(w["startSec"].as_f64().unwrap() <= w["focusStartSec"].as_f64().unwrap());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn analysis_methods_answer_and_unknown_methods_are_errors() {
        let (s, dir) = session();
        let f = s.call("frequency", json!({ "query": { "scope": { "unit": "distinctLines" }, "limit": 3 } })).unwrap();
        assert_eq!(f["items"].as_array().unwrap().len(), 3);
        let c = s.call("collocations", json!({ "query": { "node": "夜", "minFreq": 1 } })).unwrap();
        assert!(c["nodeFreq"].as_i64().unwrap() >= 2);
        let k = s.call("keyness", json!({ "query": { "target": { "songIds": ["001"] }, "minFreq": 1 } })).unwrap();
        assert!(k["over"].is_array());
        assert!(s.call("songProfile", json!({ "songId": "004" })).unwrap()["repeatedLines"].as_i64().unwrap() >= 1);
        assert!(s.call("noSuchThing", json!({})).is_err());
        assert!(s.call("lyrics", json!({})).is_err(), "缺参数要报错而不是 panic");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_c_abi_round_trips_and_reports_errors_as_json() {
        let dir = std::env::temp_dir().join(format!("jp-ffi-c-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir_c = CString::new(dir.to_str().unwrap()).unwrap();
        unsafe {
            let db = jp_write_fixture(dir_c.as_ptr());
            assert!(!db.is_null());
            let mut err: *mut c_char = std::ptr::null_mut();
            let s = jp_open(db, &mut err);
            assert!(!s.is_null() && err.is_null());
            jp_string_free(db);

            let method = CString::new("tracks").unwrap();
            let out = jp_call(s, method.as_ptr(), std::ptr::null());
            let v: Value = serde_json::from_str(CStr::from_ptr(out).to_str().unwrap()).unwrap();
            assert_eq!(v["ok"].as_array().unwrap().len() as i64, jp_corpus::fixture::TRACKS);
            jp_string_free(out);

            let bad = CString::new("{not json").unwrap();
            let out = jp_call(s, method.as_ptr(), bad.as_ptr());
            let v: Value = serde_json::from_str(CStr::from_ptr(out).to_str().unwrap()).unwrap();
            assert!(v["error"].as_str().unwrap().contains("JSON"));
            jp_string_free(out);
            jp_close(s);

            // 没打开的会话：不崩，报错
            let out = jp_call(std::ptr::null(), method.as_ptr(), std::ptr::null());
            let v: Value = serde_json::from_str(CStr::from_ptr(out).to_str().unwrap()).unwrap();
            assert!(v["error"].is_string());
            jp_string_free(out);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn audio_paths_from_another_machine_are_found_under_the_library_root() {
        let (s, dir) = session();
        let audio = dir.join("raw").join("audio").join("歌手");
        std::fs::create_dir_all(&audio).unwrap();
        std::fs::write(audio.join("x.flac"), b"x").unwrap();
        let found = s.call("resolveMedia", json!({ "path": r"E:\cloud music\x.flac" })).unwrap();
        assert_eq!(found.as_str().map(PathBuf::from), Some(audio.join("x.flac")));
        std::fs::remove_dir_all(dir).ok();
    }
}
