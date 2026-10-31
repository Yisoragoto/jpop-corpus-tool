import AppKit
import JPCore
import JPCorpusModel
import SwiftUI

/// 卡拉 OK 字幕样式的设置（对应 Windows 的 KaraokeStyleEditor）：预设、字体字号粗细、没唱 / 唱过两套颜色、
/// 描边、发光、立体阴影、两行错开。设置页「歌词」里一份，全屏卡拉 OK 顶栏的调色板按钮里一份，改的是同一个
/// `app.karaokeStyle`（和 Windows 同一套字段）。
@MainActor
struct KaraokeStyleControls: View {
    @Environment(AppModel.self) private var app
    /// 字体菜单里的家族名
    let families: [String]

    private static let presetLabels: [KaraokeStyle.Preset: String] = [.classic: "经典蓝", .pink: "粉色像素风", .plain: "简洁"]

    var body: some View {
        @Bindable var app = app
        let style = app.karaokeStyle
        VStack(alignment: .leading, spacing: 8) {
            SettingGroup("卡拉 OK 字幕") {
                SettingRow(icon: "paintpalette", title: "预设", description: "换预设只换颜色、描边、发光和阴影，字体和字号留着") {
                    Picker("预设", selection: Binding(get: { style.matchingPreset }, set: { id in
                        if let id { app.karaokeStyle = app.karaokeStyle.applying(id) }
                    })) {
                        ForEach(KaraokeStyle.Preset.allCases) { id in
                            Text(Self.presetLabels[id] ?? id.rawValue).tag(Optional(id))
                        }
                    }
                    .pickerStyle(.segmented).labelsHidden().frame(width: 240)
                }
                SettingDivider()
                SettingRow(icon: "textformat", title: "字幕字体", description: "日文粗体（ヒラギノ角ゴ、游ゴシック）最像 KTV") {
                    Picker("字幕字体", selection: $app.karaokeStyle.fontFamily) {
                        Text("跟歌词字体一样").tag("")
                        if !style.fontFamily.isEmpty && !families.contains(style.fontFamily) {
                            Text("\(style.fontFamily)（本机没有）").tag(style.fontFamily)
                        }
                        ForEach(families, id: \.self) { Text(verbatim: $0).tag($0) }
                    }.frame(width: 190)
                }
                SettingDivider()
                SettingRow(icon: "textformat.size", title: "字幕字号", description: "窗口窄了会按比例缩小") {
                    SettingSlider(title: "字幕字号", value: $app.karaokeStyle.fontSize, range: KaraokeStyle.fontSizeRange)
                }
                SettingDivider()
                SettingRow(icon: "bold", title: "粗细") {
                    Picker("粗细", selection: $app.karaokeStyle.fontWeight) {
                        ForEach(Array(stride(from: 400, through: 900, by: 100)), id: \.self) { Text(verbatim: "\($0)").tag($0) }
                    }.frame(width: 100)
                }
                SettingDivider()
                SettingRow(icon: "character", title: "没唱到的字", description: "填色、描边、发光") {
                    HStack(spacing: 10) {
                        swatch("填色", \.unsungFill)
                        swatch("描边", \.unsungStroke)
                        swatch("发光", \.unsungGlow)
                    }
                }
                SettingDivider()
                SettingRow(icon: "character.cursor.ibeam", title: "唱过的字", description: "逐字从左往右擦过去") {
                    HStack(spacing: 10) {
                        swatch("填色", \.sungFill)
                        swatch("描边", \.sungStroke)
                        swatch("发光", \.sungGlow)
                    }
                }
                SettingDivider()
                SettingRow(icon: "pencil.tip", title: "描边粗细") {
                    SettingSlider(title: "描边粗细", value: $app.karaokeStyle.strokeWidth, range: KaraokeStyle.strokeWidthRange)
                }
                SettingDivider()
                SettingRow(icon: "sun.max", title: "发光强度", description: "0 是不发光") {
                    SettingSlider(title: "发光强度", value: $app.karaokeStyle.glowStrength, range: KaraokeStyle.glowStrengthRange)
                }
                SettingDivider()
                SettingRow(icon: "square.3.layers.3d.down.right", title: "立体阴影", description: "往右下方叠出的厚度，让字浮起来；0 是没有") {
                    HStack(spacing: 10) {
                        swatch("阴影色", \.shadowColor)
                        SettingSlider(title: "立体阴影", value: $app.karaokeStyle.shadowDepth, range: KaraokeStyle.shadowDepthRange)
                    }
                }
                SettingDivider()
                SettingRow(icon: "text.justify.left", title: "两行错开", description: "上行靠左、下行靠右；关掉两行都居中") {
                    Toggle("两行错开", isOn: $app.karaokeStyle.staggered).toggleStyle(.switch)
                }
            }
            preview
        }
    }

    /// 预览：前半唱过、后半没唱，和卡拉 OK 里同一个视图
    private static let previewLine = try? JSONDecoder().decode(LyricLine.self, from: Data(
        #"{"utteranceId":-1,"lineIdx":0,"timeSec":0,"text":"恋のお相手はお星様","tokens":[]}"#.utf8))
    private static let previewUnits = try? JSONDecoder().decode([LyricTimingUnit].self, from: Data(
        #"[{"charStart":0,"charEnd":9,"startSec":0,"endSec":1,"source":"preview"}]"#.utf8))

    @ViewBuilder private var preview: some View {
        if let line = Self.previewLine {
            KaraokeLyricText(line: line, units: Self.previewUnits, sec: 0.62, size: 34)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 18)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color(red: 0.12, green: 0.13, blue: 0.18)))
            .accessibilityLabel("字幕样式预览")
        }
    }

    private func swatch(_ title: String, _ key: WritableKeyPath<KaraokeStyle, String>) -> some View {
        ColorPicker(selection: Binding(
            get: { Color(hex: app.karaokeStyle[keyPath: key]) },
            set: { color in
                guard let c = NSColor(color).usingColorSpace(.sRGB) else { return }
                app.karaokeStyle[keyPath: key] = KaraokeStyle.hex(r: c.redComponent, g: c.greenComponent, b: c.blueComponent)
            }), supportsOpacity: false) {
            Text(title).font(.caption).foregroundStyle(.secondary)
        }
        .fixedSize()
    }
}
