import JPCore
import JPCorpusModel
import SwiftUI

/// KTV 一行（对应 Windows 的 `.ktv-row` + paintKaraoke）：不换行，没唱的白字、唱过的亮蓝，
/// 字外一圈深色投影（封面再亮也读得清），振假名在上面跟着变色。
///
/// 单位内按码点顺序扫亮，原始单位时间不变；没有逐字时间的行到点整行亮起，不按字数估算。
/// 每帧只有正在唱的那一个字带遮罩（缩放一个矩形，不用 GeometryReader），其余的字只换颜色：
/// 以前每个字都挂一个 GeometryReader 遮罩、每秒重排 30 次，是卡拉 OK 时 CPU 高的主因
@MainActor
struct KaraokeLyricText: View {
    @Environment(AppModel.self) private var app
    let line: LyricLine
    let units: [LyricTimingUnit]?
    let sec: Double
    var size: CGFloat = 34

    static let sung = Color(red: 0x4c / 255, green: 0xc2 / 255, blue: 1)
    static let unsung = Color.white

    private struct Part: Identifiable {
        var start: Int
        var chars: [Unicode.Scalar]
        var reading: String?
        var id: Int { start }
    }
    private var parts: [Part] {
        let chars = Array(line.text.unicodeScalars)
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
        let hasRuby = app.showFurigana && !(app.furigana[line.utteranceId] ?? []).isEmpty
        HStack(alignment: .bottom, spacing: 0) {
            ForEach(parts) { part in
                VStack(spacing: 0) {
                    if hasRuby {
                        Text(part.reading ?? " ")
                            .font(.system(size: size * 0.38, weight: .semibold))
                            .foregroundStyle(rubyLit(part) ? Self.sung : Self.unsung)
                            .opacity(part.reading == nil ? 0 : 1)
                    }
                    HStack(spacing: 0) {
                        ForEach(Array(part.chars.enumerated()), id: \.offset) { offset, ch in
                            glyph(String(ch), at: part.start + offset)
                        }
                    }
                }
                .fixedSize()
            }
        }
        .font(lyricFont(app.lyricsDisplay, size: size, weight: .bold))
        .lineLimit(1)
        // 两层投影当描边，和 Windows 的 drop-shadow 一样
        .shadow(color: .black.opacity(0.85), radius: 1)
        .shadow(color: .black.opacity(0.85), radius: 4, y: 2)
        .animation(nil, value: sec)
    }

    private func progress(at index: Int) -> Double {
        if units == nil { return wholeLineOn ? 1 : 0 }
        guard let unit = units?.first(where: { $0.charStart <= index && index < $0.charEnd }) else { return 0 }
        let local = unit.progress(at: sec) * Double(unit.charEnd - unit.charStart) - Double(index - unit.charStart)
        return min(1, max(0, local))
    }
    private func rubyLit(_ part: Part) -> Bool {
        (part.start..<(part.start + part.chars.count)).contains { progress(at: $0) > 0 }
    }

    @ViewBuilder private func glyph(_ text: String, at index: Int) -> some View {
        let p = progress(at: index)
        if p <= 0 {
            Text(text).foregroundStyle(Self.unsung)
        } else if p >= 1 {
            Text(text).foregroundStyle(Self.sung)
        } else {
            Text(text).foregroundStyle(Self.unsung)
                .overlay {
                    Text(text).foregroundStyle(Self.sung)
                        .mask(alignment: .leading) { Rectangle().scaleEffect(x: p, anchor: .leading) }
                }
        }
    }
}
