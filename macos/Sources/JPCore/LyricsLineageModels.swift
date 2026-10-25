import Foundation

public struct LyricsIssue: Decodable, Hashable, Sendable {
    public var code: String
    public var severity: String
    public var lineIdx: Int?
    public var message: String
}
public struct LyricsLanguageGuess: Decodable, Hashable, Sendable {
    public var code: String
    public var confidence: Double
}
public struct LyricsValidation: Decodable, Hashable, Sendable {
    public var lineCount: Int
    public var timedLineCount: Int
    public var completeness: Double
    public var repeatedLineRatio: Double
    public var language: LyricsLanguageGuess
    public var issues: [LyricsIssue]
}
public struct LyricLineQuality: Decodable, Hashable, Sendable {
    public var utteranceId: Int64
    public var lineIdx: Int
    public var timeSec: Double?
    public var timeConfidence: Double?
    public var tokenStatus: String?
    public var tokenizerRunId: Int64?
}
public struct LyricTokenizerRun: Decodable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var fingerprint: String
    public var engine: String
    public var engineVersion: String
    public var dictDescription: String
    public var dictCreatedAt: String
    public var dictSize: UInt64
    public var splitMode: String
    public var configHash: String
    public var posScheme: String
    public var lineCount: Int
}
public indirect enum LyricJSON: Decodable, Hashable, Sendable {
    case object([String: LyricJSON]), array([LyricJSON]), string(String), number(Double), bool(Bool), null
    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let value = try? c.decode(Bool.self) { self = .bool(value) }
        else if let value = try? c.decode(Double.self) { self = .number(value) }
        else if let value = try? c.decode(String.self) { self = .string(value) }
        else if let value = try? c.decode([String: LyricJSON].self) { self = .object(value) }
        else { self = .array(try c.decode([LyricJSON].self)) }
    }
    public var text: String {
        switch self {
        case .string(let s): return s
        case .number(let n): return String(format: "%g", n)
        case .bool(let b): return String(b)
        case .null: return ""
        case .array(let a): return a.map(\.text).joined(separator: "，")
        case .object(let o): return o.keys.sorted().map { "\($0)：\(o[$0]!.text)" }.joined(separator: "，")
        }
    }
    public subscript(_ key: String) -> LyricJSON? { if case .object(let o) = self { return o[key] }; return nil }
}
public struct LyricRevision: Decodable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var kind: String
    public var lineIdx: Int?
    public var after: LyricJSON?
    public var createdAt: String
}
