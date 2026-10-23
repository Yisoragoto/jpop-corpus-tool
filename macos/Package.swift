// swift-tools-version:5.10
//
// JPOP Corpus 的原生 macOS 客户端。
//
// 业务逻辑不在这里：检索、分析、行区间、库结构和迁移都在 Rust 的 jp-corpus 里，
// 经 rust/crates/jp-ffi 编成静态库 libjp_ffi.a 链进来（见 scripts/build-rust.sh）。
// 客户端分层：
//
//   CJPCore     C 头文件的模块映射（头文件就是 jp-ffi 自己那份，不复制）
//   JPCore      Swift 包装：会话、JSON 编解码、数据模型。不依赖 SwiftUI，可以单测
//   JPCorpusModel 可测试的应用、导入与 AVFoundation 播放状态
//   JPCorpusApp SwiftUI 界面
//
// 先构建 Rust：  ./scripts/build-rust.sh
// 再：          swift run JPCorpus         （或 ./scripts/bundle.sh 打成 .app）

import Foundation
import PackageDescription

/// Rust 静态库所在目录。build-rust.sh 默认放在 rust/target/release；
/// 可以用环境变量 JP_FFI_LIB_DIR 指到别处（比如通用二进制的输出目录）
let packageDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let rustLibDir = ProcessInfo.processInfo.environment["JP_FFI_LIB_DIR"]
    ?? packageDir.appendingPathComponent("../rust/target/release").standardizedFileURL.path

let package = Package(
    name: "JPCorpus",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "JPCorpus", targets: ["JPCorpusApp"]),
        .library(name: "JPCore", targets: ["JPCore"]),
    ],
    targets: [
        .systemLibrary(name: "CJPCore", path: "Sources/CJPCore"),
        .target(
            name: "JPCore",
            dependencies: ["CJPCore"],
            linkerSettings: [
                .unsafeFlags(["-L", rustLibDir]),
                .linkedLibrary("jp_ffi"),
                // 制卡（jp-anki）带进来的：切例句音频用播放那套解码器（rodio → CoreAudio），
                // 变调库 Rubber Band 是 C++；AnkiConnect / 下载走的 HTTP 库要 Security
                .linkedLibrary("c++"),
                .linkedFramework("Accelerate"),  // Rubber Band 在 Apple 平台上用 vDSP 做 FFT
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("AudioUnit"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
        .target(name: "JPCorpusModel", dependencies: ["JPCore"]),
        .executableTarget(
            name: "JPCorpusApp",
            dependencies: ["JPCore", "JPCorpusModel"]
        ),
        .testTarget(name: "JPCorpusModelTests", dependencies: ["JPCorpusModel"]),
        .testTarget(
            name: "JPCoreTests",
            dependencies: ["JPCore"],
            resources: [.copy("Fixtures")]
        ),
    ]
)
