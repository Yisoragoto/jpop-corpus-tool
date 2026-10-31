import Foundation

// 导入、分词词典、查词的数据模型。和 Models.swift 一样：字段名就是 Rust serde 的 camelCase，
// 也就是 Tauri 前端的同一套名字（jp-import 的 ScanResult / ImportReport，jp-setup 的 TokenizerStatus……）。

// MARK: - 扫描与计划

/// 计划里每个文件打算怎么处置（Rust `jp_import::Action`，按 `kind` 区分）
public struct PlanAction: Codable, Hashable, Sendable {
    public enum Kind: String, Codable, Sendable {
        /// 库里没有，会新建
        case new
        /// 这个文件已经在库里
        case alreadyImported
        /// 曲名 + 歌手对得上，但是另一个文件。不自动决定，不导
        case possibleDuplicate
        /// 这一批里出现了两次，只导第一个
        case duplicateInBatch
        /// 导不了
        case skipped
    }

    public var kind: Kind
    public var songId: String?
    public var existingPath: String?
    public var firstPath: String?
    /// 跳过的原因；可能重复时是判成同一个录音的依据
    public var reason: String?
    /// 新建：库里同名同歌手那首的另一个版本（Live、翻唱、重制、时长不同……）
    public var versionOf: VersionOf?
    /// 已在库中，但文件变了（导入时刷新文件信息）
    public var fileChanged: Bool?

    public struct VersionOf: Codable, Hashable, Sendable {
        public var songId: String
        public var existingPath: String
        public var reason: String
    }

    /// 会不会真的往库里写
    public var writes: Bool { kind == .new }
}

public struct PlanSummary: Codable, Hashable, Sendable {
    public var total: Int
    public var new: Int
    public var alreadyImported: Int
    public var possibleDuplicates: Int
    public var duplicatesInBatch: Int
    public var skipped: Int
    /// 已在库中、但文件变了的。老后端没有这一项
    public var filesChanged: Int?
    /// 有没有要做的事：新歌，或者变了的文件要刷新
    public var hasWork: Bool { new > 0 || (filesChanged ?? 0) > 0 }
}

public struct PlanItem: Codable, Hashable, Identifiable, Sendable {
    public var path: String
    public var fileName: String
    public var title: String
    public var artist: String
    public var album: String
    public var durationSec: Double?
    public var hasLyrics: Bool
    /// 读 tag 时出的问题；nil 是一切正常
    public var warning: String?
    /// `tag` / `fileName` / `folder` / `none`。从文件夹名来的歌手只是猜
    public var artistSource: String
    public var action: PlanAction
    public var id: String { path }
}

public struct ScanResult: Codable, Hashable, Sendable {
    public var summary: PlanSummary
    public var items: [PlanItem]
    /// 分词器可不可用。不可用时照样能导，只是没有分词——检索、分析要等补上分词
    public var tokenizerReady: Bool
    /// 选中但不是音频、没进计划的文件名
    public var ignored: [String]
}

// MARK: - 后台作业

/// 后台作业的进度（Rust `jp_ffi::jobs::JobView`）。导入、下载分词词典都是这个形状，`result` 各不相同
public struct JobProgress<Value: Decodable & Sendable>: Decodable, Sendable {
    public var running: Bool
    /// 跑完了（成功、失败、取消都算）
    public var finished: Bool
    public var cancelled: Bool
    /// 导入：第几首 / 一共几首。下载：字节
    public var done: Int64
    public var total: Int64
    /// 导入：刚导完的歌名。下载：`downloading` / `extracting` / `tokenizing`
    public var stage: String
    public var result: Value?
    public var error: String?

    public var fraction: Double? {
        total > 0 ? min(1, Double(done) / Double(total)) : nil
    }
}

public struct ImportedTrack: Codable, Hashable, Sendable {
    public struct Outcome: Codable, Hashable, Sendable {
        /// `imported` / `failed`
        public var kind: String
        public var lyricLines: Int?
        public var tokens: Int?
        public var error: String?
    }

    public var songId: String
    public var path: String
    public var title: String
    public var outcome: Outcome
}

/// 一次导入做了什么。取消时是取消前真做完的那些（一首歌一个事务）
public struct ImportResult: Codable, Hashable, Sendable {
    public var tracks: [ImportedTrack]
    public var cancelled: Bool
    public var imported: Int
    public var failed: Int
    public var lyricLines: Int
    public var tokens: Int
    public var csv: String?
    /// 已在库中、文件变了而刷新了文件信息的首数
    public var filesRefreshed: Int?
}

/// `runImport` 的立即返回：这一批会不会带分词
public struct ImportStarted: Codable, Hashable, Sendable {
    public var tokenize: Bool
}

// MARK: - 分词词典

public struct TokenizerStatus: Codable, Hashable, Sendable {
    public var ready: Bool
    public var dictPath: String?
    public var dictBytes: Int64
    /// `library`（语料库目录里的 sudachi/）、`venv`、`app`、`none`
    public var source: String
    /// 下载或复制进来的那一份放在哪
    public var bundledDir: String
}

public struct Tokenized: Codable, Hashable, Sendable {
    public var songs: Int
    public var lines: Int
    public var tokens: Int
    public var correctionsRestored: Int
}

/// 装好词典的结果：装在哪 + 顺手补了多少首歌的分词
public struct DictionaryReady: Codable, Hashable, Sendable {
    public var bytes: Int64
    public var dir: String
    public var tokenized: Tokenized
    /// 补分词失败的原因；空串是没失败
    public var tokenizeError: String
}

// MARK: - 查词（Yomitan 词典）

public struct LookupHeadword: Codable, Hashable, Sendable {
    public var term: String
    public var reading: String
}

public struct LookupDefinition: Codable, Hashable, Sendable {
    public var dictionary: String
    /// 每一项是一条释义的纯文本（可能有换行）
    public var text: [String]
    public var entries: [DictionaryValue]?
}

public struct LookupEntry: Codable, Hashable, Sendable {
    public var headwords: [LookupHeadword]
    public var definitions: [LookupDefinition]
}

/// `dictLookupText`：从一段文字开头查到的词条，释义是纯文本
public struct LookupText: Codable, Hashable, Sendable {
    public var entries: [LookupEntry]
    /// 命中的原文有几个字
    public var originalTextLength: Int
}

public struct DictionaryInfo: Codable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var title: String
    public var enabled: Bool
    public var revision: String?
    public var author: String?
    public var frequencyMode: String?
    public var counts: DictionaryCounts?
    public var priority: Int64?

    /// 「12,345 条 · 频率 · 图片 30」
    public var summary: String {
        var parts: [String] = []
        if let terms = counts?.terms, terms > 0 { parts.append("\(terms.formatted()) 条") }
        if let meta = counts?.termMeta {
            if let n = meta["freq"], n > 0 { parts.append("频率 \(n.formatted())") }
            if let n = meta["pitch"], n > 0 { parts.append("音调 \(n.formatted())") }
        }
        if let media = counts?.media, media > 0 { parts.append("图片 \(media.formatted())") }
        if let revision, !revision.isEmpty { parts.append("版本 \(revision)") }
        return parts.joined(separator: " · ")
    }
}

/// 词典里各类东西的条数（导入时记下的）。缺的项就是没有
public struct DictionaryCounts: Codable, Hashable, Sendable {
    public var terms: Int64?
    public var media: Int64?
    public var termMeta: [String: Int64]?
}

/// 保留词典结构化释义的 JSON，不在 Swift 复制渲染业务。
public indirect enum DictionaryValue: Codable, Hashable, Sendable {
    case string(String), number(Double), bool(Bool), array([DictionaryValue]), object([String: DictionaryValue]), null
    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode(Double.self) { self = .number(v) }
        else if let v = try? c.decode([DictionaryValue].self) { self = .array(v) }
        else { self = .object(try c.decode([String: DictionaryValue].self)) }
    }
    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .string(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .bool(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        case .null: try c.encodeNil()
        }
    }
}
