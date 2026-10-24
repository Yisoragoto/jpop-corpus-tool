import Foundation
import JPCore

/// 歌词模型可替换的异步边界，测试不连接网络。
public protocol LyricsCalling: AnyObject, Sendable {
    func songSetLanguage(_ songId: String, language: String) async throws
    func audioPeaks(_ songId: String, perSec: UInt32) async throws -> [UInt8]
    func lyrics(_ songId: String) async throws -> [LyricLine]
    func songLineage(_ songId: String) async throws -> SongLineage
    func lyricVersionRaw(_ versionId: Int64) async throws -> String?
    func wordTimings(_ utteranceIds: [Int64]) async throws -> [WordTiming]
    func lyricsSaveLines(_ songId: String, lines: [EditedLyricLine], note: String) async throws -> LyricsAttached
    func lyricsSaveTiming(_ songId: String, lines: [TimedEditLine], note: String) async throws -> LyricsAttached
    func lyricsRestoreVersion(_ songId: String, versionId: Int64) async throws -> LyricsAttached
    func lyricsImportFile(_ songId: String, path: String) async throws -> LyricsAttached
    func lyricsRevalidate(_ songId: String?) async throws -> Int
    func lyricsSourcesGet() async throws -> LyricsSources
    func lyricsSourcesSet(_ config: LyricsSourcesInput) async throws -> LyricsSources
    func lyricsMissing() async throws -> [MissingLyrics]
    func lyricsFillOne(_ songId: String) async throws -> LyricsAttached?
    func lyricsFillStart(songIds: [String]?) async throws
    func lyricsFillProgress() async throws -> JobProgress<LyricsUpgradeProgress>
    func lyricsFillCancel() async throws
    func lyricsUpgradeStart() async throws
    func lyricsTimedSearch(_ songId: String) async throws -> TimedLyricsSearch
    func lyricsTimedApply(_ songId: String, candidateId: String) async throws -> LyricsAttached
    func wordTimeSet(utteranceId: Int64, charStart: Int, charEnd: Int, startSec: Double) async throws
    func wordTimeClear(utteranceId: Int64, charStart: Int) async throws
}

extension CorpusClient: LyricsCalling {}
