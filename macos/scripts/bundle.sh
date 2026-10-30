#!/usr/bin/env bash
# 打成可以双击的 JPOP Corpus.app（未签名，本机自用；分发要再签名、公证）。
#
#   ./scripts/build-rust.sh && ./scripts/bundle.sh
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"
swift build -c release --product JPCorpus
bin="$(swift build -c release --show-bin-path)/JPCorpus"
app="$here/build/JPOP Corpus.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/JPCorpus"
# 与 Windows 包共用版本源。
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' ../rust/crates/jp-app/Cargo.toml | head -1)"
test -n "$version"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>JPOP Corpus</string>
  <key>CFBundleDisplayName</key><string>JPOP Corpus</string>
  <key>CFBundleIdentifier</key><string>io.github.yisoragoto.jpcorpus</string>
  <key>CFBundleExecutable</key><string>JPCorpus</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
  <key>CFBundleLocalizations</key><array><string>zh-Hans</string><string>en</string></array>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.education</string>
</dict>
</plist>
PLIST
# 应用图标：Resources/AppIcon.png（1024×1024，由同目录的 AppIcon.svg 导出），切成 iconset 再打成 .icns。
# 以前没有这一步，Finder 和程序坞里是一张空白图标
iconset="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" Resources/AppIcon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) Resources/AppIcon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
# 英文界面：翻译表（L10n 运行时用）和 Localizable.strings（提示框标题、菜单名这类系统接口用）
python3 "$here/scripts/gen-l10n.py" --out "$app/Contents/Resources"
codesign --force --sign - "$app" >/dev/null 2>&1 || true
echo "已生成：$app"
