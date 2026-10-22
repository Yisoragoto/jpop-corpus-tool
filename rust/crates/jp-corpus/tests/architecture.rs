use std::{collections::BTreeSet, fs, path::Path};

fn dependencies(toml: &str) -> BTreeSet<String> {
    let mut section = "";
    let mut names = BTreeSet::new();
    for line in toml.lines().map(str::trim) {
        if line.starts_with('#') { continue; }
        if line.starts_with('[') {
            section = line.trim_matches(['[', ']']);
            if let Some((_, name)) = section.split_once("dependencies.") {
                names.insert(name.trim_matches('"').to_string());
            }
        } else if (section == "dependencies" || section.ends_with(".dependencies"))
            && let Some((name, _)) = line.split_once('=')
        {
            names.insert(name.trim().trim_matches('"').split('.').next().unwrap().to_string());
        }
    }
    names
}

#[test]
fn the_core_only_depends_on_its_explicit_allowlist() {
    let actual = dependencies(include_str!("../Cargo.toml"));
    let allowed: BTreeSet<String> = ["anyhow", "serde", "serde_json", "rusqlite", "jp-normalize", "rodio", "sha2"]
        .into_iter().map(String::from).collect();
    assert!(!actual.iter().any(|s| s == "jp-app" || s.starts_with("tauri")), "核心不能依赖壳：{actual:?}");
    assert!(actual.is_subset(&allowed), "新增依赖要先审查白名单：{actual:?}");
    // 解析器也要认平台依赖和子表，避免把壳藏在另一种 TOML 写法里绕过守卫。
    assert!(dependencies("[target.'cfg(windows)'.dependencies]\ntauri = {}\n[dependencies.jp-app]\npath = '../jp-app'").contains("tauri"));
    assert!(dependencies("[dependencies.jp-app]\npath = '../jp-app'").contains("jp-app"));
}

fn inspect(dir: &Path) {
    // 运行时递归读目录：新加的模块也要检查；include_str! 的手工清单会漏掉新文件。
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            inspect(&path);
        } else if path.extension().is_some_and(|s| s == "rs") {
            let source = fs::read_to_string(&path).unwrap();
            let compact = source.split_whitespace().collect::<String>();
            assert!(!compact.contains("usetauri") && !compact.contains("use::tauri"), "核心引入了 UI：{}", path.display());
        }
    }
}

#[test]
fn the_core_source_does_not_import_tauri() {
    inspect(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
}
