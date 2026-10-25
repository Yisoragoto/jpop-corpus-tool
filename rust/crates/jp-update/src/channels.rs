//! debug 是滚动包：版本相同也能有新构建；只有构建编号决定是否提醒，semver 只挡降级。
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use super::{API_BASE, AssetView, Channel, DOWNLOAD_PREFIX, ReleaseView, UpdateStatus, is_newer, parse_release};

pub trait Transport { fn get(&self, url: &str) -> Result<Option<Vec<u8>>>; }
pub struct OfficialTransport;
impl Transport for OfficialTransport { fn get(&self, url: &str) -> Result<Option<Vec<u8>>> { super::get_optional(url) } }

pub(super) fn not_older(current: &str, candidate: &str) -> bool {
    match (semver::Version::parse(current.trim_start_matches(['v', 'V'])), semver::Version::parse(candidate)) {
        (Ok(now), Ok(next)) => next >= now,
        _ => false,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest { build_id: String, version: String, file: String, sha256: String, size: u64 }

fn debug(transport: &impl Transport) -> Result<Option<ReleaseView>> {
    let Some(body) = transport.get(&format!("{API_BASE}/tags/debug"))? else { return Ok(None) };
    let release: serde_json::Value = serde_json::from_slice(&body).context("debug 发布信息不是有效 JSON")?;
    ensure!(release["tag_name"] == "debug" && release["prerelease"] == true && release["draft"] == false, "debug 发布必须是固定 tag 的非草稿预发布");
    let assets = release["assets"].as_array().context("debug 发布缺少资产列表")?;
    let prefix = format!("{DOWNLOAD_PREFIX}debug/");
    let manifests: Vec<_> = assets.iter().filter(|a| a["name"] == "latest.json").collect();
    ensure!(manifests.len() == 1, "debug 发布缺少唯一的 latest.json，不能安装");
    let url = manifests[0]["browser_download_url"].as_str().context("debug 清单缺少下载地址")?;
    ensure!(url == format!("{prefix}latest.json"), "debug 清单地址不在本仓库的 debug 发布下");
    let bytes = transport.get(url)?.context("debug 的 latest.json 不存在，不能安装")?;
    let manifest: Manifest = serde_json::from_slice(&bytes).context("debug 的 latest.json 缺少必要字段或格式损坏，不能安装")?;
    ensure!(!manifest.build_id.is_empty() && manifest.build_id.len() <= 128 && manifest.build_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')), "debug 构建编号无效，不能安装");
    ensure!(manifest.sha256.len() == 64 && manifest.sha256.bytes().all(|b| b.is_ascii_hexdigit()), "debug 缺少有效的 SHA-256，不能安装");
    ensure!(manifest.size > 0, "debug 清单里的安装包大小无效");
    ensure!(super::debug_asset_version(&manifest.file) == Some(manifest.version.as_str()), "debug 清单版本与安装包文件名不一致");
    super::check_asset_name(&manifest.file)?;
    let files: Vec<_> = assets.iter().filter(|a| a["name"].as_str() == Some(&manifest.file)).collect();
    ensure!(files.len() == 1, "debug 发布缺少清单指定的唯一安装包");
    let url = files[0]["browser_download_url"].as_str().context("debug 安装包缺少下载地址")?;
    ensure!(url == format!("{prefix}{}", manifest.file), "debug 安装包地址不在本仓库的 debug 发布下");
    ensure!(files[0]["size"].as_u64() == Some(manifest.size), "debug 发布资产大小与 latest.json 不一致");
    Ok(Some(ReleaseView {
        build_id: Some(manifest.build_id), version: manifest.version, tag: "debug".into(),
        name: "debug build".into(), notes: release["body"].as_str().unwrap_or("").into(),
        published_at: release["published_at"].as_str().unwrap_or("").into(),
        url: release["html_url"].as_str().unwrap_or("").into(), prerelease: true,
        installer: Some(AssetView { name: manifest.file, size: manifest.size, url: url.into(), sha256: Some(manifest.sha256.to_ascii_lowercase()) }),
    }))
}

fn stable(transport: &impl Transport) -> Result<Option<ReleaseView>> {
    let Some(body) = transport.get(&format!("{API_BASE}/latest"))? else { return Ok(None) };
    let value = serde_json::from_slice(&body).context("正式版发布信息不是有效 JSON")?;
    Ok(parse_release(&value).filter(|r| !r.prerelease && semver::Version::parse(&r.version).is_ok_and(|v| v.pre.is_empty())))
}

pub fn check_with(current: &str, build_id: &str, channel: Channel, transport: &impl Transport) -> Result<UpdateStatus> {
    let (latest, effective) = match channel {
        Channel::Stable => (stable(transport)?, Channel::Stable),
        Channel::Debug => match debug(transport)? {
            Some(release) => (Some(release), Channel::Debug),
            None => (stable(transport)?, Channel::Stable),
        },
    };
    let update_available = latest.as_ref().is_some_and(|r| match effective {
        Channel::Stable => is_newer(current, &r.version),
        Channel::Debug => r.build_id.as_deref() != Some(build_id) && not_older(current, &r.version),
    });
    let message = if effective == Channel::Stable && latest.as_ref().is_some_and(|r| is_newer(&r.version, current)) {
        "当前是 debug 版，正式版还没有更新".into()
    } else { String::new() };
    Ok(UpdateStatus { current: current.into(), build_id: build_id.into(), channel: effective, skipped: false, message, latest, update_available })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque};
    use serde_json::{Value, json};

    struct Scripted(RefCell<VecDeque<(String, Option<Vec<u8>>)>>);
    impl Transport for Scripted {
        fn get(&self, url: &str) -> Result<Option<Vec<u8>>> {
            let (expected, reply) = self.0.borrow_mut().pop_front().expect("不该再发请求");
            assert_eq!(url, expected);
            Ok(reply)
        }
    }
    fn script(replies: Vec<(&str, Option<Value>)>) -> Scripted {
        Scripted(RefCell::new(replies.into_iter().map(|(url, v)| (url.to_string(), v.map(|v| serde_json::to_vec(&v).unwrap()))).collect()))
    }
    fn stable_body() -> Value { json!({"tag_name":"v0.3.3","draft":false,"prerelease":false,"assets":[]}) }
    fn debug_body() -> Value { json!({"tag_name":"debug","draft":false,"prerelease":true,"assets":[
        {"name":"latest.json","browser_download_url":format!("{DOWNLOAD_PREFIX}debug/latest.json")},
        {"name":"JPOP.Corpus.Tool_0.3.3-debug.exe","size":12,"browser_download_url":format!("{DOWNLOAD_PREFIX}debug/JPOP.Corpus.Tool_0.3.3-debug.exe")}
    ]}) }
    fn manifest(id: &str) -> Value { json!({"buildId":id,"version":"0.3.3","file":"JPOP.Corpus.Tool_0.3.3-debug.exe","sha256":"ab".repeat(32),"size":12}) }
    fn debug_script(body: Value, manifest: Value) -> Scripted {
        script(vec![("https://api.github.com/repos/Yisoragoto/jpop-corpus-tool/releases/tags/debug", Some(body)),
            ("https://github.com/Yisoragoto/jpop-corpus-tool/releases/download/debug/latest.json", Some(manifest))])
    }

    #[test]
    fn stable_uses_latest_and_semver_and_never_accepts_prereleases() {
        let url = "https://api.github.com/repos/Yisoragoto/jpop-corpus-tool/releases/latest";
        let status = check_with("0.3.2", "old", Channel::Stable, &script(vec![(url,Some(stable_body()))])).unwrap();
        assert!(status.update_available);
        for field in ["prerelease", "tag_name"] {
            let mut body = stable_body();
            body[field] = if field == "prerelease" { json!(true) } else { json!("v0.3.4-beta.1") };
            assert!(check_with("0.3.2", "old", Channel::Stable, &script(vec![(url,Some(body))])).unwrap().latest.is_none());
        }
    }
    #[test]
    fn debug_compares_build_ids_even_when_the_version_is_equal() {
        assert!(check_with("0.3.3", "old", Channel::Debug, &debug_script(debug_body(),manifest("new"))).unwrap().update_available);
        assert!(!check_with("0.3.3", "new", Channel::Debug, &debug_script(debug_body(),manifest("new"))).unwrap().update_available);
        assert!(!check_with("0.4.0", "old", Channel::Debug, &debug_script(debug_body(),manifest("new"))).unwrap().update_available);
    }
    #[test]
    fn debug_fails_closed_on_missing_manifest_hash_and_malformed_metadata() {
        let mut missing = debug_body(); missing["assets"] = json!([]);
        let transport = script(vec![("https://api.github.com/repos/Yisoragoto/jpop-corpus-tool/releases/tags/debug",Some(missing))]);
        assert!(check_with("0.3.2", "old", Channel::Debug, &transport).is_err());
        for field in ["sha256", "buildId", "file", "size"] {
            let mut bad = manifest("new"); bad.as_object_mut().unwrap().remove(field);
            assert!(check_with("0.3.2", "old", Channel::Debug, &debug_script(debug_body(),bad)).is_err(), "缺 {field}");
        }
        let mut bad = manifest("new"); bad["sha256"] = json!("00");
        assert!(check_with("0.3.2", "old", Channel::Debug, &debug_script(debug_body(),bad)).is_err());
    }
    #[test]
    fn only_an_absent_debug_release_falls_back_to_stable() {
        let transport = script(vec![("https://api.github.com/repos/Yisoragoto/jpop-corpus-tool/releases/tags/debug",None),
            ("https://api.github.com/repos/Yisoragoto/jpop-corpus-tool/releases/latest",Some(stable_body()))]);
        let status = check_with("0.3.2", "old", Channel::Debug, &transport).unwrap();
        assert_eq!(status.channel, Channel::Stable); assert!(status.update_available);
    }
    #[test]
    fn switching_to_stable_does_not_downgrade() {
        let transport = script(vec![("https://api.github.com/repos/Yisoragoto/jpop-corpus-tool/releases/latest",Some(stable_body()))]);
        let status = check_with("0.4.0", "debug-build", Channel::Stable, &transport).unwrap();
        assert!(!status.update_available); assert!(!status.message.is_empty());
    }
    #[test]
    fn local_build_identity_is_embedded() { assert!(!super::super::BUILD_ID.is_empty()); assert!(super::super::BUILD_ID.contains('-')); }
}
