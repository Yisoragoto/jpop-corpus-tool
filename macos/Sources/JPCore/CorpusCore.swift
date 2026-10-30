import CJPCore
import Foundation

/// 调 Rust 核心出的错。消息是 Rust 那边给的一句人话，直接给用户看。
public enum CoreError: LocalizedError, Equatable {
    case open(String)
    case call(method: String, message: String)
    case decode(method: String, message: String)

    public var errorDescription: String? {
        switch self {
        case .open(let message): return "打不开语料库：\(message)"
        case .call(_, let message): return message
        case .decode(let method, let message): return "\(method) 的返回值读不懂：\(message)"
        }
    }
}

/// 一个打开的语料库（Rust 的 `jp_ffi::Session`）。
///
/// 所有方法都是同步的，在调用线程上跑；Rust 侧内部加锁，所以可以从任意线程调。
/// 界面上用 ``CorpusClient``，它把调用放到后台队列、给出 async 接口。
public final class CorpusCore: @unchecked Sendable {
    private let handle: OpaquePointer
    public let databaseURL: URL

    /// 打开（必要时迁移）`corpus.db`。和 Windows 客户端启动时做的完全一样：
    /// 老库补齐结构，比本程序新的库直接拒绝，不会把它改坏。
    public init(databaseURL: URL) throws {
        var error: UnsafeMutablePointer<CChar>? = nil
        guard let handle = jp_open(databaseURL.path, &error) else {
            let message = error.map { String(cString: $0) } ?? "未知错误"
            if let error { jp_string_free(error) }
            throw CoreError.open(message)
        }
        self.handle = handle
        self.databaseURL = databaseURL
    }

    deinit {
        jp_close(handle)
    }

    /// 在 `directory` 下写一个四首歌的合成语料库，返回它的 `corpus.db`。测试和演示用。
    public static func writeFixture(in directory: URL) throws -> URL {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        guard let raw = jp_write_fixture(directory.path) else {
            throw CoreError.open("写不出示例语料库")
        }
        defer { jp_string_free(raw) }
        return URL(fileURLWithPath: String(cString: raw))
    }

    /// 调一个方法，返回 `ok` 那部分的原始 JSON。
    public func callJSON(_ method: String, argsJSON: String = "{}") throws -> Data {
        guard let raw = jp_call(handle, method, argsJSON) else {
            throw CoreError.call(method: method, message: "没有返回值")
        }
        defer { jp_string_free(raw) }
        let envelope = Data(String(cString: raw).utf8)
        guard let object = try JSONSerialization.jsonObject(with: envelope, options: [.fragmentsAllowed]) as? [String: Any] else {
            throw CoreError.decode(method: method, message: "信封不是对象")
        }
        if let message = object["error"] as? String {
            throw CoreError.call(method: method, message: message)
        }
        let ok = object["ok"] ?? NSNull()
        return try JSONSerialization.data(withJSONObject: ok, options: [.fragmentsAllowed])
    }

    /// 调一个方法：参数编码成 JSON，返回值解码成 `R`。
    public func call<R: Decodable>(_ method: String, _ args: [String: AnyEncodable] = [:], as type: R.Type = R.self) throws -> R {
        let argsData = try JSONEncoder().encode(args)
        let data = try callJSON(method, argsJSON: String(decoding: argsData, as: UTF8.self))
        do {
            return try JSONDecoder().decode(R.self, from: data)
        } catch {
            throw CoreError.decode(method: method, message: String(describing: error))
        }
    }

    /// 没有返回值的调用
    public func run(_ method: String, _ args: [String: AnyEncodable] = [:]) throws {
        let argsData = try JSONEncoder().encode(args)
        _ = try callJSON(method, argsJSON: String(decoding: argsData, as: UTF8.self))
    }

    public func lyricTimeline(_ songId: String) throws -> [LineTimeline] {
        try call("lyricTimeline", ["songId": AnyEncodable(songId)])
    }
}

/// 把不同类型的参数装进同一个字典
public struct AnyEncodable: Encodable, @unchecked Sendable {
    private let encodeValue: (Encoder) throws -> Void

    public init<T: Encodable>(_ value: T) {
        encodeValue = value.encode
    }

    public func encode(to encoder: Encoder) throws {
        try encodeValue(encoder)
    }
}

/// 界面用的异步外壳：所有调用排在一条后台串行队列上，不卡主线程。
///
/// 导入、分词词典、查词这几组方法 Rust 那边不拿语料库的锁，走另一条并发队列（``performDetached(_:)``）：
/// 207MB 的词典正在复制、或者几秒的目录扫描在跑的时候，检索和进度轮询不用排在它后面。
public final class CorpusClient: @unchecked Sendable {
    public let core: CorpusCore
    private let queue = DispatchQueue(label: "jpcorpus.core", qos: .userInitiated)
    private let jobs = DispatchQueue(label: "jpcorpus.jobs", qos: .userInitiated, attributes: .concurrent)

    public init(core: CorpusCore) {
        self.core = core
    }

    public func perform<R>(_ work: @escaping @Sendable (CorpusCore) throws -> R) async throws -> R {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { [core] in
                continuation.resume(with: Result { try work(core) })
            }
        }
    }

    /// 不排在串行队列上。只给 Rust 那边不拿语料库锁的方法用（见类型说明）
    public func performDetached<R>(_ work: @escaping @Sendable (CorpusCore) throws -> R) async throws -> R {
        try await withCheckedThrowingContinuation { continuation in
            jobs.async { [core] in
                continuation.resume(with: Result { try work(core) })
            }
        }
    }

    // ── 常用方法（名字和 Tauri 客户端的 api.ts 对齐）──

    public func tracks() async throws -> [Track] {
        try await perform { try $0.call("tracks") }
    }

    public func track(_ songId: String) async throws -> Track? {
        try await perform { try $0.call("track", ["songId": AnyEncodable(songId)]) }
    }

    public func lyrics(_ songId: String) async throws -> [LyricLine] {
        try await perform { try $0.call("lyrics", ["songId": AnyEncodable(songId)]) }
    }

    public func lyricTimeline(_ songId: String) async throws -> [LineTimeline] {
        try await perform { try $0.lyricTimeline(songId) }
    }

    public func lineSpans(_ songId: String) async throws -> [LineSpan] {
        try await perform { try $0.call("lineSpans", ["songId": AnyEncodable(songId)]) }
    }

    public func listenWindow(_ utteranceId: Int64, before: Int = 0, after: Int = 0) async throws -> ListenWindow? {
        try await perform {
            try $0.call("listenWindow", [
                "utteranceId": AnyEncodable(utteranceId),
                "before": AnyEncodable(before),
                "after": AnyEncodable(after),
            ])
        }
    }

    public func kwic(_ query: KwicQuery) async throws -> [KwicHit] {
        try await perform { try $0.call("kwic", ["query": AnyEncodable(query)]) }
    }

    public func searchLyrics(_ text: String, limit: Int = 500) async throws -> [LyricHit] {
        try await perform { try $0.call("searchLyrics", ["text": AnyEncodable(text), "limit": AnyEncodable(limit)]) }
    }

    public func frequency(_ query: FrequencyQuery) async throws -> FrequencyResult {
        try await perform { try $0.call("frequency", ["query": AnyEncodable(query)]) }
    }

    public func lexicalProfile(_ query: LexicalQuery) async throws -> LexicalResult {
        try await perform { try $0.call("lexicalProfile", ["query": AnyEncodable(query)]) }
    }

    public func collocations(_ query: CollocationQuery) async throws -> CollocationResult {
        try await perform { try $0.call("collocations", ["query": AnyEncodable(query)]) }
    }

    public func keyness(_ query: KeynessQuery) async throws -> KeynessResult {
        try await perform { try $0.call("keyness", ["query": AnyEncodable(query)]) }
    }

    public func songProfile(_ songId: String, limit: Int = 12) async throws -> SongProfile {
        try await perform { try $0.call("songProfile", ["songId": AnyEncodable(songId), "limit": AnyEncodable(limit)]) }
    }

    public func wordInCorpus(_ lemma: String, currentSongId: String?, scope: Subcorpus? = nil, limit: Int = 1000) async throws -> WordInCorpus {
        try await perform {
            try $0.call("wordInCorpus", ["lemma": AnyEncodable(lemma), "currentSongId": AnyEncodable(currentSongId), "limit": AnyEncodable(limit), "scope": AnyEncodable(scope)])
        }
    }

    public func performers() async throws -> [PersonSummary] {
        try await peopleByRole("performer", limit: 5000)
    }

    public func overview() async throws -> Overview {
        try await perform { try $0.call("overview") }
    }

    public func setAudioRoots(_ roots: [String]) async throws {
        try await perform { try $0.run("setAudioRoots", ["roots": AnyEncodable(roots)]) }
    }

    /// 库里存的音频路径在这台机器上对应哪个文件；找不到是 nil
    public func resolveMedia(_ stored: String) async throws -> String? {
        try await perform { try $0.call("resolveMedia", ["path": AnyEncodable(stored)]) }
    }

    public func recordPlay(_ event: PlayEvent) async throws {
        try await perform { try $0.run("recordPlay", ["event": AnyEncodable(event)]) }
    }

    // ── 导入：扫描（只读）→ 计划 → 后台写库，轮询进度 ──

    /// 扫一个文件夹，和现有曲库比对，返回计划。不写库；记下这一批等 ``runImport(tokenize:)``
    public func scanFolder(_ path: String) async throws -> ScanResult {
        try await performDetached { try $0.call("scanFolder", ["path": AnyEncodable(path)]) }
    }

    /// 同上，但是一批具体的文件（也可以夹着文件夹）
    public func scanFiles(_ paths: [String]) async throws -> ScanResult {
        try await performDetached { try $0.call("scanFiles", ["paths": AnyEncodable(paths)]) }
    }

    /// 开始导入扫描过的那一批，立刻返回；进度用 ``importProgress()`` 问
    @discardableResult
    public func runImport(tokenize: Bool = true) async throws -> ImportStarted {
        try await performDetached { try $0.call("runImport", ["tokenize": AnyEncodable(tokenize)]) }
    }

    public func importProgress() async throws -> JobProgress<ImportResult> {
        try await performDetached { try $0.call("importProgress") }
    }

    /// 停下导入。停在哪首，那首之前的都是真导进去了
    public func cancelImport() async throws {
        try await performDetached { try $0.run("cancelImport") }
    }

    // ── 分词词典（Sudachi）──

    public func tokenizerStatus() async throws -> TokenizerStatus {
        try await performDetached { try $0.call("tokenizerStatus") }
    }

    /// 开始下载词典（43MB，解开 207MB），立刻返回；进度用 ``tokenizerProgress()`` 问
    public func tokenizerDownload() async throws {
        try await performDetached { try $0.run("tokenizerDownload") }
    }

    public func tokenizerProgress() async throws -> JobProgress<DictionaryReady> {
        try await performDetached { try $0.call("tokenizerProgress") }
    }

    /// 把别处的一份词典（另一个语料库目录、0.1.x 的项目目录……）复制进来。同步做完才返回
    public func tokenizerInstall(from path: String) async throws -> DictionaryReady {
        try await performDetached { try $0.call("tokenizerInstall", ["path": AnyEncodable(path)]) }
    }

    /// 有歌词、没分词的歌（没词典时导进来的）
    public func untokenizedCount() async throws -> Int {
        struct Song: Decodable { var songId: String }
        let songs: [Song] = try await perform { try $0.call("tokenizeMissingList") }
        return songs.count
    }

    public func tokenizeMissing() async throws -> Tokenized {
        try await performDetached { try $0.call("tokenizeMissingRun") }
    }

    // ── 查词（语料库目录里的 Yomitan 词典库 dictionaries.db）──

    public func dictionaries() async throws -> [DictionaryInfo] {
        try await performDetached { try $0.call("dictList") }
    }

    /// 从 `text` 开头查词，释义是纯文本
    public func lookup(_ text: String) async throws -> LookupText {
        try await performDetached { try $0.call("dictLookupText", ["text": AnyEncodable(text)]) }
    }
}
