import Foundation

// 词典管理（导入 Yomitan 词典包、启用、排序、删除）和一键制卡（AnkiConnect）。
// 方法名和 jp-ffi 一致；制卡的逻辑是 Rust 的 jp_anki::lyric_mine，和 Windows 版同一份

// MARK: - 词典导入

public struct DictImportSummary: Codable, Hashable, Sendable {
    public var dictionaryId: Int64
    public var title: String
    public var terms: Int64
    public var media: Int64
}

public struct DictImportResult: Codable, Hashable, Sendable {
    public var path: String
    public var summary: DictImportSummary?
    public var error: String?
}

public struct DictImportBatch: Codable, Hashable, Sendable {
    public var results: [DictImportResult]
}

// MARK: - Anki

/// 卡片样式（Lapis 的五种）
public enum LapisCardKind: String, Codable, CaseIterable, Sendable {
    case vocab, wordAndSentence, click, sentence, audio

    public var label: String {
        switch self {
        case .vocab: return "单词卡"
        case .wordAndSentence: return "词+句卡"
        case .click: return "点击卡"
        case .sentence: return "句子卡"
        case .audio: return "音频卡"
        }
    }
}

/// 应用自己建的笔记类型（照 Lapis 做的，第一次制卡时自动建）
public let lyricsNoteType = "Lyrics"

public struct AnkiStatus: Codable, Hashable, Sendable {
    public var connected: Bool
    public var version: Int64
    public var decks: [String]
    public var models: [String]
    /// 连不上时给人看的指引
    public var message: String
}

/// 这张卡从哪来、放到哪去（Rust `MineRequest`）
public struct MineRequest: Codable, Hashable, Sendable {
    /// 查词用的那段文字（从点的词开始到行尾）
    public var lookupText: String
    /// 选中词条的第一个词头
    public var term: String
    public var reading: String
    public var songId: String?
    public var utteranceId: Int64?
    /// 点的是这一行的第几个词
    public var tokenIndex: Int?
    public var deck: String
    public var artistSubdeck: Bool
    public var model: String
    public var mainDictionary: String?
    public var kind: LapisCardKind

    public init(lookupText: String, term: String, reading: String, songId: String?, utteranceId: Int64?, tokenIndex: Int?,
                deck: String, artistSubdeck: Bool, model: String, mainDictionary: String?, kind: LapisCardKind) {
        self.lookupText = lookupText
        self.term = term
        self.reading = reading
        self.songId = songId
        self.utteranceId = utteranceId
        self.tokenIndex = tokenIndex
        self.deck = deck
        self.artistSubdeck = artistSubdeck
        self.model = model
        self.mainDictionary = mainDictionary
        self.kind = kind
    }
}

public enum MineOutcome: Decodable, Hashable, Sendable {
    /// 加上了。`deck` 是实际放进的牌组；`warnings` 是加上了但有东西没做成（比如例句音频没切出来）
    case added(noteId: Int64, deck: String, deckCreated: Bool, skippedFields: [String], warnings: [String])
    /// 已经有这个词的卡了，没有再加
    case duplicate(noteIds: [Int64])

    private enum Keys: String, CodingKey {
        case kind, noteId, deck, deckCreated, skippedFields, warnings, noteIds
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "added":
            self = .added(noteId: try c.decode(Int64.self, forKey: .noteId), deck: try c.decode(String.self, forKey: .deck),
                          deckCreated: try c.decode(Bool.self, forKey: .deckCreated),
                          skippedFields: try c.decode([String].self, forKey: .skippedFields),
                          warnings: try c.decode([String].self, forKey: .warnings))
        case "duplicate":
            self = .duplicate(noteIds: try c.decode([Int64].self, forKey: .noteIds))
        case let other:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "不认识的制卡结果 \(other)")
        }
    }
}

public struct MineCheck: Codable, Hashable, Sendable {
    public var connected: Bool
    public var message: String
    /// 和传入的词头一一对应：已有卡片的笔记 id
    public var notes: [[Int64]]
}

extension CorpusClient {
    // ── 词典：都不拿语料库的锁（走并发队列），导入跑着时照样能查词、检索 ──

    public func dictImport(_ paths: [String]) async throws {
        try await performDetached { try $0.run("dictImport", ["paths": AnyEncodable(paths)]) }
    }

    public func dictImportProgress() async throws -> JobProgress<DictImportBatch> {
        try await performDetached { try $0.call("dictImportProgress") }
    }

    public func dictImportCancel() async throws {
        try await performDetached { try $0.run("dictImportCancel") }
    }

    public func dictSetEnabled(_ id: Int64, _ enabled: Bool) async throws {
        try await performDetached { try $0.run("dictSetEnabled", ["id": AnyEncodable(id), "enabled": AnyEncodable(enabled)]) }
    }

    /// `ids` 是从上到下的新顺序（排在前面的词典释义先出）
    public func dictSetOrder(_ ids: [Int64]) async throws {
        try await performDetached { try $0.run("dictSetOrder", ["ids": AnyEncodable(ids)]) }
    }

    public func dictDelete(_ id: Int64) async throws {
        try await performDetached { try $0.run("dictDelete", ["id": AnyEncodable(id)]) }
    }

    // ── Anki：HTTP 到本机，几十毫秒到一两秒，同样不占语料库的队列 ──

    public func ankiStatus() async throws -> AnkiStatus {
        try await performDetached { try $0.call("ankiStatus") }
    }

    public func ankiMineCheck(model: String, expressions: [String]) async throws -> MineCheck {
        try await performDetached {
            try $0.call("ankiMineCheck", ["model": AnyEncodable(model), "expressions": AnyEncodable(expressions)])
        }
    }

    public func ankiMine(_ request: MineRequest) async throws -> MineOutcome {
        try await performDetached { try $0.call("ankiMine", ["request": AnyEncodable(request)]) }
    }
}
