import JPCore
import JPCorpusModel
import SwiftUI

/// KTV 一行（对应 Windows 的 `.ktv-row` + paintKaraoke）：不换行，按「字幕样式」（`app.karaokeStyle`）画
/// 粗体大字 + 描边 + 发光 + 立体阴影，振假名在上面跟着同一套样式。
///
/// 两层：底下一层整行画「没唱」那套，上面一层整行画「唱过」那套，用逐字的遮罩露出唱到的部分。
/// 两层都不随时间变（`KtvLayer` 是 Equatable，每帧不重画），每帧只换遮罩里每个字那一条矩形的宽度——
/// 描边是一圈偏移的拷贝，重画很贵，不能每秒 30 次。发光在遮罩之后糊，侧面在两层合成之后叠。
///
/// 单位内按码点顺序扫亮，原始单位时间不变；没有逐字时间的行到点整行亮起，不按字数估算。
@MainActor
struct KaraokeLyricText: View {
    @Environment(AppModel.self) private var app
    let line: LyricLine
    let units: [LyricTimingUnit]?
    let sec: Double
    var size: CGFloat = 34

    struct Part: Identifiable, Equatable {
        var start: Int
        var chars: [String]
        var reading: String?
        var id: Int { start }
    }
    private var parts: [Part] {
        let chars = Array(line.text.unicodeScalars).map { String($0) }
        let rubies = app.showFurigana ? (app.furigana[line.utteranceId] ?? []) : []
        var out: [Part] = [], cursor = 0
        for ruby in rubies.sorted(by: { $0.start < $1.start }) {
            guard ruby.start >= cursor, ruby.start < ruby.end, ruby.end <= chars.count else { continue }
            if cursor < ruby.start { out.append(.init(start: cursor, chars: Array(chars[cursor..<ruby.start]))) }
            out.append(.init(start: ruby.start, chars: Array(chars[ruby.start..<ruby.end]), reading: ruby.reading))
            cursor = ruby.end
        }
        if cursor < chars.count { out.append(.init(start: cursor, chars: Array(chars[cursor...]))) }
        return out
    }
    /// 没有逐字时间：整行到点亮起
    private var wholeLineOn: Bool { units == nil && (line.timeSec.map { sec >= $0 } ?? false) }

    var body: some View {
        let style = app.karaokeStyle
        let parts = self.parts
        let hasRuby = parts.contains { $0.reading != nil }
        let metrics = KtvMetrics(style: style, size: size)
        let fonts = KtvFonts(font: karaokeFont(style, app.lyricsDisplay, size: size),
                             ruby: karaokeFont(style, app.lyricsDisplay, size: size * 0.38))
        let count = parts.last.map { $0.start + $0.chars.count } ?? 0
        let progress = (0..<count).map(progress(at:))
        let lit = progress.contains { $0 > 0 }
        let unsung = KtvLook.unsung(style), sung = KtvLook.sung(style)
        let mask = KtvMask(parts: parts, hasRuby: hasRuby, fonts: fonts, progress: progress, pad: metrics.pad)
        ZStack {
            // 发光垫在最底下，而且是遮罩之后再糊：唱过那层的光跟着擦到哪亮到哪，擦色的边上不会切出一条硬边
            // 两层糊：一层贴边、一层散开，和 Windows 的两个 drop-shadow（g/4、g/2）一样亮
            if metrics.glow > 0 {
                ForEach([metrics.glow / 4, metrics.glow / 2], id: \.self) { radius in
                    layer(parts, hasRuby, fonts, unsung.silhouette(unsung.glow), metrics).blur(radius: radius)
                    if lit { layer(parts, hasRuby, fonts, sung.silhouette(sung.glow), metrics).mask { mask }.blur(radius: radius) }
                }
            }
            // 描好边的两层合成一张，再整体往右下叠出侧面：侧面接在描边外面，不会夹进字里
            ZStack {
                layer(parts, hasRuby, fonts, unsung, metrics)
                if lit { layer(parts, hasRuby, fonts, sung, metrics).mask { mask } }
            }
            .compositingGroup()
            .modifier(KtvExtrude(color: unsung.shadow, steps: KaraokeStyle.extrusionSteps(metrics.depth), unit: metrics.step))
        }
        // 两层和遮罩都在四周留了 pad 给描边、发光、阴影；排版时把这圈让回去
        .padding(-metrics.pad)
        .lineLimit(1)
        .animation(nil, value: sec)
    }

    private func layer(_ parts: [Part], _ hasRuby: Bool, _ fonts: KtvFonts, _ look: KtvLook, _ metrics: KtvMetrics) -> some View {
        KtvLayer(parts: parts, hasRuby: hasRuby, fonts: fonts, look: look, metrics: metrics).equatable()
    }

    private func progress(at index: Int) -> Double {
        if units == nil { return wholeLineOn ? 1 : 0 }
        guard let unit = units?.first(where: { $0.charStart <= index && index < $0.charEnd }) else { return 0 }
        let local = unit.progress(at: sec) * Double(unit.charEnd - unit.charStart) - Double(index - unit.charStart)
        return min(1, max(0, local))
    }
}

/// 字幕样式按当前字号折算出来的长度（样式里的数是按 style.fontSize 定的，窗口窄了一起缩）
struct KtvMetrics: Equatable {
    var stroke: CGFloat
    var glow: CGFloat
    var depth: Int
    var step: CGFloat
    var pad: CGFloat
    init(style: KaraokeStyle, size: CGFloat) {
        let scale = size / max(1, style.fontSize)
        stroke = style.strokeWidth * scale
        glow = style.glowStrength * scale
        depth = Int(style.shadowDepth.rounded())
        step = scale
        pad = ceil(stroke + glow * 1.5 + style.shadowDepth * scale + 2)
    }
}

struct KtvFonts: Equatable {
    var font: Font
    var ruby: Font
}

/// 一套「填色 + 描边 + 发光 + 立体阴影」
struct KtvLook: Equatable {
    var fill: Color
    var stroke: Color
    var glow: Color
    var shadow: Color
    static func unsung(_ s: KaraokeStyle) -> KtvLook {
        .init(fill: Color(hex: s.unsungFill), stroke: Color(hex: s.unsungStroke), glow: Color(hex: s.unsungGlow), shadow: Color(hex: s.shadowColor))
    }
    /// 整块一个颜色（填色、描边都是它）：发光用的剪影
    func silhouette(_ color: Color) -> KtvLook { .init(fill: color, stroke: color, glow: color, shadow: color) }
    static func sung(_ s: KaraokeStyle) -> KtvLook {
        .init(fill: Color(hex: s.sungFill), stroke: Color(hex: s.sungStroke), glow: Color(hex: s.sungGlow), shadow: Color(hex: s.shadowColor))
    }
}

extension Color {
    init(hex: String) {
        let c = KaraokeStyle.rgb(hex)
        self.init(.sRGB, red: c.r, green: c.g, blue: c.b)
    }
}

/// 一行字的排版：每个字单独一个 Text（遮罩要和它逐字对齐），振假名在上
private struct KtvGlyphs: View {
    let parts: [KaraokeLyricText.Part]
    let hasRuby: Bool
    let fonts: KtvFonts
    let color: Color
    var body: some View {
        HStack(alignment: .bottom, spacing: 0) {
            ForEach(parts) { part in
                VStack(spacing: 0) {
                    if hasRuby {
                        Text(verbatim: part.reading ?? " ").font(fonts.ruby).opacity(part.reading == nil ? 0 : 1)
                    }
                    HStack(spacing: 0) {
                        ForEach(Array(part.chars.enumerated()), id: \.offset) { _, ch in Text(verbatim: ch).font(fonts.font) }
                    }
                }
                .fixedSize()
            }
        }
        .foregroundStyle(color)
    }
}

/// 一整行按一套样式画好填色和描边：描边是一圈偏移的拷贝（SwiftUI 的 Text 没有描边）。
/// 侧面（KtvExtrude）和发光在外面加，见 KaraokeLyricText.body
struct KtvLayer: View, Equatable {
    let parts: [KaraokeLyricText.Part]
    let hasRuby: Bool
    let fonts: KtvFonts
    let look: KtvLook
    let metrics: KtvMetrics

    private var ring: [CGSize] {
        let w = metrics.stroke
        guard w > 0.2 else { return [] }
        let n = max(8, min(24, Int(w * 3)))
        var out = (0..<n).map { i -> CGSize in
            let a = Double(i) / Double(n) * 2 * .pi
            return CGSize(width: cos(a) * w, height: sin(a) * w)
        }
        // 粗描边外圈的拷贝之间会漏缝：里面再补一圈
        if w > 3 {
            out += (0..<(n / 2)).map { i -> CGSize in
                let a = (Double(i) + 0.5) / Double(n / 2) * 2 * .pi
                return CGSize(width: cos(a) * w * 0.55, height: sin(a) * w * 0.55)
            }
        }
        return out
    }

    var body: some View {
        ZStack {
            ForEach(Array(ring.enumerated()), id: \.offset) { _, offset in
                KtvGlyphs(parts: parts, hasRuby: hasRuby, fonts: fonts, color: look.stroke).offset(offset)
            }
            KtvGlyphs(parts: parts, hasRuby: hasRuby, fonts: fonts, color: look.fill)
        }
        .padding(metrics.pad)
    }
}

/// 立体侧面：每一步都把之前叠好的整块再往右下挪一次（1、2、4… 翻倍），几步就连成一整块实心的侧面
struct KtvExtrude: ViewModifier {
    let color: Color
    let steps: [Int]
    let unit: CGFloat
    func body(content: Content) -> some View {
        var out = AnyView(content)
        for step in steps {
            let d = CGFloat(step) * unit
            out = AnyView(out.shadow(color: color, radius: 0, x: d, y: d))
        }
        return out
    }
}

/// 唱过那层的遮罩：和 KtvGlyphs 同样排版，每个字换成一条按进度从左往右伸长的矩形；
/// 矩形上下伸出 pad，描边、发光、阴影一起露出来。振假名底下有字唱到了就整个露出来
private struct KtvMask: View {
    let parts: [KaraokeLyricText.Part]
    let hasRuby: Bool
    let fonts: KtvFonts
    let progress: [Double]
    let pad: CGFloat

    var body: some View {
        let last = progress.count - 1
        HStack(alignment: .bottom, spacing: 0) {
            ForEach(parts) { part in
                let lit = (part.start..<(part.start + part.chars.count)).contains { progress.indices.contains($0) && progress[$0] > 0 }
                VStack(spacing: 0) {
                    if hasRuby {
                        Text(verbatim: part.reading ?? " ").font(fonts.ruby).hidden()
                            .overlay { Rectangle().padding(-pad).opacity(lit ? 1 : 0) }
                    }
                    HStack(spacing: 0) {
                        ForEach(Array(part.chars.enumerated()), id: \.offset) { offset, ch in
                            let index = part.start + offset
                            let p = progress.indices.contains(index) ? progress[index] : 0
                            Text(verbatim: ch).font(fonts.font).hidden()
                                .overlay {
                                    Rectangle().padding(.vertical, -pad).scaleEffect(x: p, anchor: .leading)
                                }
                                // 行首、行尾（唱完）把左右伸出去的描边也露出来
                                .overlay(alignment: .leading) {
                                    if index == 0 && p > 0 { Rectangle().frame(width: pad).padding(.vertical, -pad).offset(x: -pad) }
                                }
                                .overlay(alignment: .trailing) {
                                    if index == last && p >= 1 { Rectangle().frame(width: pad).padding(.vertical, -pad).offset(x: pad) }
                                }
                        }
                    }
                }
                .fixedSize()
            }
        }
        .padding(pad)
    }
}

/// 字幕字体：样式里选了就用它，没选跟歌词字体走；粗细用样式里的
@MainActor
func karaokeFont(_ style: KaraokeStyle, _ display: LyricsDisplay, size: Double) -> Font {
    var d = display
    if !style.fontFamily.isEmpty { d.fontFamily = style.fontFamily }
    let weights: [Int: (NSFont.Weight, Int)] = [
        400: (.regular, 5), 500: (.medium, 6), 600: (.semibold, 8), 700: (.bold, 9), 800: (.heavy, 10), 900: (.black, 11),
    ]
    let (weight, familyWeight) = weights[style.fontWeight] ?? (.bold, 9)
    return lyricFont(d, size: size, weight: weight, familyWeight: familyWeight)
}
