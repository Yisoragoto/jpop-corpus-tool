//! 更新轨道也写进 settings.json；改库路径时不能顺手丢掉更新偏好。
use std::{path::Path, sync::Mutex};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel { #[default] Stable, Debug }

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub update_channel: Channel,
    pub skipped_stable: String,
    pub skipped_debug: String,
}

impl Preferences {
    pub fn skipped(&self, channel: Channel, identity: &str) -> bool {
        !identity.is_empty() && identity == match channel { Channel::Stable => &self.skipped_stable, Channel::Debug => &self.skipped_debug }
    }
}

pub fn read_at(path: &Path) -> Preferences {
    std::fs::read(path).ok().and_then(|s| serde_json::from_slice(&s).ok()).unwrap_or_default()
}

pub fn read() -> Preferences { read_at(&crate::library_root::settings_path()) }

static WRITING: Mutex<()> = Mutex::new(());

pub fn edit_at(path: &Path, edit: impl FnOnce(&mut Value)) -> Result<()> {
    let _guard = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let mut settings = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).context("设置文件损坏，请先修复 settings.json")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.into()),
    };
    ensure!(settings.is_object(), "设置文件应当是 JSON 对象");
    edit(&mut settings);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, serde_json::to_vec_pretty(&settings)?)?;
    std::fs::rename(temp, path)?;
    Ok(())
}

pub fn set_channel(channel: Channel) -> Result<()> {
    edit_at(&crate::library_root::settings_path(), |s| s["update_channel"] = json!(channel))
}

pub fn skip(channel: Channel, id: &str) -> Result<()> {
    ensure!(id.len() <= 128, "跳过的构建编号太长");
    edit_at(&crate::library_root::settings_path(), |s| {
        s[match channel { Channel::Stable => "skipped_stable", Channel::Debug => "skipped_debug" }] = json!(id);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skips_are_isolated_and_other_settings_survive() {
        let path = std::env::temp_dir().join(format!("jp-update-settings-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        edit_at(&path, |s| { s["libraryRoot"] = json!("library"); s["unknown"] = json!(42); }).unwrap();
        edit_at(&path, |s| { s["update_channel"] = json!("debug"); s["skipped_debug"] = json!("same-id"); }).unwrap();
        let prefs = read_at(&path);
        assert!(prefs.skipped(Channel::Debug, "same-id"));
        assert!(!prefs.skipped(Channel::Stable, "same-id"));
        assert!(!prefs.skipped(Channel::Debug, "next-id"));
        edit_at(&path, |s| s["libraryRoot"] = json!("other")).unwrap();
        let raw: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(raw["unknown"], 42);
        assert_eq!(read_at(&path).update_channel, Channel::Debug);
        let _ = std::fs::remove_file(path);
    }
}
