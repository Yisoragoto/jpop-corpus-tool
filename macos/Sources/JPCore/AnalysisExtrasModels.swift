import Foundation

public struct NgramQuery: Codable, Hashable, Sendable {
    public var scope: AnalysisScope
    public var n: Int
    public var field: MatchField
    public var minFreq: Int64
    public var minSongs: Int64
    public var contains: String?
    public var limit: Int?

    public init(scope: AnalysisScope = AnalysisScope(), n: Int = 3, field: MatchField = .lemma,
                minFreq: Int64 = 3, minSongs: Int64 = 2, contains: String? = nil, limit: Int? = 200) {
        self.scope = scope; self.n = n; self.field = field; self.minFreq = minFreq
        self.minSongs = minSongs; self.contains = contains; self.limit = limit
    }
}

public struct Ngram: Codable, Hashable, Identifiable, Sendable {
    public var words: [String]
    public var surface: String
    public var pos: [String]
    public var freq: Int64
    public var per10k: Double
    public var songs: Int64
    public var range: Double
    public var dispersion: Dispersion
    public var exampleUtteranceId: Int64
    public var exampleSongId: String
    public var id: [String] { words }
}

public struct NgramResult: Codable, Hashable, Sendable {
    public var n: Int
    public var types: Int
    public var items: [Ngram]
    public var provenance: Provenance
}

public struct InflectionQuery: Codable, Hashable, Sendable {
    public var scope: AnalysisScope
    public var pos: String?
    public var examples: Int?

    public init(scope: AnalysisScope = AnalysisScope(), pos: String? = nil, examples: Int? = nil) {
        self.scope = scope; self.pos = pos; self.examples = examples
    }
}

/// Rust 的元组序列化为数组，不能按对象解码。
public struct MorphologyExample: Codable, Hashable, Sendable {
    public var word: String
    public var count: Int64

    public init(word: String, count: Int64) { self.word = word; self.count = count }

    public init(from decoder: Decoder) throws {
        var values = try decoder.unkeyedContainer()
        word = try values.decode(String.self)
        count = try values.decode(Int64.self)
        guard values.isAtEnd else {
            throw DecodingError.dataCorruptedError(in: values, debugDescription: "例词必须是 [词, 次数]")
        }
    }

    public func encode(to encoder: Encoder) throws {
        var values = encoder.unkeyedContainer()
        try values.encode(word)
        try values.encode(count)
    }
}

public struct InflectionForm: Codable, Hashable, Identifiable, Sendable {
    public var cform: String
    public var coarse: String
    public var freq: Int64
    public var share: Double
    public var songs: Int64
    public var lemmas: [MorphologyExample]
    public var surfaces: [MorphologyExample]
    public var id: String { cform }
}

public struct InflectionResult: Codable, Hashable, Sendable {
    public var total: Int64
    public var unannotated: Int64
    public var forms: [InflectionForm]
    public var provenance: Provenance
}

public struct OrthographyQuery: Codable, Hashable, Sendable {
    public var scope: AnalysisScope
    public var minFreq: Int64
    public var minFormFreq: Int64
    public var contentOnly: Bool
    public var limit: Int?

    public init(scope: AnalysisScope = AnalysisScope(), minFreq: Int64 = 5, minFormFreq: Int64 = 1,
                contentOnly: Bool = true, limit: Int? = 200) {
        self.scope = scope; self.minFreq = minFreq; self.minFormFreq = minFormFreq
        self.contentOnly = contentOnly; self.limit = limit
    }
}

public enum Writing: String, Codable, CaseIterable, Sendable {
    case kanji, hiragana, katakana, kana, other
}

public struct WritingForm: Codable, Hashable, Identifiable, Sendable {
    public var lemma: String
    public var writing: Writing
    public var freq: Int64
    public var songs: Int64
    public var exampleUtteranceId: Int64
    public var exampleSongId: String
    public var id: String { lemma }
}

public struct VariantGroup: Codable, Hashable, Identifiable, Sendable {
    public var normalized: String
    public var reading: String
    public var pos: String
    public var freq: Int64
    public var forms: [WritingForm]
    public var mergedByReading: Bool
    public var id: [String] { [normalized, pos] }
}

public struct OrthographyResult: Codable, Hashable, Sendable {
    public var groups: [VariantGroup]
    public var totalGroups: Int
    public var unannotated: Int64
    public var provenance: Provenance
}

public struct CqlQuery: Codable, Hashable, Sendable {
    public var query: String
    public var subcorpus: Subcorpus
    public var dedup: Bool
    public var crossLine: Bool
    public var limit: Int64?

    public init(query: String, subcorpus: Subcorpus = Subcorpus(), dedup: Bool = false,
                crossLine: Bool = false, limit: Int64? = nil) {
        self.query = query; self.subcorpus = subcorpus; self.dedup = dedup
        self.crossLine = crossLine; self.limit = limit
    }
}

public enum BreakdownGroup: String, Codable, CaseIterable, Sendable { case year, performer }

public struct BreakdownQuery: Codable, Hashable, Sendable {
    public var by: BreakdownGroup
    public var kwic: KwicQuery?
    public var cql: CqlQuery?

    public init(by: BreakdownGroup = .year, kwic: KwicQuery? = nil, cql: CqlQuery? = nil) {
        self.by = by; self.kwic = kwic; self.cql = cql
    }
}

public struct BreakdownRow: Codable, Hashable, Identifiable, Sendable {
    public var key: String
    public var label: String
    public var hits: Int64
    public var hitSongs: Int64
    public var tokens: Int64
    public var songs: Int64
    public var per10k: Double
    public var ratio: Double
    public var id: String { key }
}

public struct BreakdownResult: Codable, Hashable, Sendable {
    public var by: BreakdownGroup
    public var rows: [BreakdownRow]
    public var hits: Int64
    public var tokens: Int64
    public var songs: Int64
    public var per10k: Double
}

public struct NetworkQuery: Codable, Hashable, Sendable {
    public var collocation: CollocationQuery
    public var size: Int
    public var minPair: Int64
    public var perNode: Int

    public init(collocation: CollocationQuery, size: Int = 20, minPair: Int64 = 2, perNode: Int = 3) {
        self.collocation = collocation; self.size = size; self.minPair = minPair; self.perNode = perNode
    }
}

public struct NetworkNode: Codable, Hashable, Sendable {
    public var lemma: String
    public var pos: String
    public var freq: Int64
    public var cofreq: Int64
    public var logDice: Double
    public var score: Double
}

public struct NetworkEdge: Codable, Hashable, Sendable {
    public var a: Int
    public var b: Int
    public var freq: Int64
    public var logDice: Double
}

public struct NetworkResult: Codable, Hashable, Sendable {
    public var node: String
    public var nodeFreq: Int64
    public var nodes: [NetworkNode]
    public var edges: [NetworkEdge]
    public var provenance: Provenance
}
