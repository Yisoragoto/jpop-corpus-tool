import Foundation

public struct WordCandidate: Codable, Hashable, Sendable, Identifiable {
    public var lemma: String; public var pos: String; public var count: Int64; public var songCount: Int64
    public var inAnki: Bool; public var studied: Bool
    public var id: String { lemma }
}
public struct AnkiExample: Codable, Hashable, Sendable {
    public var songId: String; public var artist: String; public var title: String; public var timeSec: Double?
    public var text: String; public var surface: String; public var audioPath: String; public var endSec: Double?; public var utteranceId: Int64
}
public struct AnkiCard: Codable, Hashable, Sendable {
    public var expression: String; public var reading: String; public var meaning: String; public var sentence: String
    public var sentenceAudio: String; public var source: String; public var jlpt: String; public var pitch: String
    public var freq: String; public var partOfSpeech: String; public var lookupTerm: String
    // Rust 对空 examples 使用 skip_serializing_if。
    public var examples: [AnkiExample]?
    public var noExamples: Bool; public var noDefinitions: Bool
}
public struct AnkiProgress: Codable, Hashable, Sendable {
    public var job: String; public var done: Int; public var total: Int; public var lemma: String
    public var outcome: String; public var newSentences: Int; public var message: String
    public var finished: Bool; public var cancelled: Bool; public var error: String
    public init(job: String, total: Int) {
        self.job = job; self.total = total; done = 0; lemma = ""; outcome = ""; newSentences = 0
        message = ""; finished = false; cancelled = false; error = ""
    }
}
public struct AnkiJobResult: Codable, Sendable { public var progress: AnkiProgress; public var log: [AnkiProgress] }
public enum AnkiDupMode: String, Codable, CaseIterable, Sendable { case append, skip }
public enum AnkiDupScope: String, Codable, CaseIterable, Sendable { case deck, root }
public enum AnkiRefreshScope: String, Codable, CaseIterable, Sendable { case deck, deckAndChildren, rootAndChildren }
public struct AnkiExportItem: Codable, Hashable, Sendable {
    public var lemma: String; public var pos: String
    public init(lemma: String, pos: String) { self.lemma = lemma; self.pos = pos }
}
public struct AnkiBatchRequest: Sendable {
    public var words: [AnkiExportItem] = []; public var deck = ""; public var maxExamples = 2; public var maxDicts: Int? = 3
    public var dupMode: AnkiDupMode = .append; public var dupScope: AnkiDupScope = .deck; public var clipAudio = true
    public init() {}
    var arguments: [String: AnyEncodable] {
        ["words": AnyEncodable(words), "deck": AnyEncodable(deck), "maxExamples": AnyEncodable(maxExamples),
         "maxDicts": AnyEncodable(maxDicts), "dupMode": AnyEncodable(dupMode), "dupScope": AnyEncodable(dupScope), "clipAudio": AnyEncodable(clipAudio)]
    }
}
public struct MiningReportResult: Codable, Sendable {
    public var path: String; public var collectionPath: String
    public var cards: Int; public var studied: Int; public var words: Int; public var artists: Int; public var songs: Int; public var decks: Int
    public var noteTypes: [String]; public var openError: String
}
extension CorpusClient {
    public func ankiNotesSince(_ sinceMs: Int64) async throws -> Int? { try await performDetached { try $0.call("ankiNotesSince", ["sinceMs": AnyEncodable(sinceMs)]) } }
    public func ankiWords(pos: [String], skipStudied: Bool, minCount: Int64 = 1, songIds: [String] = [], limit: Int = 300) async throws -> [WordCandidate] {
        try await performDetached { try $0.call("ankiWords", ["pos": AnyEncodable(pos), "skipStudied": AnyEncodable(skipStudied), "minCount": AnyEncodable(minCount), "songIds": AnyEncodable(songIds), "limit": AnyEncodable(limit)]) }
    }
    public func ankiPreview(lemma: String, pos: String, maxExamples: Int, maxDicts: Int?) async throws -> AnkiCard {
        try await performDetached { try $0.call("ankiPreview", ["lemma": AnyEncodable(lemma), "pos": AnyEncodable(pos), "maxExamples": AnyEncodable(maxExamples), "maxDicts": AnyEncodable(maxDicts)]) }
    }
    public func ankiEnsureNoteType() async throws -> Bool { try await performDetached { try $0.call("ankiEnsureNoteType") } }
    public func ankiExportStart(_ request: AnkiBatchRequest) async throws -> Int { try await performDetached { try $0.call("ankiExportStart", request.arguments) } }
    public func ankiUpdateStart(_ request: AnkiBatchRequest) async throws -> Int { try await performDetached { try $0.call("ankiUpdateStart", request.arguments) } }
    public func ankiRefreshPreview(deck: String, scope: AnkiRefreshScope) async throws -> Int { try await performDetached { try $0.call("ankiRefreshPreview", ["deck": AnyEncodable(deck), "scope": AnyEncodable(scope)]) } }
    public func ankiRefreshStart(deck: String, scope: AnkiRefreshScope) async throws { try await performDetached { try $0.run("ankiRefreshStart", ["deck": AnyEncodable(deck), "scope": AnyEncodable(scope)]) } }
    public func ankiCancel() async throws { try await performDetached { try $0.run("ankiCancel") } }
    public func ankiIsRunning() async throws -> Bool { try await performDetached { try $0.call("ankiIsRunning") } }
    public func ankiProgress() async throws -> JobProgress<AnkiJobResult> { try await performDetached { try $0.call("ankiProgress") } }
    public func ankiBrowseNotes(_ noteIds: [Int64]) async throws { try await performDetached { try $0.run("ankiBrowseNotes", ["noteIds": AnyEncodable(noteIds)]) } }
    public func ankiModelNames() async throws -> [String] { try await performDetached { try $0.call("ankiModelNames") } }
    public func ankiMiningReport(now: String, deckContains: String? = nil) async throws -> MiningReportResult { try await performDetached { try $0.call("ankiMiningReport", ["now": AnyEncodable(now), "deckContains": AnyEncodable(deckContains)]) } }
    public func exportText(path: String, content: String) async throws { try await performDetached { try $0.run("exportText", ["path": AnyEncodable(path), "content": AnyEncodable(content)]) } }
    public func exportPng(path: String, bytes: [UInt8]) async throws { try await performDetached { try $0.run("exportPng", ["path": AnyEncodable(path), "bytes": AnyEncodable(bytes)]) } }
}
