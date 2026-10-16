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
public final class CorpusClient: @unchecked Sendable {
    public let core: CorpusCore
    private let queue = DispatchQueue(label: "jpcorpus.core", qos: .userInitiated)

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

    public func collocations(_ query: CollocationQuery) async throws -> CollocationResult {
        try await perform { try $0.call("collocations", ["query": AnyEncodable(query)]) }
    }

    public func keyness(_ query: KeynessQuery) async throws -> KeynessResult {
        try await perform { try $0.call("keyness", ["query": AnyEncodable(query)]) }
    }

    public func songProfile(_ songId: String, limit: Int = 12) async throws -> SongProfile {
        try await perform { try $0.call("songProfile", ["songId": AnyEncodable(songId), "limit": AnyEncodable(limit)]) }
    }

    public func wordInCorpus(_ lemma: String, currentSongId: String?) async throws -> WordInCorpus {
        try await perform {
            try $0.call("wordInCorpus", ["lemma": AnyEncodable(lemma), "currentSongId": AnyEncodable(currentSongId), "limit": AnyEncodable(24)])
        }
    }

    public func performers() async throws -> [PersonSummary] {
        try await perform { try $0.call("performers") }
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
}
