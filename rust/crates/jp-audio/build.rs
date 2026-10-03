//! 把 `third_party/rubberband/` 里的 Rubber Band 编进来（变调用，见 `src/pitch.rs`）。
//!
//! 用的是上游自带的**单文件构建** `single/RubberBandSingle.cpp`：一个编译单元，
//! 内置 FFT 和内置重采样器，不依赖 FFTW、libsamplerate 之类的外部库。
//!
//! 为什么不用 crates.io 上的 `rubberband-sys`：它用 bindgen 生成声明，编译时要 libclang，
//! 没装 LLVM 的机器上直接编不过。这里用到的 C 接口只有十来个函数，
//! 在 `src/rubberband.rs` 里手写声明就够了。

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../third_party/rubberband");
    let single = root.join("single/RubberBandSingle.cpp");
    let header = root.join("rubberband/rubberband-c.h");
    assert!(single.is_file(), "找不到 Rubber Band 源码：{}（仓库根目录的 third_party/rubberband/）", single.display());

    println!("cargo:rerun-if-changed={}", root.display());

    // 版本号从上游头文件里读，编进变调缓存的文件名（`pitch::engine_tag`）：
    // 以后升级 third_party 里的源码，旧版本渲染的缓存自动不再被认作这一版的
    let text = std::fs::read_to_string(&header).unwrap_or_else(|err| panic!("读不了 {}：{err}", header.display()));
    let version = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("#define RUBBERBAND_VERSION"))
        .map(|rest| rest.trim().trim_matches('"').to_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| panic!("{} 里没有 RUBBERBAND_VERSION", header.display()));
    println!("cargo:rustc-env=RUBBERBAND_VERSION={version}");

    cc::Build::new()
        .cpp(true)
        .std("c++14")
        // <windows.h> 的 min/max 宏会和 std::min/std::max 打架；M_PI 在 MSVC 上要这个宏才有
        .define("NOMINMAX", None)
        .define("_USE_MATH_DEFINES", None)
        // 调试构建也开优化：不优化的话 R3 渲染一首歌要几分钟，`tauri dev` 下变调等于不能用
        .opt_level(2)
        .debug(false)
        // 上游的代码，警告不归我们管
        .warnings(false)
        .file(&single)
        .compile("rubberband");
}
