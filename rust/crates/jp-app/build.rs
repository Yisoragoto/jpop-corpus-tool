fn main() {
    println!("cargo:rerun-if-env-changed=JPOP_BUILD_ID");
    println!("cargo:rerun-if-changed=src");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap();
    println!("cargo:rerun-if-changed={}", root.join(".git/HEAD").display());
    println!("cargo:rerun-if-changed={}", root.join(".git/refs").display());
    let build_id = std::env::var("JPOP_BUILD_ID").unwrap_or_else(|_| {
        let safe = format!("safe.directory={}", root.display().to_string().replace('\\', "/"));
        let sha = std::process::Command::new("git").args(["-c", &safe, "rev-parse", "--short=12", "HEAD"])
            .current_dir(root).output().ok().filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(|| "local".into());
        format!("{sha}-{}", utc_minute())
    });
    assert!(!build_id.is_empty() && build_id.len() <= 128 && build_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')), "JPOP_BUILD_ID 必须是非空构建编号");
    println!("cargo:rustc-env=JPOP_BUILD_ID={build_id}");
    // 集成测试的可执行文件也要声明 ComCtl32 v6 依赖。
    //
    // `tauri-plugin-dialog` → `rfd` 打开了 `common-controls-v6`，链进来的
    // 代码要求进程加载 ComCtl32 版本 6。真正的应用由 tauri-build 生成的
    // 清单声明了这个依赖，但 `cargo test` 产出的 exe 没有清单，于是一启动
    // 就 `STATUS_ENTRYPOINT_NOT_FOUND`——测试一条都跑不起来。
    //
    // Cargo 的 feature 是并集，没法把 rfd 的这个 feature 关掉，
    // 所以反过来给测试 exe 补上清单声明。等价于 MSVC 里的
    // `#pragma comment(linker, "/MANIFESTDEPENDENCY:...")`。
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
    tauri_build::build()
}

fn utc_minute() -> String {
    // 标准库没有日期格式化；按 Gregorian civil-from-days 算 UTC，避免为构建编号增加 chrono。
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let days = (seconds / 86400) as i64 + 719468;
    let era = days / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}{:02}{:02}", seconds / 3600 % 24, seconds / 60 % 60)
}
