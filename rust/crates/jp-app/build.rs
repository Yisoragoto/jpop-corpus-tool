fn main() {
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
