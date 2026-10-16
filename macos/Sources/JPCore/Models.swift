import Foundation

// 数据模型。字段名和 Rust 那边 serde 的 camelCase 一一对应——
// 也就是 Tauri 前端 api.ts 里的同一套名字。三处（Rust、TS、Swift）以 Rust 为准；
// Rust 多出来的字段这里不声明也没关系（解码时忽略），少了才会报错。
// 跨语言的形状由 Tests/JPCoreTests 对着合成语料库逐个解码来守。

// MARK: - 曲库

public struct Track: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var title: String
    public var artist: String
    public var album: String
    public var albumId: Int64?
    public var year: String
    public var genre: String
    public var audioPath: String
    public var coverPath: String
    public var durationSec: Double?
    /// 歌词行数。0 表示还没有歌词
    public var lineCount: Int64
}

public struct LineToken: Codable, Hashable, Sendable {
    public var surface: String
    public var lemma: String
    /// UPOS，空串表示没定出词性
    public var pos: String
}

public struct LyricLine: Codable, Hashable, Identifiable, Sendable {
    public var utteranceId: Int64
    public var lineIdx: Int64
    /// 秒。nil 表示这份 LRC 没有时间轴
    public var timeSec: Double?
    public var text: String
    public var tokens: [LineToken]
    public var id: Int64 { utteranceId }
}

public struct PersonSummary: Codable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var name: String
    public var role: String
    public var trackCount: Int64
    public var imagePath: String
}

public struct Overview: Codable, Hashable, Sendable {
    public var tracks: Int64
    public var albums: Int64
    public var people: Int64
    public var lyricLines: Int64
    public var tokens: Int64
    public var vocabulary: Int64
    public var totalDurationSec: Double
}

// MARK: - 检索

public enum MatchField: String, Codable, CaseIterable, Sendable {
    /// 按写法匹配
    case surface
    /// 按辞书形匹配（駆ける / 駆けた / 駆けろ 归到一起）
    case lemma
}

/// 子语料。条件之间 AND，同一条件内的多个值 OR。全空 = 整个语料库
public struct Subcorpus: Codable, Hashable, Sendable {
    public var performerIds: [Int64] = []
    public var yearFrom: Int?
    public var yearTo: Int?
    public var albumIds: [Int64] = []
    public var genres: [String] = []
    public var songIds: [String] = []

    public init(performerIds: [Int64] = [], yearFrom: Int? = nil, yearTo: Int? = nil, songIds: [String] = []) {
        self.performerIds = performerIds
        self.yearFrom = yearFrom
        self.yearTo = yearTo
        self.songIds = songIds
    }

    public var isAll: Bool {
        performerIds.isEmpty && yearFrom == nil && yearTo == nil && albumIds.isEmpty && genres.isEmpty && songIds.isEmpty
    }
}

public struct NearFilter: Codable, Hashable, Sendable {
    public var word: String
    public var field: MatchField = .lemma
    public var window: Int = 3

    public init(word: String, field: MatchField = .lemma, window: Int = 3) {
        self.word = word
        self.field = field
        self.window = window
    }
}

public struct KwicQuery: Codable, Hashable, Sendable {
    /// 多个候选词；`A+B` 是连续的多词表达（て+しまう）
    public var keywords: [String]
    public var field: MatchField = .surface
    public var pos: String?
    public var personIds: [Int64] = []
    public var crossLine = false
    public var dedup = true
    public var jpOnly = false
    public var limit: Int? = 5000
    public var subcorpus = Subcorpus()
    public var near: NearFilter?

    public init(keywords: [String], field: MatchField = .surface) {
        self.keywords = keywords
        self.field = field
    }

    /// 输入框里的字符串 → 关键词。空格或 `|` 分隔多个，`+` 两边的空格去掉（整体是一个表达）
    public static func parseKeywords(_ raw: String) -> [String] {
        var s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.hasPrefix("("), s.hasSuffix(")") { s = String(s.dropFirst().dropLast()) }
        s = s.replacingOccurrences(of: #"\s*\+\s*"#, with: "+", options: .regularExpression)
        return s.split(whereSeparator: { $0 == "|" || $0.isWhitespace }).map(String.init)
    }
}

/// 行的结束时间是怎么来的
public enum SpanEnd: String, Codable, Sendable {
    /// 下一行开始
    case nextLine
    /// 离下一行太远（间奏），按字数估的
    case estimated
    /// 曲目结束
    case trackEnd
}

public struct KwicHit: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var artist: String
    public var title: String
    public var audioPath: String
    public var utteranceId: Int64
    public var lineIdx: Int64
    public var timeSec: Double?
    public var text: String
    public var left: String
    public var keyword: String
    public var right: String
    /// 同一首歌里折叠掉的重复次数
    public var repeatCount: Int64
    public var corrected: Bool
    /// 这一行在音频里的结束时间（行级）
    public var endSec: Double?
    public var endKind: SpanEnd?
    public var matchLen: Int64

    public var id: String { "\(utteranceId)-\(keyword)-\(left.count)" }
}

public struct LyricHit: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var artist: String
    public var title: String
    public var utteranceId: Int64
    public var timeSec: Double?
    public var text: String
    public var rank: Double
    public var id: Int64 { utteranceId }
}

public struct WordExample: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var artist: String
    public var title: String
    public var utteranceId: Int64
    public var timeSec: Double?
    public var text: String
    public var id: Int64 { utteranceId }
}

public struct PosCount: Codable, Hashable, Sendable {
    public var pos: String
    public var count: Int64
}

public struct WordInCorpus: Codable, Hashable, Sendable {
    public var lemma: String
    public var occurrences: Int64
    public var songCount: Int64
    public var artistCount: Int64
    public var firstYear: String?
    public var lastYear: String?
    public var posDistribution: [PosCount]
    public var examples: [WordExample]
}

// MARK: - 定位

public struct LineSpan: Codable, Hashable, Sendable {
    public var utteranceId: Int64
    public var lineIdx: Int64
    public var startSec: Double
    public var endSec: Double
    public var endKind: SpanEnd
}

/// 试听区间：起播（含提前量）、到哪儿停、命中的是哪一行
public struct ListenWindow: Codable, Hashable, Sendable {
    public var songId: String
    public var startSec: Double
    public var endSec: Double
    public var focusUtteranceId: Int64
    public var focusStartSec: Double
    public var focusEndSec: Double
    public var utteranceIds: [Int64]
}

public struct PlayEvent: Codable, Hashable, Sendable {
    public var songId: String
    public var listenedSec: Double
    public var positionSec: Double
    public var completed: Bool
    /// library / search / research
    public var source: String

    public init(songId: String, listenedSec: Double, positionSec: Double, completed: Bool, source: String) {
        self.songId = songId
        self.listenedSec = listenedSec
        self.positionSec = positionSec
        self.completed = completed
        self.source = source
    }
}

// MARK: - 分析

public enum CountUnit: String, Codable, CaseIterable, Sendable {
    /// 唱几遍算几遍
    case tokens
    /// 同一首歌里重复的行只算第一次
    case distinctLines
}

public struct AnalysisScope: Codable, Hashable, Sendable {
    public var subcorpus = Subcorpus()
    public var unit: CountUnit = .tokens
    public var jpOnly = false

    public init(subcorpus: Subcorpus = Subcorpus(), unit: CountUnit = .tokens, jpOnly: Bool = false) {
        self.subcorpus = subcorpus
        self.unit = unit
        self.jpOnly = jpOnly
    }
}

public struct CorpusSnapshot: Codable, Hashable, Sendable {
    public var schemaVersion: Int64
    public var songs: Int64
    public var lines: Int64
    public var tokens: Int64
    public var corrections: Int64
    public var fingerprint: String
}

/// 结果的出处：库指纹 + 规则版本 + 范围。指纹和参数相同，结果就相同
public struct Provenance: Codable, Hashable, Sendable {
    public var snapshot: CorpusSnapshot
    public var rules: String
    public var scope: String
    public var unit: CountUnit
    public var songs: Int64
    public var tokens: Int64
}

public struct FrequencyQuery: Codable, Hashable, Sendable {
    public var scope = AnalysisScope()
    public var pos: String?
    public var limit: Int? = 500

    public init(scope: AnalysisScope = AnalysisScope(), pos: String? = nil, limit: Int? = 500) {
        self.scope = scope
        self.pos = pos
        self.limit = limit
    }
}

public struct FrequencyItem: Codable, Hashable, Identifiable, Sendable {
    public var lemma: String
    public var pos: String
    public var freq: Int64
    public var per10k: Double
    public var rawFreq: Int64
    public var distinctFreq: Int64
    /// 来自重复行的比例。越高越是副歌堆出来的
    public var repeatShare: Double
    public var songs: Int64
    public var range: Double
    public var surfaces: [String]
    public var id: String { "\(lemma)|\(pos)" }
}

public struct FrequencyResult: Codable, Hashable, Sendable {
    public var items: [FrequencyItem]
    public var provenance: Provenance
}

public enum CollocationMeasure: String, Codable, CaseIterable, Sendable {
    case logDice, mi, logLikelihood, frequency
}

public struct CollocationQuery: Codable, Hashable, Sendable {
    public var scope = AnalysisScope()
    public var node: String
    public var field: MatchField = .lemma
    public var left = 3
    public var right = 3
    public var minFreq: Int64 = 2
    public var collocatePos: String?
    public var contentOnly = true
    public var measure: CollocationMeasure = .logDice
    public var limit: Int? = 60

    public init(node: String, field: MatchField = .lemma, scope: AnalysisScope = AnalysisScope()) {
        self.node = node
        self.field = field
        self.scope = scope
    }
}

public struct Collocate: Codable, Hashable, Identifiable, Sendable {
    public var lemma: String
    public var pos: String
    public var freq: Int64
    public var freqLeft: Int64
    public var freqRight: Int64
    public var collocateFreq: Int64
    public var songs: Int64
    public var logDice: Double
    public var mi: Double
    public var logLikelihood: Double
    public var tScore: Double
    public var exampleUtteranceId: Int64
    public var exampleSongId: String
    public var id: String { "\(lemma)|\(pos)" }

    public func score(_ measure: CollocationMeasure) -> Double {
        switch measure {
        case .logDice: return logDice
        case .mi: return mi
        case .logLikelihood: return logLikelihood
        case .frequency: return Double(freq)
        }
    }
}

public struct CollocationResult: Codable, Hashable, Sendable {
    public var nodeFreq: Int64
    public var collocates: [Collocate]
    public var provenance: Provenance
}

public struct KeynessQuery: Codable, Hashable, Sendable {
    public var target: Subcorpus
    /// nil = 语料库里其余的歌
    public var reference: Subcorpus?
    public var unit: CountUnit = .distinctLines
    public var jpOnly = true
    public var pos: String?
    public var minFreq: Int64 = 5
    public var limit: Int? = 30

    public init(target: Subcorpus, reference: Subcorpus? = nil) {
        self.target = target
        self.reference = reference
    }
}

public struct KeyItem: Codable, Hashable, Identifiable, Sendable {
    public var lemma: String
    public var pos: String
    public var targetFreq: Int64
    public var referenceFreq: Int64
    public var targetPer10k: Double
    public var referencePer10k: Double
    /// 带符号：正 = 目标里偏多
    public var logLikelihood: Double
    public var logRatio: Double
    public var targetSongs: Int64
    public var id: String { "\(lemma)|\(pos)" }
}

public struct KeynessResult: Codable, Hashable, Sendable {
    public var over: [KeyItem]
    public var under: [KeyItem]
    public var target: Provenance
    public var reference: Provenance
}

public struct SongProfile: Codable, Hashable, Sendable {
    public var songId: String
    public var lines: Int64
    public var repeatedLines: Int64
    public var tokens: Int64
    public var distinctTokens: Int64
    public var keywords: [KeyItem]
    public var provenance: Provenance
}

/// UPOS → 中文名。和 Tauri 前端的 POS_LABELS 一致
public let posLabels: [String: String] = [
    "NOUN": "名词", "PROPN": "专名", "VERB": "动词", "ADJ": "形容词", "ADV": "副词",
    "AUX": "助动词", "PRON": "代词", "DET": "连体词", "NUM": "数词", "ADP": "助词",
    "SCONJ": "接续助词", "CCONJ": "接续词", "PART": "终助词", "INTJ": "感叹词",
]
