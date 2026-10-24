import Foundation

public struct EditedLyricLine: Codable, Hashable, Sendable {
    public var timeSec: Double?
    public var text: String
    public init(timeSec: Double?, text: String) { self.timeSec = timeSec; self.text = text }
}

public struct TimedEditUnit: Codable, Hashable, Sendable {
    public var charStart: Int
    public var charEnd: Int
    public var startSec: Double
    public var endSec: Double?
    public init(charStart: Int, charEnd: Int, startSec: Double, endSec: Double?) {
        self.charStart = charStart; self.charEnd = charEnd
        self.startSec = startSec; self.endSec = endSec
    }
}

public struct TimedEditLine: Codable, Hashable, Sendable {
    public var timeSec: Double?
    public var text: String
    public var units: [TimedEditUnit]
    public init(timeSec: Double?, text: String, units: [TimedEditUnit]) {
        self.timeSec = timeSec; self.text = text; self.units = units
    }
}

public struct WordTiming: Codable, Hashable, Sendable {
    public var utteranceId: Int64
    public var charStart: Int
    public var charEnd: Int
    public var startSec: Double
    public var endSec: Double?
    public var source: String
}

public struct LyricsAttached: Codable, Hashable, Sendable {
    public var songId: String
    public var source: String
    public var matched: String
    public var lyricLines: Int
    public var tokens: Int
    public var credits: Int
    public var correctionsRestored: Int
    public var lrcPath: String
}

public struct MissingLyrics: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var durationSec: Double?
    public var audioPath: String
    public var siblingLrc: String?
    public var id: String { songId }
}

public struct LyricsSources: Codable, Hashable, Sendable {
    public var netease = true
    public var qq = true
    public var kugou = true
    public var amll = true
    public var lrclib = true
    public var apple = false
    public var appleStorefront = "jp"
    public var appleDeveloperTokenSet = false
    public var appleMusicUserTokenSet = false
    public init() {}
}

public struct TimedLyricsTarget: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var lyricLines: Int
    public var timedLines: Int
    public var id: String { songId }
}

public struct TimedLyricsCandidate: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var source: String
    public var format: String
    public var title: String
    public var artist: String
    public var lineCount: Int
    public var timedLineCount: Int
    public var mode: String
    public var preview: [String]
    public var matchNote: String
}

public struct LyricsSourceWarning: Codable, Hashable, Sendable {
    public var source: String
    public var message: String
}

public struct TimedLyricsSearch: Codable, Hashable, Sendable {
    public var songId: String
    public var title: String
    public var currentLines: Int
    public var currentTimedLines: Int
    public var candidates: [TimedLyricsCandidate]
    public var warnings: [LyricsSourceWarning]
}

public struct LyricsUpgradeItem: Codable, Hashable, Sendable {
    public var songId: String
    public var title: String
    public var status: String
    public var source: String
    public var message: String
}

public struct LyricsUpgradeProgress: Codable, Hashable, Sendable {
    public var running: Bool
    public var cancelled: Bool
    public var total: Int
    public var done: Int
    public var upgraded: Int
    public var lineFilled: Int
    public var skipped: Int
    public var notFound: Int
    public var failed: Int
    public var title: String
    public var error: String
    public var results: [LyricsUpgradeItem]
}

extension CorpusClient {
    public func lyricsSaveLines(_ songId: String, lines: [EditedLyricLine], note: String) async throws -> LyricsAttached {
        try await performDetached { try $0.call("lyricsSaveLines", ["songId": AnyEncodable(songId), "lines": AnyEncodable(lines), "note": AnyEncodable(note)]) }
    }
    public func lyricsSaveTiming(_ songId: String, lines: [TimedEditLine], note: String) async throws -> LyricsAttached {
        try await performDetached { try $0.call("lyricsSaveTiming", ["songId": AnyEncodable(songId), "lines": AnyEncodable(lines), "note": AnyEncodable(note)]) }
    }
    public func lyricsRestoreVersion(_ songId: String, versionId: Int64) async throws -> LyricsAttached {
        try await performDetached { try $0.call("lyricsRestoreVersion", ["songId": AnyEncodable(songId), "versionId": AnyEncodable(versionId)]) }
    }
    public func lyricVersionRaw(_ versionId: Int64) async throws -> String? {
        try await perform { try $0.call("lyricVersionRaw", ["versionId": AnyEncodable(versionId)]) }
    }
    public func lyricsImportFile(_ songId: String, path: String) async throws -> LyricsAttached {
        try await performDetached { try $0.call("lyricsImportFile", ["songId": AnyEncodable(songId), "path": AnyEncodable(path)]) }
    }
    public func lyricsRevalidate(_ songId: String?) async throws -> Int {
        try await performDetached { try $0.call("lyricsRevalidate", ["songId": AnyEncodable(songId)]) }
    }
    public func lyricsSourcesGet() async throws -> LyricsSources {
        try await performDetached { try $0.call("lyricsSourcesGet") }
    }
    public func lyricsSourcesSet(_ config: LyricsSources) async throws -> LyricsSources {
        try await performDetached { try $0.call("lyricsSourcesSet", ["config": AnyEncodable(config)]) }
    }
    public func lyricsMissing() async throws -> [MissingLyrics] {
        try await performDetached { try $0.call("lyricsMissing") }
    }
    public func lyricsFillOne(_ songId: String) async throws -> LyricsAttached? {
        try await performDetached { try $0.call("lyricsFillOne", ["songId": AnyEncodable(songId)]) }
    }
    public func lyricsFillStart(songIds: [String]?) async throws {
        try await performDetached { try $0.run("lyricsFillStart", ["songIds": AnyEncodable(songIds)]) }
    }
    public func lyricsFillRunning() async throws -> Bool {
        try await performDetached { try $0.call("lyricsFillRunning") }
    }
    public func lyricsFillProgress() async throws -> JobProgress<LyricsUpgradeProgress> {
        try await performDetached { try $0.call("lyricsFillProgress") }
    }
    public func lyricsFillCancel() async throws {
        try await performDetached { try $0.run("lyricsFillCancel") }
    }
    public func lyricsTimedTargets() async throws -> [TimedLyricsTarget] {
        try await performDetached { try $0.call("lyricsTimedTargets") }
    }
    public func lyricsTimedSearch(_ songId: String) async throws -> TimedLyricsSearch {
        try await performDetached { try $0.call("lyricsTimedSearch", ["songId": AnyEncodable(songId)]) }
    }
    public func lyricsTimedApply(_ songId: String, candidateId: String) async throws -> LyricsAttached {
        try await performDetached { try $0.call("lyricsTimedApply", ["songId": AnyEncodable(songId), "candidateId": AnyEncodable(candidateId)]) }
    }
    public func lyricsUpgradeStart() async throws {
        try await performDetached { try $0.run("lyricsUpgradeStart") }
    }
    public func lyricsUpgradeCancel() async throws {
        try await performDetached { try $0.run("lyricsUpgradeCancel") }
    }
    public func lyricsUpgradeStatus() async throws -> LyricsUpgradeProgress {
        try await performDetached { try $0.call("lyricsUpgradeStatus") }
    }
    public func lyricsUpgradeProgress() async throws -> JobProgress<LyricsUpgradeProgress> {
        try await performDetached { try $0.call("lyricsUpgradeProgress") }
    }
    public func wordTimings(_ utteranceIds: [Int64]) async throws -> [WordTiming] {
        try await performDetached { try $0.call("wordTimings", ["utteranceIds": AnyEncodable(utteranceIds)]) }
    }
    public func wordTimeSet(utteranceId: Int64, charStart: Int, charEnd: Int, startSec: Double) async throws {
        try await performDetached { try $0.run("wordTimeSet", ["utteranceId": AnyEncodable(utteranceId), "charStart": AnyEncodable(charStart), "charEnd": AnyEncodable(charEnd), "startSec": AnyEncodable(startSec)]) }
    }
    public func wordTimeClear(utteranceId: Int64, charStart: Int) async throws {
        try await performDetached { try $0.run("wordTimeClear", ["utteranceId": AnyEncodable(utteranceId), "charStart": AnyEncodable(charStart)]) }
    }
}
