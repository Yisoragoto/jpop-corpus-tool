import JPCore
import JPCorpusModel
import SwiftUI

/// 对应 paintKaraoke：单位内按码点顺序扫亮，原始单位时间保持不变。
@MainActor
struct KaraokeLyricText: View {
    @Environment(AppModel.self) private var app
    let line: LyricLine
    let units: [LyricTimingUnit]?
    let sec: Double
    private struct Part: Identifiable {
        var start: Int
        var chars: [Unicode.Scalar]
        var reading: String?
        var id: Int { start }
    }
    private var parts: [Part] {
        let chars = Array(line.text.unicodeScalars)
        let rubies = app.furigana[line.utteranceId] ?? []
        var out: [Part] = [], cursor = 0
        for ruby in rubies.sorted(by: { $0.start < $1.start }) {
            guard ruby.start >= cursor, ruby.start < ruby.end, ruby.end <= chars.count else { continue }
            for i in cursor..<ruby.start { out.append(.init(start: i, chars: [chars[i]])) }
            out.append(.init(start: ruby.start, chars: Array(chars[ruby.start..<ruby.end]), reading: ruby.reading))
            cursor = ruby.end
        }
        for i in cursor..<chars.count { out.append(.init(start: i, chars: [chars[i]])) }
        return out
    }
    var body: some View {
        FlowLayout(spacing: app.lyricsDisplay.letterSpacing, lineSpacing: app.lyricsDisplay.lineGap) {
            ForEach(parts) { part in
                VStack(spacing: 0) {
                    if app.showFurigana {
                        Text(part.reading ?? " ")
                            .font(lyricFont(app.lyricsDisplay, size: app.lyricsDisplay.rubySize * 34 / 15, weight: .medium))
                            .opacity(part.reading == nil ? 0 : 0.85)
                    }
                    HStack(spacing: app.lyricsDisplay.letterSpacing) {
                        ForEach(Array(part.chars.enumerated()), id: \.offset) { offset, ch in
                            glyph(String(ch), at: part.start + offset)
                        }
                    }
                }.fixedSize()
            }
        }
        .font(lyricFont(app.lyricsDisplay, size: app.lyricsDisplay.fontSize * 34 / 15))
        .animation(nil, value: sec)
    }
    private func glyph(_ text: String, at index: Int) -> some View {
        let progress: Double
        if let units, let unit = units.first(where: { $0.charStart <= index && index < $0.charEnd }) {
            let local = unit.progress(at: sec) * Double(unit.charEnd - unit.charStart) - Double(index - unit.charStart)
            progress = (min(1, max(0, local)) * 1000).rounded() / 1000
        } else { progress = line.timeSec.map { sec >= $0 ? 1.0 : 0.0 } ?? 0 }
        return Text(text).foregroundStyle(.white.opacity(0.38))
            .overlay(alignment: .leading) {
                Text(text).foregroundStyle(Color.accentColor).mask {
                    GeometryReader { geo in
                        Rectangle().frame(width: geo.size.width * progress).frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
    }
}
