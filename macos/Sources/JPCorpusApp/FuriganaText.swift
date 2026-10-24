import JPCore
import SwiftUI
import JPCorpusModel

/// 带振假名的一行歌词。注音位置是 Rust 按字符（Unicode 码点）算的，这里也按码点切。
///
/// 每个不注音的字、每段注了音的字各是一块，交给 [`FlowLayout`] 排：一行放不下就在块之间换行，
/// 注音永远跟着它的汉字走。不注音的字上面留一样高的空，整行的底线对齐。
/// 没有注音时直接用一个 `Text`，换行交给系统（比一块一块排更自然）
@MainActor
struct FuriganaText: View {
    @Environment(AppModel.self) private var app
    let text: String
    let rubies: [Ruby]
    var size: CGFloat = 15
    var weight: Font.Weight = .bold
    var alignment: HorizontalAlignment = .leading

    var body: some View {
        let scaled = Double(size) * app.lyricsDisplay.fontSize / 15
        if rubies.isEmpty {
            Text(text)
                .font(lyricFont(app.lyricsDisplay, size: scaled))
                .kerning(app.lyricsDisplay.letterSpacing)
                .fixedSize(horizontal: false, vertical: true)
        } else {
            FlowLayout(spacing: CGFloat(app.lyricsDisplay.letterSpacing), lineSpacing: CGFloat(app.lyricsDisplay.lineGap), alignment: alignment) {
                ForEach(Array(Self.pieces(text, rubies).enumerated()), id: \.offset) { _, piece in
                    VStack(spacing: 0) {
                        Text(piece.ruby ?? " ")
                            .font(lyricFont(app.lyricsDisplay, size: app.lyricsDisplay.rubySize * Double(size) / 15, weight: .medium))
                            .opacity(piece.ruby == nil ? 0 : 0.85)
                            .lineLimit(1)
                            .fixedSize()
                        Text(piece.base)
                            .font(lyricFont(app.lyricsDisplay, size: scaled))
                            .fixedSize()
                    }
                }
            }
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
