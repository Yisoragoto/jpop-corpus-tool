import Foundation
import JPCore

/// 把一行歌词拆成「词之间的原文」和「词」，词里再按振假名拆成带注音 / 不带注音的几段。
/// 和 Windows 版 `app/src/lyricSegments.ts` 是同一套规则：
///
/// - 词来自库里的分词（点了能查词），振假名来自现场分词；两者都按原文的字符（Unicode 码点）位置对齐；
/// - 词与词之间只允许夹空白；对不齐（比如改过歌词没重新分词）返回 nil，调用方退回不可点的显示，不猜位置；
/// - 有一段注音横跨两个词也返回 nil（调用方再不带注音试一次，词照样能点）。
public enum LyricSegments {
    public struct Piece: Equatable, Sendable {
        public var text: String
        /// nil 就是不用注音
        public var reading: String?
        public init(text: String, reading: String? = nil) { self.text = text; self.reading = reading }
    }

    public enum Segment: Equatable, Sendable {
        case gap(String)
        case token(index: Int, pieces: [Piece])
    }

    /// 每个词在原文里的 [起, 止)（码点下标）
    public static func alignTokens(_ chars: [Unicode.Scalar], _ tokens: [LineToken]) -> [Range<Int>]? {
        var spans: [Range<Int>] = []
        var pos = 0
        for token in tokens {
            let surface = Array(token.surface.unicodeScalars)
            if surface.isEmpty {
                spans.append(pos..<pos)
                continue
            }
            var found: Int?
            var p = pos
            while p + surface.count <= chars.count {
                if chars[p..<(p + surface.count)].elementsEqual(surface) {
                    found = p
                    break
                }
                guard chars[p].properties.isWhitespace else { return nil }
                p += 1
            }
            guard let start = found else { return nil }
            spans.append(start..<(start + surface.count))
            pos = start + surface.count
        }
        return spans
    }

    public static func segment(_ text: String, tokens: [LineToken], rubies: [Ruby]) -> [Segment]? {
        let chars = Array(text.unicodeScalars)
        guard let spans = alignTokens(chars, tokens) else { return nil }
        let valid = rubies.allSatisfy { r in
            r.start < r.end && spans.contains { $0.lowerBound <= r.start && r.end <= $0.upperBound }
        }
        guard valid else { return nil }
        func slice(_ a: Int, _ b: Int) -> String {
            var view = String.UnicodeScalarView()
            view.append(contentsOf: chars[a..<b])
            return String(view)
        }
        var out: [Segment] = []
        var cursor = 0
        for (index, span) in spans.enumerated() {
            if span.lowerBound > cursor { out.append(.gap(slice(cursor, span.lowerBound))) }
            var pieces: [Piece] = []
            var at = span.lowerBound
            let inside = rubies.filter { span.lowerBound <= $0.start && $0.end <= span.upperBound }.sorted { $0.start < $1.start }
            for ruby in inside {
                if ruby.start > at { pieces.append(Piece(text: slice(at, ruby.start))) }
                pieces.append(Piece(text: slice(ruby.start, ruby.end), reading: ruby.reading))
                at = ruby.end
            }
            if span.upperBound > at { pieces.append(Piece(text: slice(at, span.upperBound))) }
            out.append(.token(index: index, pieces: pieces))
            cursor = max(cursor, span.upperBound)
        }
        if cursor < chars.count { out.append(.gap(slice(cursor, chars.count))) }
        return out
    }

    /// 能点的拆法：先带注音；注音对不上就不注音（和 Windows 版一样，词照样能点）
    public static func clickable(_ text: String, tokens: [LineToken], rubies: [Ruby]) -> (segments: [Segment], rubiesShown: Bool)? {
        guard !tokens.isEmpty else { return nil }
        if let withRuby = segment(text, tokens: tokens, rubies: rubies) { return (withRuby, !rubies.isEmpty) }
        if rubies.isEmpty { return nil }
        return segment(text, tokens: tokens, rubies: []).map { ($0, false) }
    }

    /// 从第 `index` 个词查起：把这个词到行尾的原文交给词典做最长匹配（Yomitan 的查法）
    public static func lookupText(_ tokens: [LineToken], from index: Int) -> String {
        guard tokens.indices.contains(index) else { return "" }
        return tokens[index...].map(\.surface).joined()
    }

    /// 右键菜单里列出的可查词：除了标点、符号和空白，每个词都列（以前只列名词 / 动词 / 形容词 / 副词，
    /// 助词、助动词、代词、连体词都查不了）
    public static func lookupCandidates(_ tokens: [LineToken]) -> [Int] {
        tokens.indices.filter { i in
            let token = tokens[i]
            guard token.pos != "PUNCT", token.pos != "SYM" else { return false }
            return token.surface.unicodeScalars.contains { !$0.properties.isWhitespace }
        }
    }
}
