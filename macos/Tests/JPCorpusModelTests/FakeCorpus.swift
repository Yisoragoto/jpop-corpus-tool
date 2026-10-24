import Foundation
import JPCore
@testable import JPCorpusModel

enum FakeError: LocalizedError {
    case injected
    case unexpected(String)
    var errorDescription: String? {
        switch self {
        case .injected: return "测试注入：没有读取权限"
        case .unexpected(let method): return "Unexpected call: \(method)"
        }
    }
}

func decoded<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
    try JSONDecoder().decode(type, from: Data(json.utf8))
}

/// An actor records the actual async boundary and makes unexpected operations fail loudly.
actor FakeCorpus: CorpusCalling {
    struct Call: Equatable { let name: String; let arguments: [String] }
    private(set) var calls: [Call] = []
    var failScan = false
    var lyricResponses: [String: String] = [:]
    func respondToLyrics(_ method: String, json: String) { lyricResponses[method] = json }
    func lyricValue<T: Decodable>(_ method: String, arguments: [String] = []) throws -> T {
        calls.append(Call(name: method, arguments: arguments))
        guard let json = lyricResponses[method] else { throw FakeError.unexpected(method) }
        return try decoded(T.self, json)
    }
    func failScanning() { failScan = true }

    func scanFiles(_ paths: [String]) async throws -> ScanResult {
        calls.append(Call(name: "scanFiles", arguments: paths))
        if failScan { throw FakeError.injected }
        return try decoded(ScanResult.self, """
        {"summary":{"total":\(paths.count),"new":\(paths.count),"alreadyImported":0,"possibleDuplicates":0,"duplicatesInBatch":0,"skipped":0},"items":[],"tokenizerReady":false,"ignored":[]}
        """)
    }
    func scanFolder(_ path: String) async throws -> ScanResult {
        calls.append(Call(name: "scanFolder", arguments: [path]))
        throw FakeError.unexpected("scanFolder")
    }
    func tracks() async throws -> [Track] { calls.append(Call(name: "tracks", arguments: [])); return [] }
    func setAudioRoots(_ roots: [String]) async throws { calls.append(Call(name: "setAudioRoots", arguments: roots)) }
    func tokenizerStatus() async throws -> TokenizerStatus {
        try decoded(TokenizerStatus.self, #"{"ready":false,"dictPath":null,"dictBytes":0,"source":"none","bundledDir":"/tmp/sudachi"}"#)
    }
    func untokenizedCount() async throws -> Int { 0 }
    func runImport(tokenize: Bool) async throws -> ImportStarted { throw FakeError.unexpected("runImport") }
    func importProgress() async throws -> JobProgress<ImportResult> { throw FakeError.unexpected("importProgress") }
    func cancelImport() async throws { throw FakeError.unexpected("cancelImport") }
    func tokenizerDownload() async throws { throw FakeError.unexpected("tokenizerDownload") }
    func tokenizerProgress() async throws -> JobProgress<DictionaryReady> { throw FakeError.unexpected("tokenizerProgress") }
    func tokenizerInstall(from path: String) async throws -> DictionaryReady {
        calls.append(Call(name: "tokenizerInstall", arguments: [path]))
        return try decoded(DictionaryReady.self, #"{"bytes":1,"dir":"/tmp/sudachi","tokenized":{"songs":0,"lines":0,"tokens":0,"correctionsRestored":0},"tokenizeError":""}"#)
    }
    func tokenizeMissing() async throws -> Tokenized { throw FakeError.unexpected("tokenizeMissing") }
    func track(_ songId: String) async throws -> Track? { try lyricValue("track", arguments: [songId]) }
    func lyrics(_ songId: String) async throws -> [LyricLine] { try lyricValue("lyrics", arguments: [songId]) }
    func lyricTimeline(_ songId: String) async throws -> [LineTimeline] {
        guard lyricResponses["lyricTimeline"] != nil else { return [] }
        return try lyricValue("lyricTimeline", arguments: [songId])
    }
    func lineSpans(_ songId: String) async throws -> [LineSpan] { try lyricValue("lineSpans", arguments: [songId]) }
    func listenWindow(_ utteranceId: Int64, before: Int, after: Int) async throws -> ListenWindow? {
        try lyricValue("listenWindow", arguments: [String(utteranceId), String(before), String(after)])
    }
    func resolveMedia(_ stored: String) async throws -> String? { try lyricValue("resolveMedia", arguments: [stored]) }
    private(set) var recordedPlays: [PlayEvent] = []
    func recordPlay(_ event: PlayEvent) async throws { recordedPlays.append(event) }
    func kwic(_ query: KwicQuery) async throws -> [KwicHit] { throw FakeError.unexpected("kwic") }
    func frequency(_ query: FrequencyQuery) async throws -> FrequencyResult { throw FakeError.unexpected("frequency") }
    func collocations(_ query: CollocationQuery) async throws -> CollocationResult { throw FakeError.unexpected("collocations") }
    func keyness(_ query: KeynessQuery) async throws -> KeynessResult { throw FakeError.unexpected("keyness") }
    func songProfile(_ songId: String, limit: Int) async throws -> SongProfile { throw FakeError.unexpected("songProfile") }
    func performers() async throws -> [PersonSummary] { throw FakeError.unexpected("performers") }
    func overview() async throws -> Overview { throw FakeError.unexpected("overview") }
    func dictionaries() async throws -> [DictionaryInfo] { throw FakeError.unexpected("dictionaries") }
    func lookup(_ text: String) async throws -> LookupText { throw FakeError.unexpected("lookup") }
}

extension FakeCorpus: LyricsCalling {
    func songSetLanguage(_ songId: String, language: String) async throws { calls.append(Call(name: "songSetLanguage", arguments: [songId, language])) }
    func audioPeaks(_ songId: String, perSec: UInt32) async throws -> [UInt8] { try lyricValue("audioPeaks", arguments: [songId, String(perSec)]) }
    func songLineage(_ songId: String) async throws -> SongLineage { try lyricValue("songLineage", arguments: [songId]) }
    func lyricVersionRaw(_ versionId: Int64) async throws -> String? { try lyricValue("lyricVersionRaw", arguments: [String(versionId)]) }
    func wordTimings(_ utteranceIds: [Int64]) async throws -> [WordTiming] { try lyricValue("wordTimings", arguments: utteranceIds.map { String($0) }) }
    func lyricsSaveLines(_ songId: String, lines: [EditedLyricLine], note: String) async throws -> LyricsAttached {
        let json = String(decoding: try JSONEncoder().encode(lines), as: UTF8.self)
        return try lyricValue("lyricsSaveLines", arguments: [songId, note, json])
    }
    func lyricsSaveTiming(_ songId: String, lines: [TimedEditLine], note: String) async throws -> LyricsAttached {
        let json = String(decoding: try JSONEncoder().encode(lines), as: UTF8.self)
        return try lyricValue("lyricsSaveTiming", arguments: [songId, note, json])
    }
    func lyricsRestoreVersion(_ songId: String, versionId: Int64) async throws -> LyricsAttached { try lyricValue("lyricsRestoreVersion", arguments: [songId, String(versionId)]) }
    func lyricsImportFile(_ songId: String, path: String) async throws -> LyricsAttached { try lyricValue("lyricsImportFile", arguments: [songId, path]) }
    func lyricsRevalidate(_ songId: String?) async throws -> Int { try lyricValue("lyricsRevalidate", arguments: [songId ?? "*"]) }
    func lyricsSourcesGet() async throws -> LyricsSources { try lyricValue("lyricsSourcesGet") }
    func lyricsSourcesSet(_ config: LyricsSourcesInput) async throws -> LyricsSources {
        let json = String(decoding: try JSONEncoder().encode(config), as: UTF8.self)
        return try lyricValue("lyricsSourcesSet", arguments: [json])
    }
    func lyricsFillOne(_ songId: String) async throws -> LyricsAttached? { try lyricValue("lyricsFillOne", arguments: [songId]) }
    func lyricsMissing() async throws -> [MissingLyrics] { try lyricValue("lyricsMissing") }
    func lyricsFillStart(songIds: [String]?) async throws { calls.append(Call(name: "lyricsFillStart", arguments: songIds ?? ["*"])) }
    func lyricsFillProgress() async throws -> JobProgress<LyricsUpgradeProgress> { try lyricValue("lyricsFillProgress") }
    func lyricsFillCancel() async throws { calls.append(Call(name: "lyricsFillCancel", arguments: [])) }
    func lyricsUpgradeStart() async throws { calls.append(Call(name: "lyricsUpgradeStart", arguments: [])) }
    func lyricsTimedSearch(_ songId: String) async throws -> TimedLyricsSearch { try lyricValue("lyricsTimedSearch", arguments: [songId]) }
    func lyricsTimedApply(_ songId: String, candidateId: String) async throws -> LyricsAttached { try lyricValue("lyricsTimedApply", arguments: [songId, candidateId]) }
    func wordTimeSet(utteranceId: Int64, charStart: Int, charEnd: Int, startSec: Double) async throws {
        calls.append(Call(name: "wordTimeSet", arguments: [String(utteranceId), String(charStart), String(charEnd), String(startSec)]))
    }
    func wordTimeClear(utteranceId: Int64, charStart: Int) async throws {
        calls.append(Call(name: "wordTimeClear", arguments: [String(utteranceId), String(charStart)]))
    }
}

actor OpenRecorder {
    private(set) var urls: [URL] = []
    let corpus = FakeCorpus()
    func open(_ url: URL) -> any CorpusCalling { urls.append(url); return corpus }
}
