//! 检查更新。
//!
//! 版本就发在这个仓库的 Releases 里，所以更新也从那儿来：问一次 GitHub 的
//! Releases API，比版本号，需要的话把安装程序下回来、**校验 SHA-256**、再拉起来。
//!
//! ## 几条守得住的线
//!
//! * **只认自己仓库的下载地址**（[`is_official_download_url`]）。要跑的是一个 exe，
//!   地址从哪来就得由谁负责；接口返回里出现别的域名一律不碰。
//! * **装之前校验哈希**。GitHub 的资产带 `digest: "sha256:…"`，下完当场算一遍对上才装。
//!   对不上就删掉重来——宁可更新失败，也不执行一个来路不明的文件。
//! * **解析不出版本号就当没有新版**。宁可不提示，也不要因为一个奇怪的 tag 名天天弹窗。
//! * 装不装由用户点。「下载后自动安装」默认关着，开了也会把每一步报出来。
//!
//! 没用 `tauri-plugin-updater`：那套要自己签名、再维护一份 `latest.json`，
//! 而这个项目的发布流程就是 `gh release create` 挂两个安装包。用现成的 Releases API
//! 少一套要维护的东西，代价是自动更新只在 Windows 的 NSIS 安装包上成立——
//! 现在也只发这一个平台。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 版本发在哪
pub const REPO: &str = "Yisoragoto/jpop-corpus-tool";

/// 安装包必须从这个前缀下载，别的一律不碰
pub const DOWNLOAD_PREFIX: &str = "https://github.com/Yisoragoto/jpop-corpus-tool/releases/download/";

const API_BASE: &str = "https://api.github.com/repos/Yisoragoto/jpop-corpus-tool/releases";
pub(crate) const USER_AGENT: &str = "jpop-corpus-tool-updater";

/// 一次发布里能装的那个文件。
///
/// 也会从前端传回来（下载时），所以要能反序列化——但**传回来的地址和哈希一样要核**，
/// 见 `download_installer`：地址必须是本仓库的发布地址，哈希对不上就删掉。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetView {
    pub name: String,
    pub size: u64,
    pub url: String,
    /// GitHub 给的 `digest` 去掉 `sha256:` 前缀。没有就是 None——那种情况不自动装。
    pub sha256: Option<String>,
}

/// 一次发布。`notes` 是 Markdown 原文，前端自己决定怎么显示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseView {
    /// 去掉 `v` 前缀的版本号
    pub version: String,
    pub tag: String,
    pub name: String,
    pub notes: String,
    pub published_at: String,
    pub url: String,
    pub prerelease: bool,
    /// 能直接装的安装包（NSIS）。只发了 msi 或者源码时是 None
    pub installer: Option<AssetView>,
}

/// 检查的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// 正在跑的这一版
    pub current: String,
    /// 最新的那一次发布。接口挂了或者仓库一个 release 都没有时是 None
    pub latest: Option<ReleaseView>,
    /// 最新的比当前的新
    pub update_available: bool,
}

/// `candidate` 比 `current` 新吗。
///
/// 两边都按 semver 解析，`v` 前缀去掉。**解析不出来就返回 false**：
/// 宁可不提示，也不要因为一个奇怪的 tag 名天天弹「有新版本」。
pub fn is_newer(current: &str, candidate: &str) -> bool {
    let parse = |s: &str| semver::Version::parse(s.trim().trim_start_matches(['v', 'V']));
    match (parse(current), parse(candidate)) {
        (Ok(now), Ok(next)) => next > now,
        _ => false,
    }
}

/// 挑能装的那个资产：Windows 上是 NSIS 的 `-setup.exe`。
///
/// 不挑 msi：msi 升级要求 UpgradeCode 一致，而且静默升级的行为和 NSIS 不一样；
/// 发布说明里也一直把 setup.exe 写成推荐。
pub fn pick_installer(assets: &[AssetView]) -> Option<AssetView> {
    assets
        .iter()
        .find(|a| a.name.to_ascii_lowercase().ends_with("-setup.exe"))
        .cloned()
}

/// 这个地址是不是自己仓库的发布下载。
///
/// 要执行的是一个 exe，所以地址只认这一个前缀；接口被改、被劫持、或者哪天
/// 返回里混进别的域名，都在这里拦住。
pub fn is_official_download_url(url: &str) -> bool {
    url.starts_with(DOWNLOAD_PREFIX) && !url.contains("..")
}

/// 解析 Releases API 的一条。草稿和解析不出版本号的跳过（返回 None）。
pub fn parse_release(value: &serde_json::Value) -> Option<ReleaseView> {
    if value.get("draft").and_then(|v| v.as_bool()).unwrap_or(false) {
        return None;
    }
    let tag = value.get("tag_name")?.as_str()?.to_string();
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    if semver::Version::parse(&version).is_err() {
        return None;
    }
    let assets: Vec<AssetView> = value
        .get("assets")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    let url = a.get("browser_download_url")?.as_str()?.to_string();
                    if !is_official_download_url(&url) {
                        return None;
                    }
                    Some(AssetView {
                        name: a.get("name")?.as_str()?.to_string(),
                        size: a.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
                        url,
                        sha256: a
                            .get("digest")
                            .and_then(|v| v.as_str())
                            .and_then(|d| d.strip_prefix("sha256:"))
                            .map(|d| d.to_ascii_lowercase()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Some(ReleaseView {
        installer: pick_installer(&assets),
        version,
        name: value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&tag)
            .to_string(),
        notes: value
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        published_at: value
            .get("published_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        url: value
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        prerelease: value
            .get("prerelease")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        tag,
    })
}

/// 解析一整个列表，按版本从新到旧排。
pub fn parse_releases(body: &[u8]) -> Result<Vec<ReleaseView>> {
    let value: serde_json::Value = serde_json::from_slice(body).context("更新接口返回的不是 JSON")?;
    let list = value.as_array().context("更新接口返回的不是列表")?;
    let mut releases: Vec<ReleaseView> = list.iter().filter_map(parse_release).collect();
    releases.sort_by(|a, b| {
        let parse = |s: &str| semver::Version::parse(s).ok();
        match (parse(&b.version), parse(&a.version)) {
            (Some(x), Some(y)) => x.cmp(&y),
            _ => std::cmp::Ordering::Equal,
        }
    });
    Ok(releases)
}

/// 发一个 GET，拿回整个响应体。
fn get(url: &str) -> Result<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(10)))
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .with_context(|| format!("连不上 {url}"))?;
    let body = response
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_to_vec()
        .context("读取更新信息失败")?;
    Ok(body)
}

/// 最近几次发布。「查看更新日志」用。
pub fn changelog(limit: usize) -> Result<Vec<ReleaseView>> {
    let body = get(&format!("{API_BASE}?per_page={}", limit.clamp(1, 30)))?;
    parse_releases(&body)
}

/// 查一次有没有新版。
pub fn check(current: &str) -> Result<UpdateStatus> {
    // 用列表而不是 /latest：预发布版不在 /latest 里，而「最近发了什么」这一份
    // 顺带就是更新日志要的数据，少发一次请求。
    let releases = changelog(5)?;
    let latest = releases.into_iter().find(|r| !r.prerelease);
    Ok(UpdateStatus {
        update_available: latest
            .as_ref()
            .is_some_and(|r| is_newer(current, &r.version)),
        current: current.to_string(),
        latest,
    })
}

/// 下载进度。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub received: u64,
    pub total: u64,
    /// downloading / verifying / done / failed
    pub stage: String,
    pub message: String,
}

/// 同一个进程里只允许一个下载在跑。
///
/// 下载有**两条路**：启动时的自动更新（App.tsx）和设置里的按钮。两条一起跑的话，
/// 以前共用同一个 `.part`：两边各写各的、内容交错，而各自的哈希只算自己收到的字节——
/// **校验会「通过」，落盘的却是一团乱**；先改完名的那个还会把另一个的文件抽走，
/// 另一个于是报「改名失败」。锁 + 各用各的临时文件，两个问题一起没了。
static DOWNLOADING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 安装程序这个进程里只拉起一次。两条路都走到「装」的时候，
/// 同时跑两个 NSIS 会互相抢文件。
static INSTALL_LAUNCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 这个文件是不是已经下好而且校验得过。半截的、损坏的都返回 false。
fn verified(path: &Path, expected: &str) -> bool {
    sha256_of(path).is_ok_and(|got| got.eq_ignore_ascii_case(expected))
}

/// 整个文件的 SHA-256。流式读，6MB 的安装包和 207MB 的词典都用它。
pub(crate) fn sha256_of(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("打不开 {}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .with_context(|| format!("读不完 {}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// 把安装包下到 `dir`，校验 SHA-256，返回落盘路径。
///
/// `on_progress` 每收到一块调一次——6 MB 的文件在慢网上要好几分钟，
/// 没有进度的话用户只会看到一个不动的按钮。
///
/// 已经下好过、而且校验得过的那一份直接用，不重下。
pub fn download_installer(
    asset: &AssetView,
    dir: &Path,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<PathBuf> {
    if !is_official_download_url(&asset.url) {
        bail!("下载地址不是本仓库的发布地址，已拒绝：{}", asset.url);
    }
    let Some(expected) = asset.sha256.clone() else {
        bail!("这次发布没有给出校验值，不能自动安装；请到发布页手动下载");
    };
    std::fs::create_dir_all(dir).with_context(|| format!("建不出 {}", dir.display()))?;
    let dest = dir.join(&asset.name);

    // 排队：另一条路正在下同一个文件时，这里等它下完，然后走下面那条「已经有了」
    let _queued = DOWNLOADING.lock().unwrap_or_else(|err| err.into_inner());

    if verified(&dest, &expected) {
        on_progress(asset.size, asset.size);
        return Ok(dest);
    }

    // 断点就是这个 `.part`，所以名字是固定的：换一次名字等于把上次下到的扔了。
    // 同时下两个由上面那把锁挡着，跨进程撞车由最后的 SHA-256 兜底。
    let part = dir.join(format!("{}.part", asset.name));

    crate::net::download_resumable(&asset.url, &part, asset.size, &mut on_progress)?;

    // 校验在改名之前：对不上的那一份连文件名都不该出现。
    // 续传拼出来的东西对不对，就看这一步——所以这里算的是**盘上的字节**，
    // 不是下载时顺手攒的哈希（那只算了这一轮收到的那段）。
    let got = sha256_of(&part)?;
    if !got.eq_ignore_ascii_case(&expected) {
        let _ = std::fs::remove_file(&part);
        bail!("下载的文件校验不过（期望 {expected}，实际 {got}），已删掉；再点一次会重新下");
    }

    crate::net::finish(&part, &dest)?;
    Ok(dest)
}

/// 拉起安装程序。调用方随后退出应用——NSIS 要替换正在运行的 exe。
pub fn launch_installer(path: &Path) -> Result<()> {
    anyhow::ensure!(path.is_file(), "找不到安装包：{}", path.display());
    // 两条路（启动时自动更新、设置里的按钮）可能都走到这儿。拉起两个 NSIS 会互相抢文件，
    // 所以第二次就什么都不做——反正第一个已经在装了，应用马上要退出。
    if INSTALL_LAUNCHED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }
    std::process::Command::new(path)
        .spawn()
        .with_context(|| format!("启动安装程序失败：{}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/github-releases.json"),
        )
        .expect("缺少 tests/data/github-releases.json")
    }

    #[test]
    fn a_higher_version_is_newer_and_anything_unparseable_is_not() {
        assert!(is_newer("0.2.2", "0.2.3"));
        assert!(is_newer("0.2.2", "v0.3.0"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("0.2.2", "0.2.2"));
        assert!(!is_newer("0.2.2", "v0.2.2"));
        assert!(!is_newer("0.2.3", "0.2.2"));
        // 认不出来的一律当没有新版：宁可不提示，也不要天天弹窗
        assert!(!is_newer("0.2.2", "nightly"));
        assert!(!is_newer("0.2.2", ""));
        assert!(!is_newer("开发版", "0.3.0"));
    }

    #[test]
    fn the_nsis_setup_is_the_one_we_install() {
        let msi = AssetView {
            name: "JPOP.Corpus.Tool_0.2.2_x64_en-US.msi".into(),
            size: 1,
            url: format!("{DOWNLOAD_PREFIX}v0.2.2/x.msi"),
            sha256: None,
        };
        let exe = AssetView {
            name: "JPOP.Corpus.Tool_0.2.2_x64-setup.exe".into(),
            size: 2,
            url: format!("{DOWNLOAD_PREFIX}v0.2.2/x.exe"),
            sha256: None,
        };
        assert_eq!(pick_installer(&[msi.clone(), exe.clone()]), Some(exe));
        assert_eq!(pick_installer(&[msi]), None);
        assert_eq!(pick_installer(&[]), None);
    }

    /// 要跑的是一个 exe，地址只认自己仓库那一个前缀。
    #[test]
    fn only_this_repos_release_downloads_are_accepted() {
        assert!(is_official_download_url(&format!("{DOWNLOAD_PREFIX}v0.2.2/setup.exe")));
        assert!(!is_official_download_url("https://example.com/setup.exe"));
        assert!(!is_official_download_url("http://github.com/Yisoragoto/jpop-corpus-tool/releases/download/v1/x.exe"));
        assert!(!is_official_download_url("https://github.com/someone-else/repo/releases/download/v1/x.exe"));
        assert!(!is_official_download_url(&format!("{DOWNLOAD_PREFIX}../../../etc/x.exe")));
    }

    /// 拿真实的接口返回对账，不是手捏的 JSON
    #[test]
    fn the_real_api_response_parses_into_what_the_page_shows() {
        let releases = parse_releases(&fixture()).unwrap();
        assert_eq!(releases.len(), 3);
        assert_eq!(releases[0].version, "0.2.2");
        assert_eq!(releases[1].version, "0.2.1");
        assert_eq!(releases[2].version, "0.2.0");
        assert!(releases[0].name.contains("0.2.2"));
        assert!(releases[0].notes.len() > 200, "发布说明没解析出来");
        assert!(releases[0].published_at.starts_with("2026-"));
        assert!(!releases[0].prerelease);

        let installer = releases[0].installer.as_ref().expect("该有 setup.exe");
        assert!(installer.name.ends_with("-setup.exe"));
        assert!(installer.size > 1_000_000);
        assert_eq!(installer.sha256.as_deref().map(str::len), Some(64));
        assert!(is_official_download_url(&installer.url));
    }

    #[test]
    fn a_draft_or_odd_tag_is_skipped() {
        let json = serde_json::json!([
            { "tag_name": "v9.9.9", "draft": true, "assets": [] },
            { "tag_name": "nightly-2026-10-02", "draft": false, "assets": [] },
            { "tag_name": "v0.2.2", "draft": false, "assets": [] },
        ]);
        let releases = parse_releases(json.to_string().as_bytes()).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, "0.2.2");
    }

    /// 接口里混进别的域名时，那个资产直接不要——而不是「下下来再说」
    #[test]
    fn an_asset_from_another_host_is_dropped() {
        let json = serde_json::json!([{
            "tag_name": "v0.3.0",
            "draft": false,
            "assets": [{
                "name": "JPOP.Corpus.Tool_0.3.0_x64-setup.exe",
                "size": 123,
                "browser_download_url": "https://cdn.example.com/evil-setup.exe",
                "digest": "sha256:00"
            }],
        }]);
        let releases = parse_releases(json.to_string().as_bytes()).unwrap();
        assert_eq!(releases.len(), 1);
        assert!(releases[0].installer.is_none(), "别的域名的资产不该被采纳");
    }

    #[test]
    fn a_download_to_a_foreign_url_is_refused_before_any_network_call() {
        let asset = AssetView {
            name: "evil.exe".into(),
            size: 1,
            url: "https://cdn.example.com/evil.exe".into(),
            sha256: Some("00".into()),
        };
        let dir = std::env::temp_dir().join(format!("jp-update-test-{}", std::process::id()));
        let err = download_installer(&asset, &dir, |_, _| {}).unwrap_err();
        assert!(format!("{err}").contains("已拒绝"), "{err}");
        assert!(!dir.join("evil.exe").exists());
    }

    /// 下载那条路的核心不变量：已经下好、校验得过的那一份直接用，不重下、不改名。
    #[test]
    fn an_already_downloaded_and_verified_file_is_reused() {
        let dir = std::env::temp_dir().join(format!(
            "jp-update-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let body = b"pretend this is an installer";
        let sha = format!("{:x}", Sha256::digest(body));
        let name = "JPOP.Corpus.Tool_9.9.9_x64-setup.exe";
        std::fs::write(dir.join(name), body).unwrap();

        let asset = AssetView {
            name: name.to_string(),
            size: body.len() as u64,
            // 地址必须是本仓库的，否则第一道关就挡了
            url: format!("{DOWNLOAD_PREFIX}v9.9.9/{name}"),
            sha256: Some(sha.clone()),
        };
        // 没有网也该成功：走的是「已经有了」那条
        let got = download_installer(&asset, &dir, |_, _| {}).unwrap();
        assert_eq!(got, dir.join(name));

        // 内容被改坏之后就不能再当成「已经有了」。这里不真下载（会连网），
        // 只验校验函数本身的判断。
        std::fs::write(dir.join(name), b"corrupted").unwrap();
        assert!(!verified(&dir.join(name), &sha));
        let _ = std::fs::remove_dir_all(&dir);
    }

}
