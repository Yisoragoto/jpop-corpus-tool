import Foundation

// 首页、专辑、歌手、全库搜索、来源追踪、分析存档的数据模型。
// 和 Models.swift 一样以 Rust 的 serde 形状为准（camelCase）。

// MARK: - 首页

public struct PlayedTrack: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var album: String
    public var coverPath: String
    public var playCount: Int64
    public var lastPlayed: String
    public var totalListenedSec: Double
    public var durationSec: Double?
    public var lastPositionSec: Double
    public var lastCompleted: Bool
    public var id: String { songId }
}

/// 上次听的那首和停在哪
public struct ContinueTrack: Codable, Hashable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var coverPath: String
    public var durationSec: Double?
    public var positionSec: Double
    public var completed: Bool
    public var lastPlayed: String
    public var playCount: Int64
    /// 停下时唱到的那一行
    public var stoppedLine: String
}

public struct DayListening: Codable, Hashable, Sendable {
    public var listenedSec: Double
    public var songs: Int64
    public var linesHeard: Int64
    /// 连续几天有收听
    public var streakDays: Int64
}

/// 听过的歌覆盖了多少词
public struct Reach: Codable, Hashable, Sendable {
    public var heardSongs: Int64
    public var heardLemmas: Int64
    public var totalLemmas: Int64
}

public struct AddedTrack: Codable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var coverPath: String
    public var addedAt: String
    public var id: String { songId }
}

public struct HomeSummary: Codable, Hashable, Sendable {
    public var overview: Overview
    public var continueTrack: ContinueTrack?
    public var recentlyPlayed: [PlayedTrack]
    public var favorites: [Track]
    public var today: DayListening
    public var reach: Reach
    public var recentlyAdded: [AddedTrack]
}

// MARK: - 专辑、歌手

public struct Album: Codable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var title: String
    public var albumArtist: String
    public var year: String
    public var artworkPath: String
    public var trackCount: Int64
    public var totalDurationSec: Double
    /// Rust 已返回发行类型；旧快照缺这个字段时仍可解码。
    public var releaseType: String?
}

/// 人名录的一行：一个人在各个角色上各有几首
public struct DirectoryEntry: Codable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var name: String
    public var imagePath: String
    public var artistType: String
    public var ended: String
    public var imageManual: Bool
    public var performer: Int64
    public var composer: Int64
    public var lyricist: Int64
    public var arranger: Int64
    public var songs: Int64
    public var firstYear: String
    public var lastYear: String

    /// 「演唱 12 · 作曲 3」
    public var roleSummary: String {
        [("演唱", performer), ("作曲", composer), ("作词", lyricist), ("编曲", arranger)]
            .filter { $0.1 > 0 }
            .map { "\($0.0) \($0.1)" }
            .joined(separator: " · ")
    }
}

/// 一个人的作品：一首歌加上这个人在里面担任的角色（Rust 那边是 Track 平铺进来）
public struct PersonWork: Decodable, Hashable, Identifiable, Sendable {
    public var track: Track
    public var roles: [String]
    public var performerIds: [Int64]
    public var id: String { track.id }

    private enum Keys: String, CodingKey { case roles, performerIds }

    public init(from decoder: Decoder) throws {
        track = try Track(from: decoder)
        let c = try decoder.container(keyedBy: Keys.self)
        roles = try c.decode([String].self, forKey: .roles)
        performerIds = try c.decode([Int64].self, forKey: .performerIds)
    }
}

public struct Collaborator: Codable, Hashable, Identifiable, Sendable {
    public var personId: Int64
    public var name: String
    public var sharedTracks: Int64
    public var roles: [String]
    public var imagePath: String
    public var id: Int64 { personId }
}

public struct Credit: Codable, Hashable, Sendable {
    public var personId: Int64
    public var name: String
    public var role: String
    public var position: Int64
    public var source: String
}

/// 角色 → 中文名
public let roleLabels: [String: String] = [
    "performer": "演唱", "composer": "作曲", "lyricist": "作词", "arranger": "编曲",
]

// MARK: - 全库搜索（⌘K）

public enum QuickHit: Decodable, Hashable, Identifiable, Sendable {
    case track(songId: String, title: String, artist: String, album: String, durationSec: Double?)
    case album(albumId: Int64, title: String, albumArtist: String, trackCount: Int64)
    case person(personId: Int64, name: String, roles: [String], trackCount: Int64)
    case lyric(songId: String, title: String, artist: String, utteranceId: Int64, timeSec: Double?, text: String)
    case word(lemma: String, pos: String, freq: Int64, songCount: Int64)

    private enum Keys: String, CodingKey {
        case kind, songId, title, artist, album, durationSec, albumId, albumArtist, trackCount
        case personId, name, roles, utteranceId, timeSec, text, lemma, pos, freq, songCount
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "track":
            self = .track(songId: try c.decode(String.self, forKey: .songId), title: try c.decode(String.self, forKey: .title),
                          artist: try c.decode(String.self, forKey: .artist), album: try c.decode(String.self, forKey: .album),
                          durationSec: try c.decodeIfPresent(Double.self, forKey: .durationSec))
        case "album":
            self = .album(albumId: try c.decode(Int64.self, forKey: .albumId), title: try c.decode(String.self, forKey: .title),
                          albumArtist: try c.decode(String.self, forKey: .albumArtist), trackCount: try c.decode(Int64.self, forKey: .trackCount))
        case "person":
            self = .person(personId: try c.decode(Int64.self, forKey: .personId), name: try c.decode(String.self, forKey: .name),
                           roles: try c.decode([String].self, forKey: .roles), trackCount: try c.decode(Int64.self, forKey: .trackCount))
        case "lyric":
            self = .lyric(songId: try c.decode(String.self, forKey: .songId), title: try c.decode(String.self, forKey: .title),
                          artist: try c.decode(String.self, forKey: .artist), utteranceId: try c.decode(Int64.self, forKey: .utteranceId),
                          timeSec: try c.decodeIfPresent(Double.self, forKey: .timeSec), text: try c.decode(String.self, forKey: .text))
        case "word":
            self = .word(lemma: try c.decode(String.self, forKey: .lemma), pos: try c.decode(String.self, forKey: .pos),
                         freq: try c.decode(Int64.self, forKey: .freq), songCount: try c.decode(Int64.self, forKey: .songCount))
        case let other:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "不认识的搜索结果类型 \(other)")
        }
    }

    public var id: String {
        switch self {
        case .track(let id, _, _, _, _): return "t:\(id)"
        case .album(let id, _, _, _): return "a:\(id)"
        case .person(let id, _, _, _): return "p:\(id)"
        case .lyric(_, _, _, let u, _, _): return "l:\(u)"
        case .word(let lemma, let pos, _, _): return "w:\(lemma)|\(pos)"
        }
    }
}

public struct QuickSearchResults: Decodable, Hashable, Sendable {
    public var tracks: [QuickHit]
    public var albums: [QuickHit]
    public var people: [QuickHit]
    public var words: [QuickHit]
    public var lyrics: [QuickHit]

    public var isEmpty: Bool { tracks.isEmpty && albums.isEmpty && people.isEmpty && words.isEmpty && lyrics.isEmpty }
}

// MARK: - 来源追踪

/// Rust 的 `(String, i64)`：JSON 里是两元素数组
public struct LabelCount: Decodable, Hashable, Sendable {
    public var label: String
    public var count: Int64

    public init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        label = try c.decode(String.self)
        count = try c.decode(Int64.self)
    }
}

public struct LyricVersion: Decodable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var versionNo: Int64
    public var sourceType: String
    public var sourceName: String
    public var sourceRef: String
    public var importedAt: String
    public var lineCount: Int64
    public var timedLineCount: Int64
    public var completeness: Double?
    public var isCurrent: Bool
    public var note: String
    public var hasRawText: Bool
    public var validation: LyricsValidation?
}

public struct FieldProvenance: Decodable, Hashable, Sendable {
    public var field: String
    public var source: String
    public var sourceRef: String
    public var confidence: Double?
    public var updatedAt: String
}

public struct SongLineage: Decodable, Hashable, Sendable {
    public var songId: String
    public var language: String?
    public var versions: [LyricVersion]
    public var provenance: [FieldProvenance]
    public var lines: [LyricLineQuality]?
    public var tokenStatus: [String: Int]?
    public var tokenizers: [LyricTokenizerRun]?
    public var revisions: [LyricRevision]?
    /// 音频文件记录（现在用的排最前）。老后端没有这一项
    public var audioFiles: [AudioFileRow]?
}

/// 一个音频文件（Rust `jp_corpus::audio_files`）：文件本身的信息，和曲目元数据分开记
public struct AudioFileRow: Decodable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var songId: String
    public var path: String
    public var size: Int64?
    public var mtime: Double?
    public var durationSec: Double?
    public var format: String
    public var sampleRate: Int?
    public var channels: Int?
    public var bitDepth: Int?
    public var bitrateKbps: Int?
    public var fingerprint: String?
    /// 这首在文件里从哪儿开始（毫秒）；整轨 + CUE 时不为 0，现在恒为 0
    public var trackStartMs: Int64
    /// present / missing / replaced
    public var status: String
    public var firstSeen: String
    public var checkedAt: String

    /// 「FLAC · 44.1 kHz · 16 bit · 2 ch · 912 kbps · 24.3 MB」
    public var summary: String {
        var parts: [String] = []
        if !format.isEmpty { parts.append(format.uppercased()) }
        if let sampleRate { parts.append(String(format: "%g kHz", Double(sampleRate) / 1000)) }
        if let bitDepth { parts.append("\(bitDepth) bit") }
        if let channels { parts.append("\(channels) ch") }
        if let bitrateKbps { parts.append("\(bitrateKbps) kbps") }
        if let size { parts.append(String(format: "%.1f MB", Double(size) / 1_048_576)) }
        return parts.joined(separator: " · ")
    }
}

public struct SongQuality: Decodable, Hashable, Identifiable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var completeness: Double?
    public var sourceType: String
    public var warnings: Int64
    public var errors: Int64
    public var codes: [String]
    public var id: String { songId }
}

public struct QualityOverview: Decodable, Hashable, Sendable {
    public var songs: Int64
    public var songsWithLyrics: Int64
    public var songsWithVersion: Int64
    public var legacyVersions: Int64
    public var avgCompleteness: Double?
    public var bySource: [LabelCount]
    public var byLanguage: [LabelCount]
    public var issues: [LabelCount]
    public var tokenStatus: [LabelCount]
    public var linesLowTimeConfidence: Int64
    public var tokenizers: [LyricTokenizerRun]?
    public var worst: [SongQuality]
}

// MARK: - 分析存档

/// 统计用的筛选条件（Rust `StatsFilter`）。存档里记的范围就是它
public struct StatsFilter: Codable, Hashable, Sendable {
    public var performerIds: [Int64] = []
    public var jpOnly = false
    public var yearFrom: Int?
    public var yearTo: Int?
    public var genres: [String] = []
    public var languages: [String] = []
    public var albumIds: [Int64] = []
    public var songIds: [String] = []
    public var minCompleteness: Double?
    public var distinctLines = false

    public init() {}

    /// 一句话说明范围
    public var summary: String {
        var parts: [String] = []
        switch (yearFrom, yearTo) {
        case let (f?, t?): parts.append("\(f)–\(t) 年")
        case let (f?, nil): parts.append("\(f) 年起")
        case let (nil, t?): parts.append("到 \(t) 年")
        default: break
        }
        if !performerIds.isEmpty { parts.append("\(performerIds.count) 位歌手") }
        if !genres.isEmpty { parts.append(genres.joined(separator: "/")) }
        if !albumIds.isEmpty { parts.append("\(albumIds.count) 张专辑") }
        if !songIds.isEmpty { parts.append("\(songIds.count) 首指定的歌") }
        if jpOnly { parts.append("仅日文") }
        if distinctLines { parts.append("重复行只算一次") }
        return parts.isEmpty ? "整个语料库" : parts.joined(separator: " · ")
    }
}

public struct AnalysisSummary: Decodable, Hashable, Identifiable, Sendable {
    public var id: Int64
    public var kind: String
    public var label: String
    public var filter: StatsFilter
    public var songCount: Int64
    public var scopeHash: String
    public var resultHash: String
    public var appVersion: String
    public var rerunOf: Int64?
    public var createdAt: String

    public var kindLabel: String {
        ["corpusReport": "语料统计", "frequency": "词频表", "corpusFrequency": "词频（重复感知）",
         "collocations": "搭配", "keyness": "子语料对比"][kind] ?? kind
    }
}

/// `analysisGet`：一条存档的完整记录（存档页「详情」用）。结果本身不解
public struct AnalysisRunDetail: Decodable, Hashable, Sendable {
    public struct Spec: Decodable, Hashable, Sendable { public var kind: String }
    public struct Scope: Decodable, Hashable, Sendable { public var songCount: Int64; public var lineCount: Int64 }
    public struct Tokenizers: Decodable, Hashable, Sendable { public var runs: [LyricTokenizerRun]; public var unknownLines: Int64 }
    public var id: Int64
    public var label: String
    public var spec: Spec
    public var filter: StatsFilter
    public var scope: Scope
    public var scopeHash: String
    public var tokenizers: Tokenizers
    /// 口径：键 → 值（数组或标量），按 Windows 的方式显示
    public var method: [String: LyricJSON]
    public var resultHash: String
    public var snapshot: CorpusSnapshot?
    public var schemaVersion: Int64
    public var appVersion: String
    public var rerunOf: Int64?
    public var createdAt: String
}

/// 存一次分析时返回的完整记录。界面只用得到 id 和摘要那几项，结果本身不解
public struct AnalysisRunRef: Decodable, Hashable, Sendable {
    public var id: Int64
    public var label: String
    public var createdAt: String
}

public struct ScopeDiff: Decodable, Hashable, Sendable {
    public var added: [String]
    public var removed: [String]
    public var lyricsChanged: [String]
}

public struct MetricDiff: Decodable, Hashable, Identifiable, Sendable {
    public var name: String
    public var before: Double?
    public var after: Double?
    public var id: String { name }
}

public struct WordDiff: Decodable, Hashable, Identifiable, Sendable {
    public var lemma: String
    public var pos: String
    public var beforeRank: Int?
    public var afterRank: Int?
    public var beforeFreq: Int64?
    public var afterFreq: Int64?
    public var id: String { "\(lemma)|\(pos)" }
}

public struct Comparison: Decodable, Hashable, Sendable {
    public var before: AnalysisSummary
    public var after: AnalysisSummary
    public var resultIdentical: Bool
    public var sameCorpus: Bool?
    public var scope: ScopeDiff
    public var metrics: [MetricDiff]
    public var words: [WordDiff]
    /// 一句句人话：为什么不一样
    public var reasons: [String]
}
