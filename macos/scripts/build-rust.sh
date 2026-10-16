#!/usr/bin/env bash
# 构建 Rust 核心的静态库 libjp_ffi.a，Swift 包从 rust/target/release 链接它。
#
#   ./scripts/build-rust.sh            本机架构
#   ./scripts/build-rust.sh universal  arm64 + x86_64 合成通用库，输出到 rust/target/universal-release
#                                      （之后 JP_FFI_LIB_DIR=rust/target/universal-release swift build）
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
rust="$here/../rust"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"

if [[ "${1:-}" == "universal" ]]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  (cd "$rust" && cargo build --release -p jp-ffi --target aarch64-apple-darwin --target x86_64-apple-darwin)
  out="$rust/target/universal-release"
  mkdir -p "$out"
  lipo -create \
    "$rust/target/aarch64-apple-darwin/release/libjp_ffi.a" \
    "$rust/target/x86_64-apple-darwin/release/libjp_ffi.a" \
    -output "$out/libjp_ffi.a"
  echo "通用静态库：$out/libjp_ffi.a"
else
  (cd "$rust" && cargo build --release -p jp-ffi)
  echo "静态库：$rust/target/release/libjp_ffi.a"
fi
