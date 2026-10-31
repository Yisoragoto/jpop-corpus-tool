import Foundation

public struct LyricsFontFile: Decodable, Hashable, Sendable {
    public var family: String
    public var path: String
    public var bundled: Bool
}
public struct LyricsFontCatalog: Decodable, Sendable {
    public var files: [LyricsFontFile]
    public var problems: [String]
}

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
    public init(utteranceId: Int64, charStart: Int, charEnd: Int, startSec: Double, endSec: Double?, source: String) {
        self.utteranceId = utteranceId; self.charStart = charStart; self.charEnd = charEnd
        self.startSec = startSec; self.endSec = endSec; self.source = source
    }
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

/// SettingsInput：只编码可写字段；nil 保留旧凭据，空字符串清除。
public struct LyricsSourcesInput: Encodable, Sendable {
    public var netease: Bool
    public var qq: Bool
    public var kugou: Bool
    public var amll: Bool
    public var lrclib: Bool
    public var apple: Bool
    public var appleStorefront: String
    public var appleDeveloperToken: String?
    public var appleMusicUserToken: String?
    public init(_ view: LyricsSources, developerToken: String? = nil, musicUserToken: String? = nil) {
        netease = view.netease; qq = view.qq; kugou = view.kugou; amll = view.amll; lrclib = view.lrclib
        apple = view.apple; appleStorefront = view.appleStorefront.trimmingCharacters(in: .whitespacesAndNewlines)
        appleDeveloperToken = developerToken; appleMusicUserToken = musicUserToken
    }
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
    public var precisionLabel: String {
        if mode == "plain" { return "纯文本" }
        if mode == "lineSynced" { return "逐行" }
        if timedLineCount > 0 { return "含真实逐字时间" }
        if ["plain", "txt", "text", "plaintext", "plain_text"].contains(format.lowercased()) { return "纯文本" }
        return format.lowercased() == "lrc" ? "逐行" : "逐行 / 纯文本"
    }
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
    public func lyricsFontCatalog(imported: [String]) async throws -> LyricsFontCatalog {
        try await performDetached { try $0.call("fontsCatalog", ["imported": AnyEncodable(imported)]) }
    }
    public func lyricsFontImportArchive(_ path: String) async throws -> [String] {
        try await performDetached { try $0.call("fontsImportArchive", ["path": AnyEncodable(path)]) }
    }
    public func songSetLanguage(_ songId: String, language: String) async throws {
        try await performDetached { try $0.run("songSetLanguage", ["songId": AnyEncodable(songId), "language": AnyEncodable(language)]) }
    }
    public func audioPeaks(_ songId: String, perSec: UInt32) async throws -> [UInt8] {
        try await performDetached { try $0.call("audioPeaks", ["songId": AnyEncodable(songId), "perSec": AnyEncodable(perSec)]) }
    }
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
    public func lyricsSourcesSet(_ config: LyricsSourcesInput) async throws -> LyricsSources {
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

// MARK: - 歌词来源、整体偏移、统一时间轴（Rust：jp_corpus::lyric_sources / lyric_timeline）

/// 一首歌的一份歌词来源：你导入或编辑的、音频旁边的文件、内嵌、在线、升级前已有。
public struct LyricSourceRow: Codable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var songId: String
    /// user / sibling / embedded / online / legacy
    public var kind: String
    public var provider: String
    public var sourceRef: String
    /// original / translation
    public var role: String
    public var language: String
    public var sourceType: String
    public var rawFormat: String
    public var hasRaw: Bool
    public var contentHash: String
    public var lineCount: Int
    public var timedLineCount: Int
    /// none / line / word
    public var precision: String
    public var userModified: Bool
    public var missing: Bool
    public var offsetMs: Int
    public var updatedAt: String
    public var isOriginal: Bool { role == "original" }
    /// 不能拿来用的原因；能用时为 nil
    public var unusableReason: String? {
        if !hasRaw { return "当时没有保存原文" }
        if missing { return "文件不在了" }
        if lineCount == 0 { return "解析不出歌词行" }
        return nil
    }
}

public struct LyricSelection: Codable, Hashable, Sendable {
    public var sourceId: Int64?
    public var translationSourceId: Int64?
    public var translationOff: Bool
    /// user / auto / legacy
    public var chosenBy: String
    /// 已经加进歌词行时间（timeSec）里的偏移
    public var offsetMs: Int
    public var updatedAt: String
}

public struct LyricLookup: Codable, Hashable, Sendable {
    public var provider: String
    /// found / not_found / failed / ambiguous
    public var status: String
    public var message: String
    public var checkedAt: String
    public var retryAfter: String
}

public struct LyricSourcesView: Codable, Hashable, Sendable {
    public var songId: String
    public var sources: [LyricSourceRow]
    public var selection: LyricSelection?
    public var recommended: Int64?
    public var translationSourceId: Int64?
    public var lookups: [LyricLookup]
}

public struct LyricTimelineWord: Codable, Hashable, Sendable {
    public var charStart: Int
    public var charEnd: Int
    public var startMs: Int
    public var endMs: Int?
}

public struct LyricTimelineLine: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var utteranceId: Int64?
    public var startMs: Int?
    public var endMs: Int?
    public var text: String
    public var translation: String?
    public var words: [LyricTimelineWord]?
}

public struct LyricTranslationStatus: Codable, Hashable, Sendable {
    /// none / aligned / unaligned
    public var state: String
    public var language: String
    public var aligned: Int
    public var total: Int
    public var unalignedLines: [String]?
}

/// 统一时间轴。行里的时间是歌词时间；曲目时间 = 歌词时间 + offsetMs（见 `trackMs`）。
public struct SongLyricTimeline: Codable, Hashable, Sendable {
    public var songId: String
    public var sourceId: Int64?
    public var sourceKind: String
    public var rawFormat: String
    public var precision: String
    public var offsetMs: Int
    public var trackStartMs: Int
    public var durationMs: Int?
    public var parserVersion: Int
    public var lines: [LyricTimelineLine]
    public var translation: LyricTranslationStatus
    public var warnings: [String]?
    /// 歌词时间 → 曲目时间（播放器位置）。显示和跳转都走这一个换算
    public static func trackMs(_ lyricMs: Int, offsetMs: Int) -> Int { lyricMs + offsetMs }
    /// 曲目时间 → 歌词时间
    public static func lyricMs(_ trackMs: Int, offsetMs: Int) -> Int { trackMs - offsetMs }
}

/// 偏移上限（和 Rust 的 MAX_OFFSET_MS 一致）
public let lyricMaxOffsetMs = 30_000

extension CorpusClient {
    /// 先找一遍音频旁边的同名文件和内嵌歌词，再列出这首歌的全部来源
    public func lyricSourceView(_ songId: String) async throws -> LyricSourcesView {
        try await performDetached { try $0.call("lyricSourceView", ["songId": AnyEncodable(songId)]) }
    }
    public func lyricSourceRaw(_ sourceId: Int64) async throws -> String? {
        try await performDetached { try $0.call("lyricSourceRaw", ["sourceId": AnyEncodable(sourceId)]) }
    }
    public func lyricSourceTimeline(_ sourceId: Int64) async throws -> SongLyricTimeline {
        try await performDetached { try $0.call("lyricSourceTimeline", ["sourceId": AnyEncodable(sourceId)]) }
    }
    public func lyricSongTimeline(_ songId: String) async throws -> SongLyricTimeline {
        try await performDetached { try $0.call("lyricSongTimeline", ["songId": AnyEncodable(songId)]) }
    }
    /// 用某一份当这首歌的歌词（记成你的选择，自动流程之后不换）
    public func lyricSourceApply(_ songId: String, sourceId: Int64) async throws -> LyricsAttached {
        try await performDetached { try $0.call("lyricSourceApply", ["songId": AnyEncodable(songId), "sourceId": AnyEncodable(sourceId)]) }
    }
    /// 交还自动选择：按优先级重新挑；没换就返回 nil
    public func lyricSourceAuto(_ songId: String) async throws -> LyricsAttached? {
        try await performDetached { try $0.call("lyricSourceAuto", ["songId": AnyEncodable(songId)]) }
    }
    public func lyricTranslationSelect(_ songId: String, sourceId: Int64?) async throws {
        try await performDetached { try $0.run("lyricTranslationSelect", ["songId": AnyEncodable(songId), "sourceId": AnyEncodable(sourceId)]) }
    }
    /// 设整体偏移（毫秒，正值让歌词晚一点出现），返回实际生效的值
    public func lyricOffsetSet(_ songId: String, offsetMs: Int) async throws -> Int {
        try await performDetached { try $0.call("lyricOffsetSet", ["songId": AnyEncodable(songId), "offsetMs": AnyEncodable(offsetMs)]) }
    }
}
