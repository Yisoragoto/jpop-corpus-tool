import Foundation

// 刮削（元数据、封面、歌手照片）和振假名。逻辑在 Rust 的 jp-scrape / jp-tokenizer，和 Windows 版同一份

// MARK: - 刮削

/// 各刮削状态各有几首
public struct ScrapeSummary: Codable, Hashable, Sendable {
    public var pending: Int64
    public var running: Int64
    public var success: Int64
    public var lowConfidence: Int64
    public var failed: Int64
    public var skipped: Int64
    public var total: Int64
}

public struct CountPair: Codable, Hashable, Sendable {
    public var done: Int64
    public var total: Int64

    public var fraction: Double { total > 0 ? Double(done) / Double(total) : 0 }
}

/// 库的完整度：识别、曲目封面、专辑封面、歌词、时长各齐了多少
public struct Completeness: Codable, Hashable, Sendable {
    public var identified: CountPair
    public var skipped: Int64
    public var trackCovers: CountPair
    public var albumCovers: CountPair
    public var lyrics: CountPair
    public var durations: CountPair
}

/// 批量刮削里收下的一首（或一位歌手）
public struct ScrapeItem: Codable, Hashable, Sendable {
    /// track / artist
    public var kind: String
    public var title: String
    /// success / low_confidence / failed / skipped
    public var status: String
    public var confidence: Double
    /// 采纳的候选（人看的一行）
    public var matched: String
    public var coverSaved: Bool
}

public struct ScrapeBatch: Codable, Hashable, Sendable {
    public var items: [ScrapeItem]
}

public struct ScrapeCandidate: Codable, Hashable, Sendable {
    public struct Breakdown: Codable, Hashable, Sendable {
        public var finalScore: Double

        private enum CodingKeys: String, CodingKey { case finalScore = "final_score" }
    }

    public var provider: String
    public var title: String
    public var artist: String
    public var album: String
    public var year: String
    public var genre: String
    public var durationSec: Double?
    public var artworkUrl: String
    public var thumbUrl: String
    public var breakdown: Breakdown?

    public var score: Double { breakdown?.finalScore ?? 0 }
}

/// 复核队列里的一条：本地写的是什么、候选是什么
public struct ReviewItem: Codable, Hashable, Identifiable, Sendable {
    public var filePath: String
    public var songId: String
    public var status: String
    public var confidence: Double
    public var localTitle: String
    public var localArtist: String
    public var localAlbum: String
    public var candidates: [ScrapeCandidate]
    public var explain: String
    public var errorMessage: String
    public var id: String { filePath }
}

/// 手动采纳一条候选的结果
public struct ManualApply: Codable, Hashable, Sendable {
    public var fieldsFilled: Int64
    public var creditsProtected: Int64
    public var creditsAdded: Int64
    public var coverPath: String?
}

/// 补封面的结果
public struct CoverBackfill: Codable, Hashable, Sendable {
    public var checked: Int64
    public var filled: Int64
    public var noCandidate: Int64
    public var failed: Int64
    public var samples: [String]
}

// MARK: - 振假名

public enum FuriganaMode: String, Codable, CaseIterable, Sendable {
    /// 只给汉字注（送假名不注）
    case kanji
    /// 整个词注一个读音
    case word
}

/// 一段注音在原文里的位置，按字符（Unicode 码点）数，左闭右开
public struct Ruby: Codable, Hashable, Sendable {
    public var start: Int
    public var end: Int
    public var reading: String
}

public struct LineFurigana: Codable, Hashable, Sendable {
    public var utteranceId: Int64
    public var rubies: [Ruby]
}

extension CorpusClient {
    public func scrapeSummary() async throws -> ScrapeSummary {
        try await perform { try $0.call("scrapeSummary") }
    }

    public func completeness() async throws -> Completeness {
        try await perform { try $0.call("completeness") }
    }

    /// 批量刮曲目（后台作业）。返回要刮几首
    @discardableResult
    public func scrapeStart(onlyMissing: Bool = true) async throws -> Int {
        try await performDetached { try $0.call("scrapeStart", ["onlyMissing": AnyEncodable(onlyMissing)]) }
    }

    /// 一键刮削：识别曲目 → 歌手照片 → 补封面 → 补专辑信息，一个作业跑完。进度的 stage 见 ``ScrapeStage``
    public func scrapeAllStart() async throws {
        let _: LyricJSON = try await performDetached { try $0.call("scrapeAllStart") }
    }

    /// 批量刮歌手（资料和照片）。和曲目共用一个作业
    @discardableResult
    public func scrapeArtistsStart(onlyMissing: Bool = true) async throws -> Int {
        try await performDetached { try $0.call("scrapeArtistsStart", ["onlyMissing": AnyEncodable(onlyMissing)]) }
    }

    public func scrapeProgress() async throws -> JobProgress<ScrapeBatch> {
        try await performDetached { try $0.call("scrapeProgress") }
    }

    public func scrapeCancel() async throws {
        try await performDetached { try $0.run("scrapeCancel") }
    }

    /// 刮一首（一两秒，要联网）
    public func scrapeTrack(_ songId: String) async throws -> ScrapeSummary {
        try await performDetached { try $0.call("scrapeTrack", ["songId": AnyEncodable(songId)]) }
    }

    /// 需要人工确认的（默认 low_confidence），也可以点名要别的状态
    public func scrapeReviewQueue(statuses: [String]? = nil) async throws -> [ReviewItem] {
        try await perform { try $0.call("scrapeReviewQueue", ["statuses": AnyEncodable(statuses)]) }
    }

    public func scrapeAccept(filePath: String, candidateIndex: Int) async throws -> ManualApply {
        try await performDetached {
            try $0.call("scrapeAccept", ["filePath": AnyEncodable(filePath), "candidateIndex": AnyEncodable(candidateIndex)])
        }
    }

    public func scrapeSkip(filePath: String) async throws {
        try await perform { try $0.run("scrapeSkip", ["filePath": AnyEncodable(filePath)]) }
    }

    /// 把失败的打回待刮。返回打回了几首
    public func scrapeRetryFailed() async throws -> Int {
        try await perform { try $0.call("scrapeRetryFailed") }
    }

    /// 给识别成功但没有封面的歌补封面（先用音频里内嵌的，再用刮削时存下的候选），再填专辑封面
    public func scrapeBackfillCovers() async throws -> CoverBackfill {
        try await performDetached { try $0.call("scrapeBackfillCovers") }
    }

    /// 整首歌的振假名。要分词词典（没有时报错，界面提示去导入页下载）
    public func lyricsFurigana(_ songId: String, mode: FuriganaMode = .kanji) async throws -> [LineFurigana] {
        try await perform { try $0.call("lyricsFurigana", ["songId": AnyEncodable(songId), "mode": AnyEncodable(mode)]) }
    }
}
