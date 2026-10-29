import JPCore
import SwiftUI
import JPCorpusModel

/// 带振假名的一行歌词。注音位置是 Rust 按字符（Unicode 码点）算的，这里也按码点切。
///
/// 每个不注音的字、每段注了音的字各是一块，交给 [`FlowLayout`] 排：一行放不下就在块之间换行，
/// 注音永远跟着它的汉字走。不注音的字上面留一样高的空，整行的底线对齐。
/// 没有注音时直接用一个 `Text`，换行交给系统（比一块一块排更自然）。
///
/// 传了 `tokens` 和 `onToken`：每个词是一块、点了就查词（和 Windows 版一样：点词只查词、不动播放）。
/// 词之间的标点、空白不能点，点它们会落到外层（歌词行的「从这里放」）。
@MainActor
struct FuriganaText: View {
    @Environment(AppModel.self) private var app
    let text: String
    let rubies: [Ruby]
    var size: CGFloat = 15
    var weight: Font.Weight = .bold
    var alignment: HorizontalAlignment = .leading
    var tokens: [LineToken] = []
    var onToken: ((Int) -> Void)?

    var body: some View {
        let display = app.lyricsDisplay
        let scaled = Double(size) * display.fontSize / 15
        if let onToken, let split = LyricSegments.clickable(text, tokens: tokens, rubies: rubies) {
            FlowLayout(spacing: CGFloat(display.letterSpacing), lineSpacing: CGFloat(display.lineGap), alignment: alignment) {
                ForEach(Array(split.segments.enumerated()), id: \.offset) { _, segment in
                    switch segment {
                    case .gap(let gap):
                        ForEach(Array(gap.enumerated()), id: \.offset) { _, ch in
                            piece(LyricSegments.Piece(text: String(ch)), ruby: split.rubiesShown, scaled: scaled)
                        }
                    case .token(let index, let pieces):
                        LyricWord(help: tokens.indices.contains(index) ? tokens[index].lemma : "") {
                            HStack(alignment: .bottom, spacing: CGFloat(display.letterSpacing)) {
                                ForEach(Array(pieces.enumerated()), id: \.offset) { _, p in
                                    piece(p, ruby: split.rubiesShown, scaled: scaled)
                                }
                            }
                        } tap: { onToken(index) }
                    }
                }
            }
        } else if rubies.isEmpty {
            Text(text)
                .font(lyricFont(display, size: scaled))
                .kerning(display.letterSpacing)
                .fixedSize(horizontal: false, vertical: true)
        } else {
            FlowLayout(spacing: CGFloat(display.letterSpacing), lineSpacing: CGFloat(display.lineGap), alignment: alignment) {
                ForEach(Array(Self.pieces(text, rubies).enumerated()), id: \.offset) { _, p in
                    piece(LyricSegments.Piece(text: p.base, reading: p.ruby), ruby: true, scaled: scaled)
                }
            }
        }
    }

    /// 一块：上面注音（没有就留同样高的空），下面原文
    private func piece(_ p: LyricSegments.Piece, ruby: Bool, scaled: Double) -> some View {
        let display = app.lyricsDisplay
        return VStack(spacing: 0) {
            if ruby {
                Text(p.reading ?? " ")
                    .font(lyricFont(display, size: display.rubySize * Double(size) / 15, weight: .medium))
                    .opacity(p.reading == nil ? 0 : 0.85)
                    .lineLimit(1)
                    .fixedSize()
            }
            Text(p.text)
                .font(lyricFont(display, size: scaled))
                .fixedSize()
        }
    }

    struct Piece {
        var base: String
        var ruby: String?
    }

    /// 切成块：注音段整段一块，其余每个字一块。位置越界或交叠的注音丢掉（不猜）
    static func pieces(_ text: String, _ rubies: [Ruby]) -> [Piece] {
        let scalars = Array(text.unicodeScalars)
        func slice(_ a: Int, _ b: Int) -> String {
            var s = String.UnicodeScalarView()
            s.append(contentsOf: scalars[a..<b])
            return String(s)
        }
        var out: [Piece] = []
        var i = 0
        for r in rubies.sorted(by: { $0.start < $1.start }) where r.start >= i && r.end <= scalars.count && r.start < r.end {
            // 注音前面的字：按「字」切（合成字、带浊点的假名不拆开）
            out += slice(i, r.start).map { Piece(base: String($0)) }
            out.append(Piece(base: slice(r.start, r.end), ruby: r.reading))
            i = r.end
        }
        out += slice(i, scalars.count).map { Piece(base: String($0)) }
        return out
    }
}

/// 歌词里能点的一个词：悬停时淡淡地托一下底，让人知道点得动
@MainActor
private struct LyricWord<Content: View>: View {
    let help: String
    @ViewBuilder let content: () -> Content
    let tap: () -> Void
    @State private var hover = false

    var body: some View {
        content()
            .background {
                RoundedRectangle(cornerRadius: 4, style: .continuous)
                    .fill(Color.primary.opacity(hover ? 0.12 : 0))
                    .padding(-2)
            }
            .contentShape(Rectangle())
            .onHover { hover = $0 }
            .onTapGesture(perform: tap)
            .help(help.isEmpty ? "查词" : "查词：\(help)")
    }
}
