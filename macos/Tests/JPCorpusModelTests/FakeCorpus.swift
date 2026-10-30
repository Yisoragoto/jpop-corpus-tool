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
    /// 先找「方法名:参数」的专门回答（比如某一首的音频找不到），没有再用这个方法的通用回答
    func lyricValue<T: Decodable>(_ method: String, arguments: [String] = []) throws -> T {
        calls.append(Call(name: method, arguments: arguments))
        let specific = lyricResponses["\(method):\(arguments.joined(separator: ","))"]
        guard let json = specific ?? lyricResponses[method] else { throw FakeError.unexpected(method) }
        return try decoded(T.self, json)
    }
    func failScanning() { failScan = true }

    /// 让某个调用（「方法名:参数」）卡住，直到 `release`：用来控制两个并发请求谁先完成
    private var held: Set<String> = []
    private var parked: [String: [CheckedContinuation<Void, Never>]] = [:]
    func hold(_ key: String) { held.insert(key) }
    func release(_ key: String) {
        held.remove(key)
        for waiter in parked.removeValue(forKey: key) ?? [] { waiter.resume() }
    }
    func isParked(_ key: String) -> Bool { !(parked[key]?.isEmpty ?? true) }
    private func gate(_ key: String) async {
        guard held.contains(key) else { return }
        await withCheckedContinuation { parked[key, default: []].append($0) }
    }
    /// 假装有一次导入正在跑
    private var importRunning = false
    func setImportRunning(_ running: Bool) { importRunning = running }

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
    func importProgress() async throws -> JobProgress<ImportResult> {
        guard importRunning else { throw FakeError.unexpected("importProgress") }
        return try decoded(JobProgress<ImportResult>.self, #"{"running":true,"finished":false,"cancelled":false,"done":1,"total":3,"stage":"x","result":null,"error":null}"#)
    }
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
        await gate("listenWindow:\(utteranceId),\(before),\(after)")
        return try lyricValue("listenWindow", arguments: [String(utteranceId), String(before), String(after)])
    }
    func resolveMedia(_ stored: String) async throws -> String? {
        await gate("resolveMedia:\(stored)")
        return try lyricValue("resolveMedia", arguments: [stored])
    }
    private(set) var recordedPlays: [PlayEvent] = []
    private var recordPlayFails = false
    func failRecordPlay(_ fail: Bool) { recordPlayFails = fail }
    func recordPlay(_ event: PlayEvent) async throws {
        if recordPlayFails { throw FakeError.injected }
        recordedPlays.append(event)
    }
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

extension FakeCorpus: LibraryCalling {
    func libraryEditSong(_ songId: String, edit: SongEdit) async throws -> EditReport {
        return try lyricValue("libraryEditSong", arguments: [String(decoding: try JSONEncoder().encode(songId), as: UTF8.self), String(decoding: try JSONEncoder().encode(edit), as: UTF8.self)])
    }
    func libraryDeleteSong(_ songId: String) async throws -> DeleteResult {
        return try lyricValue("libraryDeleteSong", arguments: [String(decoding: try JSONEncoder().encode(songId), as: UTF8.self)])
    }
    func libraryMissingAudio() async throws -> [MissingAudio] {
        return try lyricValue("libraryMissingAudio", arguments: [])
    }
    func librarySuggestRelinks(_ folder: String) async throws -> [RelinkSuggestion] {
        return try lyricValue("librarySuggestRelinks", arguments: [String(decoding: try JSONEncoder().encode(folder), as: UTF8.self)])
    }
    func libraryRelinkAudio(_ links: [RelinkRequest]) async throws -> RelinkBatch {
        return try lyricValue("libraryRelinkAudio", arguments: [String(decoding: try JSONEncoder().encode(links), as: UTF8.self)])
    }
    func libraryNewAudio(ignoredFolders: [String], ignoredFiles: [String]) async throws -> NewAudio {
        return try lyricValue("libraryNewAudio", arguments: [String(decoding: try JSONEncoder().encode(ignoredFolders), as: UTF8.self), String(decoding: try JSONEncoder().encode(ignoredFiles), as: UTF8.self)])
    }
    func artistImageSet(_ name: String, path: String) async throws -> String {
        return try lyricValue("artistImageSet", arguments: [String(decoding: try JSONEncoder().encode(name), as: UTF8.self), String(decoding: try JSONEncoder().encode(path), as: UTF8.self)])
    }
    func artistImageReset(_ name: String) async throws -> String {
        return try lyricValue("artistImageReset", arguments: [String(decoding: try JSONEncoder().encode(name), as: UTF8.self)])
    }
    func artistRoster() async throws -> [ArtistRow] {
        return try lyricValue("artistRoster", arguments: [])
    }
    func memberships() async throws -> [Membership] {
        return try lyricValue("memberships", arguments: [])
    }
    func bandMembers(_ personId: Int64) async throws -> [Membership] {
        return try lyricValue("bandMembers", arguments: [String(decoding: try JSONEncoder().encode(personId), as: UTF8.self)])
    }
    func bandsOf(_ personId: Int64) async throws -> [Membership] {
        return try lyricValue("bandsOf", arguments: [String(decoding: try JSONEncoder().encode(personId), as: UTF8.self)])
    }
    func membershipSuggestions() async throws -> [MembershipSuggestion] {
        return try lyricValue("membershipSuggestions", arguments: [])
    }
    func membershipPut(_ input: MembershipInput) async throws -> MembershipPutOutcome {
        return try lyricValue("membershipPut", arguments: [String(decoding: try JSONEncoder().encode(input.bandId), as: UTF8.self), String(decoding: try JSONEncoder().encode(input.memberId), as: UTF8.self), String(decoding: try JSONEncoder().encode(input.memberName), as: UTF8.self), String(decoding: try JSONEncoder().encode(input.part), as: UTF8.self), String(decoding: try JSONEncoder().encode(input.joined), as: UTF8.self), String(decoding: try JSONEncoder().encode(input.leftAt), as: UTF8.self)])
    }
    func membershipConfirm(bandId: Int64, memberId: Int64) async throws -> MembershipPutOutcome {
        return try lyricValue("membershipConfirm", arguments: [String(decoding: try JSONEncoder().encode(bandId), as: UTF8.self), String(decoding: try JSONEncoder().encode(memberId), as: UTF8.self)])
    }
    func membershipReject(bandId: Int64, memberId: Int64) async throws {
        let _: Bool = try lyricValue("membershipReject", arguments: [String(decoding: try JSONEncoder().encode(bandId), as: UTF8.self), String(decoding: try JSONEncoder().encode(memberId), as: UTF8.self)])
    }
    func tokenCorrection(_ utteranceId: Int64) async throws -> CorrectionView? {
        return try lyricValue("tokenCorrection", arguments: [String(decoding: try JSONEncoder().encode(utteranceId), as: UTF8.self)])
    }
    func saveTokenCorrection(_ utteranceId: Int64, tokens: [TokenEdit]) async throws {
        let _: Bool = try lyricValue("saveTokenCorrection", arguments: [String(decoding: try JSONEncoder().encode(utteranceId), as: UTF8.self), String(decoding: try JSONEncoder().encode(tokens), as: UTF8.self)])
    }
    func revertTokenCorrection(_ utteranceId: Int64) async throws -> Bool {
        return try lyricValue("revertTokenCorrection", arguments: [String(decoding: try JSONEncoder().encode(utteranceId), as: UTF8.self)])
    }
    func peopleByRole(_ role: String, limit: Int) async throws -> [PersonSummary] {
        return try lyricValue("peopleByRole", arguments: [String(decoding: try JSONEncoder().encode(role), as: UTF8.self), String(decoding: try JSONEncoder().encode(limit), as: UTF8.self)])
    }
    func worksByPerson(_ personId: Int64) async throws -> [Track] {
        return try lyricValue("worksByPerson", arguments: [String(decoding: try JSONEncoder().encode(personId), as: UTF8.self)])
    }
    func mostPlayed(minListenedSec: Double, limit: Int) async throws -> [PlayedTrack] {
        return try lyricValue("mostPlayed", arguments: [String(decoding: try JSONEncoder().encode(minListenedSec), as: UTF8.self), String(decoding: try JSONEncoder().encode(limit), as: UTF8.self)])
    }
    func recentlyPlayed(limit: Int) async throws -> [PlayedTrack] {
        return try lyricValue("recentlyPlayed", arguments: [String(decoding: try JSONEncoder().encode(limit), as: UTF8.self)])
    }
    func migratePlan(_ path: String) async throws -> MigratePlan {
        return try lyricValue("migratePlan", arguments: [String(decoding: try JSONEncoder().encode(path), as: UTF8.self)])
    }
    func migrateRun(_ path: String, options: MigrateOptions) async throws {
        let _: Bool = try lyricValue("migrateRun", arguments: [String(decoding: try JSONEncoder().encode(path), as: UTF8.self), String(decoding: try JSONEncoder().encode(options), as: UTF8.self)])
    }
    func backfillDurations() async throws {
        let _: Bool = try lyricValue("backfillDurations", arguments: [])
    }
    func fillAlbumArtwork() async throws {
        let _: Bool = try lyricValue("fillAlbumArtwork", arguments: [])
    }
    func libraryProgress() async throws -> JobProgress<LibraryJobResult> {
        return try lyricValue("libraryProgress", arguments: [])
    }
}
