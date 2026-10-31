//! 系统强调色：启动时读一次 Windows 的强调色色阶，前端写进 `--sys-accent-*`。
//!
//! WinUI 的强调按钮、开关、导航指示条都跟着系统强调色走（深色主题用 Light2，浅色用 Dark1），
//! 写死 `#4cc2ff` 的话，用户在系统设置里换了颜色，这里还是那一个蓝。
//! 网页里拿不到这套色阶（`prefers-color-scheme` 只有明暗），所以从 Rust 读 `UISettings`。
//! 其它平台返回 `None`，前端用 CSS 里的回退色。

use std::collections::BTreeMap;

#[tauri::command(async)]
pub fn system_accent() -> Option<BTreeMap<String, String>> {
    #[cfg(windows)]
    {
        use windows::UI::ViewManagement::{UIColorType, UISettings};
        let settings = UISettings::new().ok()?;
        let mut colors = BTreeMap::new();
        for (name, kind) in [
            ("accent", UIColorType::Accent),
            ("light1", UIColorType::AccentLight1),
            ("light2", UIColorType::AccentLight2),
            ("light3", UIColorType::AccentLight3),
            ("dark1", UIColorType::AccentDark1),
            ("dark2", UIColorType::AccentDark2),
            ("dark3", UIColorType::AccentDark3),
        ] {
            let color = settings.GetColorValue(kind).ok()?;
            colors.insert(
                name.to_owned(),
                format!("#{:02x}{:02x}{:02x}", color.R, color.G, color.B),
            );
        }
        Some(colors)
    }
    #[cfg(not(windows))]
    {
        None
    }
}
