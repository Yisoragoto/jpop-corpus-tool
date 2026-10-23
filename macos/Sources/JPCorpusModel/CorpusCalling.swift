import Foundation
import JPCore

/// Only the async operations used by the native client; no raw FFI or synchronous calls.
public protocol CorpusCalling: AnyObject, Sendable {
    func scanFiles(_ paths: [String]) async throws -> ScanResult
    func scanFolder(_ path: String) async throws -> ScanResult
    func runImport(tokenize: Bool) async throws -> ImportStarted
    func importProgress() async throws -> JobProgress<ImportResult>
    func cancelImport() async throws
    func tokenizerStatus() async throws -> TokenizerStatus
    func untokenizedCount() async throws -> Int
    func tokenizerDownload() async throws
    func tokenizerProgress() async throws -> JobProgress<DictionaryReady>
    func tokenizerInstall(from path: String) async throws -> DictionaryReady
    func tokenizeMissing() async throws -> Tokenized
    func tracks() async throws -> [Track]
    func track(_ songId: String) async throws -> Track?
    func setAudioRoots(_ roots: [String]) async throws
    func lyrics(_ songId: String) async throws -> [LyricLine]
    func lyricTimeline(_ songId: String) async throws -> [LineTimeline]
    func lineSpans(_ songId: String) async throws -> [LineSpan]
    func listenWindow(_ utteranceId: Int64, before: Int, after: Int) async throws -> ListenWindow?
    func resolveMedia(_ stored: String) async throws -> String?
    func recordPlay(_ event: PlayEvent) async throws
    func kwic(_ query: KwicQuery) async throws -> [KwicHit]
    func frequency(_ query: FrequencyQuery) async throws -> FrequencyResult
    func collocations(_ query: CollocationQuery) async throws -> CollocationResult
    func keyness(_ query: KeynessQuery) async throws -> KeynessResult
    func songProfile(_ songId: String, limit: Int) async throws -> SongProfile
    func performers() async throws -> [PersonSummary]
    func overview() async throws -> Overview
    func dictionaries() async throws -> [DictionaryInfo]
    func lookup(_ text: String) async throws -> LookupText
}

extension CorpusClient: CorpusCalling {}

public extension CorpusCalling {
    /// A client without timing support safely falls back to line playback.
    func lyricTimeline(_ songId: String) async throws -> [LineTimeline] { [] }

    func listenWindow(_ utteranceId: Int64) async throws -> ListenWindow? {
        try await listenWindow(utteranceId, before: 0, after: 0)
    }

    func songProfile(_ songId: String) async throws -> SongProfile {
        try await songProfile(songId, limit: 12)
    }
}
