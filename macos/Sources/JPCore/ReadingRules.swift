import Foundation

/// 用户改过的读音（#63，Rust 的 `jp_corpus::ruby::ReadingRule`）：按写法记，只管这首歌或整个库。
/// 振假名（`lyricsFurigana`）会盖上它：这首歌的优先，其次是全库的。
public struct ReadingRule: Codable, Hashable, Sendable {
    /// 全库的规则这里是空串
    public var songId: String
    public var surface: String
    public var reading: String
    public var wholeLibrary: Bool
}

extension CorpusClient {
    /// 这首歌里这个写法现在用的规则（本首的优先，其次全库的）；没有改过是 nil
    public func readingRule(songId: String, surface: String) async throws -> ReadingRule? {
        try await perform { try $0.call("readingRule", ["songId": AnyEncodable(songId), "surface": AnyEncodable(surface)]) }
    }

    /// 改读音。`wholeLibrary` 时整个库里这个写法都这么读
    public func setReadingRule(songId: String, surface: String, reading: String, wholeLibrary: Bool) async throws -> ReadingRule {
        try await perform {
            try $0.call("readingRuleSet", [
                "songId": AnyEncodable(songId), "surface": AnyEncodable(surface),
                "reading": AnyEncodable(reading), "wholeLibrary": AnyEncodable(wholeLibrary),
            ])
        }
    }

    /// 去掉现在起作用的那条规则，返回去掉的那条
    @discardableResult
    public func clearReadingRule(songId: String, surface: String) async throws -> ReadingRule? {
        try await perform { try $0.call("readingRuleClear", ["songId": AnyEncodable(songId), "surface": AnyEncodable(surface)]) }
    }
}
