import Foundation

/// 五个元音，按段：a = ア段……（Rust 的 `jp_normalize::phonology::Vowel`）
public enum Vowel: String, Codable, Hashable, Sendable, CaseIterable {
    case a, i, u, e, o
}

public struct VowelQuery: Codable, Hashable, Sendable {
    public var scope: AnalysisScope
    /// 单元音词至少几个带元音的拍
    public var minWordMorae: Int

    public init(scope: AnalysisScope = AnalysisScope(), minWordMorae: Int = 2) {
        self.scope = scope; self.minWordMorae = minWordMorae
    }
}

public struct VowelShare: Codable, Hashable, Sendable {
    public var vowel: Vowel
    public var morae: Int64
    public var share: Double
}

public struct UnivocalicWord: Codable, Hashable, Identifiable, Sendable {
    /// 按发音的片假名
    public var kana: String
    public var lemma: String
    /// 最常见的几种写法（Rust 的元组，按数组解码）
    public var surfaces: [MorphologyExample]
    public var morae: Int
    public var freq: Int64
    public var songs: Int64
    public var id: String { kana + "/" + lemma }
}

public struct UnivocalicLine: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var utteranceId: Int64
    public var text: String
    public var kana: String
    public var morae: Int
    public var id: Int64 { utteranceId }
}

public struct UnivocalicGroup: Codable, Hashable, Identifiable, Sendable {
    public var vowel: Vowel
    public var distinctWords: Int64
    public var tokens: Int64
    public var words: [UnivocalicWord]
    public var lineCount: Int64
    public var lines: [UnivocalicLine]
    public var id: Vowel { vowel }
}

public struct VowelResult: Codable, Hashable, Sendable {
    public var vowelMorae: Int64
    public var vowels: [VowelShare]
    public var sokuon: Int64
    public var hatsuon: Int64
    public var long: Int64
    public var words: Int64
    public var skipped: Int64
    public var skippedExamples: [MorphologyExample]
    public var univocalic: [UnivocalicGroup]
    public var provenance: Provenance
}

/// 押韵的位置：行尾（脚韵）、行首（头韵）、行里任何地方
public enum RhymePosition: String, Codable, Hashable, Sendable {
    case end, start, any
}

public struct RhymeQuery: Codable, Hashable, Sendable {
    public var scope: AnalysisScope
    /// 元音模式（a i u e o，n、q、.）或假名
    public var pattern: String
    public var position: RhymePosition
    /// ン、ッ 也要对上
    public var strict: Bool

    public init(scope: AnalysisScope = AnalysisScope(), pattern: String = "", position: RhymePosition = .end, strict: Bool = false) {
        self.scope = scope; self.pattern = pattern; self.position = position; self.strict = strict
    }
}

public struct RhymeHit: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var utteranceId: Int64
    public var timeSec: Double?
    public var text: String
    public var kana: String
    public var matched: String
    /// 押韵的词在 `text` 里的位置，按 Unicode 标量（Rust 的 char）数
    public var charStart: Int?
    public var charEnd: Int?
    public var id: Int64 { utteranceId }
}

public struct RhymeCarrier: Codable, Hashable, Identifiable, Sendable {
    public var text: String
    public var kana: String
    public var lines: Int64
    public var songs: Int64
    public var id: String { text }
}

public struct RhymeResult: Codable, Hashable, Sendable {
    /// 解析后的模式；空串表示输入里没有能比的元音
    public var pattern: String
    public var lines: Int64
    public var songs: Int64
    public var hits: [RhymeHit]
    public var carriers: [RhymeCarrier]
    public var provenance: Provenance
}

public struct RhymeStatsQuery: Codable, Hashable, Sendable {
    public var scope: AnalysisScope
    public var position: RhymePosition
    public var strict: Bool
    /// 韵脚取几个音
    public var tail: Int

    public init(scope: AnalysisScope = AnalysisScope(), position: RhymePosition = .end, strict: Bool = false, tail: Int = 3) {
        self.scope = scope; self.position = position; self.strict = strict; self.tail = tail
    }
}

public struct RhymeLength: Codable, Hashable, Identifiable, Sendable {
    public var length: Int
    public var adjacent: Int64
    public var adjacentRhymed: Int64
    public var adjacentRate: Double
    public var baseline: Int64
    public var baselineRhymed: Int64
    public var baselineRate: Double
    /// 相邻行押韵率 / 任意两行押韵率；基线为 0 时没有
    public var lift: Double?
    public var id: Int { length }
}

public struct RhymePairExample: Codable, Hashable, Sendable {
    public var songId: String
    public var first: String
    public var second: String
}

public struct RhymeTail: Codable, Hashable, Identifiable, Sendable {
    public var tail: String
    public var pairs: Int64
    public var songs: Int64
    public var sameWord: Int64
    public var example: RhymePairExample
    public var id: String { tail }
}

public struct SongRhyme: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var pairs: Int64
    public var rhymed: Int64
    public var rate: Double
    public var id: String { songId }
}

public struct RhymeStatsResult: Codable, Hashable, Sendable {
    public var tail: Int
    public var lengths: [RhymeLength]
    /// 押 `tail` 个音的对里，两行以同一个词收尾的
    public var sameWord: Int64
    public var tails: [RhymeTail]
    public var songs: [SongRhyme]
    public var provenance: Provenance
}

extension CorpusClient {
    public func vowels(_ query: VowelQuery) async throws -> VowelResult {
        try await perform { try $0.call("vowels", ["query": AnyEncodable(query)]) }
    }

    public func rhyme(_ query: RhymeQuery) async throws -> RhymeResult {
        try await perform { try $0.call("rhyme", ["query": AnyEncodable(query)]) }
    }

    public func rhymeStats(_ query: RhymeStatsQuery) async throws -> RhymeStatsResult {
        try await perform { try $0.call("rhymeStats", ["query": AnyEncodable(query)]) }
    }
}
