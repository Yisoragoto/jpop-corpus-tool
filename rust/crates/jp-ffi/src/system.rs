//! 更新与诊断的 JSON 适配。
//!
//! 更新：`updateCheck` 记下这次查到的发布；`updateDownloadStart` 只下那一次检查里的安装包
//! （前端只给要下哪一版，用来核对），后台下载、校验 SHA-256，界面轮询 `updateDownloadProgress`；
//! `updateInstall` 只替换这个进程亲手下好、启动前再校验一遍的那个 dmg，拉起替换脚本后由 Swift 退出应用。
//! 这些状态跟着进程走、不跟着库走：中途换库不影响已经下好的包。
use super::*;
use std::sync::{Arc, OnceLock};

struct Updater {
    memory: jp_update::UpdateMemory,
    job: Arc<jobs::Job>,
}

fn updater() -> &'static Updater {
    static UPDATER: OnceLock<Updater> = OnceLock::new();
    UPDATER.get_or_init(|| Updater { memory: jp_update::UpdateMemory::default(), job: Arc::default() })
}

/// 安装包下到 `~/Library/Caches/JPOP Corpus/updates`
fn download_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Caches/JPOP Corpus/updates"))
        .unwrap_or_else(|| std::env::temp_dir().join("jpop-corpus-updates"))
}

pub fn dispatch(session: &Session, method: &str, args: &Value) -> Result<Option<Value>> {
    let version = arg_or(args, "appVersion", jp_update::CURRENT_VERSION.to_owned())?;
    let value = match method {
        "scrapeAttempts" => {
            let corpus = session.network_connection()?;
            let store = jp_scraper::ScrapeStore::new(corpus.connection());
            store.ensure_schema()?;
            serde_json::to_value(store.attempts(&arg::<String>(args, "filePath")?, arg_or(args, "limit", 20)?)?)?
        }
        "dictLegacySources" => {
            let titles = session.with_dictionaries(|store| Ok(store.dictionaries()?.into_iter().map(|d| d.title).collect::<Vec<_>>()))?.unwrap_or_default();
            serde_json::to_value(jp_dict::legacy::legacy_sources(&session.db_path, &titles)?)?
        }
        "dictMedia" => {
            let dictionary: String = arg(args, "dictionary")?;
            let path: String = arg(args, "path")?;
            json!(session.with_dictionaries(|store| {
                let Some(d) = store.dictionaries()?.into_iter().find(|d| d.title == dictionary) else { return Ok(Vec::new()) };
                Ok(store.media(d.id, &path)?.map(|m| m.data).unwrap_or_default())
            })?.unwrap_or_default())
        }
        "dictDefinitionHtml" => {
            let dictionary: String = arg(args, "dictionary")?;
            let entries: Vec<Value> = arg(args, "entries")?;
            json!(session.with_dictionaries(|store| jp_dict::display::definition_html(store, &dictionary, &entries))?.unwrap_or_default())
        }
        "dictStyles" => json!(session.with_dictionaries(|store| {
            store.dictionaries()?.into_iter().filter(|d| d.enabled && d.has_styles)
                .map(|d| Ok(json!({"title": d.title, "styles": store.styles(d.id)?}))).collect::<Result<Vec<_>>>()
        })?.unwrap_or_default()),
        "updatePreferences" => serde_json::to_value(jp_update::preferences(&version))?,
        "updateSetChannel" => {
            jp_corpus::update_settings::set_channel(arg(args, "channel")?)?;
            serde_json::to_value(jp_update::preferences(&version))?
        }
        "updateSkip" => {
            jp_corpus::update_settings::skip(arg(args, "channel")?, &arg::<String>(args, "id")?)?;
            serde_json::to_value(jp_update::preferences(&version))?
        }
        "updateCheck" => {
            let status = jp_update::check(&version)?;
            updater().memory.remember_check(&status);
            serde_json::to_value(status)?
        }
        "updateDownloadStart" => {
            let id: String = arg(args, "id")?;
            let asset = updater().memory.installer_for(&version, &id)?;
            let dir = download_dir();
            updater().job.spawn("更新下载", move |job| {
                let path = jp_update::download_installer(&asset, &dir, |done, total| job.report(done, total, "downloading"))?;
                let sha256 = asset.sha256.clone().context("安装包没有校验值")?;
                updater().memory.remember_download(path.clone(), sha256);
                Ok((json!(path), false))
            })?;
            Value::Null
        }
        "updateDownloadProgress" => serde_json::to_value(updater().job.view())?,
        "updateInstall" => {
            let app: String = arg(args, "appPath")?;
            let dmg = updater().memory.installer_to_launch()?;
            jp_update::launch_mac_installer(&dmg, Path::new(&app), std::process::id())?;
            Value::Null
        }
        "updateChangelog" => serde_json::to_value(jp_update::changelog(arg_or(args, "limit", 10)?)?)?,
        "updateReleasePage" => {
            let url: String = arg(args, "url")?;
            ensure!(jp_update::is_official(&url), "不是本仓库的官方发布页");
            json!(url)
        }
        "diagnosticsReport" | "diagnosticsSave" => {
            let root = session.library_root();
            let system: String = arg_or(args, "system", std::env::consts::OS.to_owned())?;
            let audio: bool = arg_or(args, "audioAvailable", false)?;
            let corpus = session.corpus.lock().map_err(|_| anyhow!("锁坏了"))?;
            let report = jp_diagnostics::report(&corpus, &root, &session.db_path, &version,
                jp_diagnostics::Capabilities { system: &system, audio, pitch: "AVAudioUnitTimePitch", pitch_cache: "不适用（原生实时变调）" }, 200);
            if method == "diagnosticsSave" {
                let path: String = arg(args, "path")?;
                std::fs::write(&path, report.as_bytes()).with_context(|| format!("写不进 {path}"))?;
                json!(path)
            } else { json!(report) }
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_and_official_page_are_offline_and_install_is_unavailable() {
        let dir = std::env::temp_dir().join(format!("jp-system-{}", std::process::id()));
        let db = jp_corpus::fixture::write_to(&dir).unwrap();
        let s = Session::open(db.to_str().unwrap()).unwrap();
        let args = json!({"appVersion":"9.8.7", "system":"macOS 14.5", "audioAvailable":true});
        let report = s.call("diagnosticsReport", args.clone()).unwrap();
        let text = report.as_str().unwrap();
        assert!(text.contains("9.8.7") && text.contains("macOS 14.5") && text.contains("AVAudioUnitTimePitch"));
        let path = dir.join("diagnostics.txt");
        let mut save = args; save["path"] = json!(path);
        s.call("diagnosticsSave", save).unwrap();
        assert!(std::fs::read_to_string(path).unwrap().contains("corpus.db"));
        assert!(s.call("updateReleasePage", json!({"url":"https://evil.test/releases"})).is_err());
        assert!(s.call("updateReleasePage", json!({"url":"https://github.com/Yisoragoto/jpop-corpus-tool/releases/tag/v0.3.3"})).is_ok());
        // 没检查过、没下载过：什么都不下、不装
        assert!(s.call("updateDownloadStart", json!({"id": "9.9.9"})).is_err());
        assert!(s.call("updateInstall", json!({"appPath": "/Applications/JPOP Corpus.app"})).is_err());
        assert_eq!(s.call("updateDownloadProgress", json!({})).unwrap()["running"], false);
        drop(s); let _ = std::fs::remove_dir_all(dir);
    }
}
