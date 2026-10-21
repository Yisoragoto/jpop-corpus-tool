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
    func track(_ songId: String) async throws -> Track? { throw FakeError.unexpected("track") }
    func lyrics(_ songId: String) async throws -> [LyricLine] { throw FakeError.unexpected("lyrics") }
    func lineSpans(_ songId: String) async throws -> [LineSpan] { throw FakeError.unexpected("lineSpans") }
    func listenWindow(_ utteranceId: Int64, before: Int, after: Int) async throws -> ListenWindow? { throw FakeError.unexpected("listenWindow") }
    func resolveMedia(_ stored: String) async throws -> String? { throw FakeError.unexpected("resolveMedia") }
    func recordPlay(_ event: PlayEvent) async throws { throw FakeError.unexpected("recordPlay") }
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

actor OpenRecorder {
    private(set) var urls: [URL] = []
    let corpus = FakeCorpus()
    func open(_ url: URL) -> any CorpusCalling { urls.append(url); return corpus }
}
