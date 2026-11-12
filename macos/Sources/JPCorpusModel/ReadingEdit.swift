import Foundation
import JPCore

/// 歌词里一个可以改读音的词（#63）。和 Windows 一样只给带汉字（或「々」）的词改，按写法去重：
/// 规则本来就是按写法记的，同一行里出现两次的词改一次就够。
public struct ReadingEdit: Identifiable, Hashable, Sendable {
    public var songId: String
    public var surface: String
    /// 现在显示的读音：注音的几段换成读音、中间的假名照抄（見つめる → みつめる）；这个词没注音时是空串
    public var shown: String
    public var id: String { songId + "|" + surface }

    public init(songId: String, surface: String, shown: String) {
        self.songId = songId
        self.surface = surface
        self.shown = shown
    }

    public static func hasKanji(_ text: String) -> Bool {
        text.unicodeScalars.contains { $0.properties.isIdeographic || $0 == "々" }
    }

    /// 这一行里能改读音的词，按出现顺序
    public static func candidates(songId: String, line: LyricLine, rubies: [Ruby]) -> [ReadingEdit] {
        var shown: [Int: String] = [:]
        for segment in LyricSegments.clickable(line.text, tokens: line.tokens, rubies: rubies)?.segments ?? [] {
            guard case let .token(index, pieces) = segment, pieces.contains(where: { $0.reading != nil }) else { continue }
            shown[index] = pieces.map { $0.reading ?? $0.text }.joined()
        }
        var seen = Set<String>()
        var out: [ReadingEdit] = []
        for (index, token) in line.tokens.enumerated() where hasKanji(token.surface) && seen.insert(token.surface).inserted {
            out.append(ReadingEdit(songId: songId, surface: token.surface, shown: shown[index] ?? ""))
        }
        return out
    }
}
