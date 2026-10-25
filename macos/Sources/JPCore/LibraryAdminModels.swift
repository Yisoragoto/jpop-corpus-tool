import Foundation

public struct SongEdit: Codable, Equatable, Sendable {
    public var title: String
    public var artist: String
    public var year: String
    public var album: String
    public var genre: String
    public init(title: String, artist: String, year: String = "", album: String = "", genre: String = "") {
        self.title = title; self.artist = artist; self.year = year; self.album = album; self.genre = genre
    }
    public init(_ t: Track) { self.init(title: t.title, artist: t.artist, year: t.year, album: t.album, genre: t.genre) }
}
public struct TokenEdit: Codable, Hashable, Sendable {
    public var surface: String
    public var lemma: String
    public var pos: String
    public init(_ surface: String, lemma: String? = nil, pos: String = "NOUN") {
        self.surface = surface; self.lemma = lemma ?? surface; self.pos = pos
    }
}
public struct RelinkRequest: Codable, Equatable, Sendable {
    public var songId: String
    public var path: String
    public init(songId: String, path: String) { self.songId = songId; self.path = path }
}
public struct MembershipInput: Codable, Sendable {
    public var bandId: Int64
    public var memberId: Int64?
    public var memberName: String?
    public var part: String
    public var joined: String
    public var leftAt: String
    public init(bandId: Int64, memberId: Int64? = nil, memberName: String? = nil, part: String = "", joined: String = "", leftAt: String = "") {
        self.bandId = bandId; self.memberId = memberId; self.memberName = memberName; self.part = part; self.joined = joined; self.leftAt = leftAt
    }
}
public enum MembershipPutOutcome: String, Codable, Sendable {
    case inserted, updated, keptExisting, rejected
    public var note: String {
        switch self { case .inserted: return "加上了"; case .updated: return "改好了"; case .keptExisting: return "已有更可靠的记录，没改"; case .rejected: return "这一条之前被否掉了" }
    }
}
public struct MigrateOptions: Codable, Sendable {
    public var dictionaries = true
    public var history = true
    public init() {}
}

public struct EditReport: Decodable, Sendable {
    public var changed: [String]
    public var performers: Int
    public var albumId: Int64?
    public var correctionsRenamed: Int
    public var albumsRemoved: Int
    public var peopleRemoved: Int
    public var csv: String
}

public struct DeleteResult: Decodable, Sendable {
    public var title: String
    public var artist: String
    public var audioPath: String
    public var coverPath: String
    public var csv: String
    public var lyricLines: Int
    public var tokens: Int
    public var credits: Int
    public var plays: Int
    public var correctionsKept: Int
    public var albumsRemoved: Int
    public var peopleRemoved: Int
    public var pitchCacheRemoved: Int
    public var coverRemoved: Bool
}

public struct MissingAudio: Decodable, Sendable {
    public var songId: String
    public var title: String
    public var artist: String
    public var audioPath: String
}

public struct RelinkSuggestion: Decodable, Sendable {
    public var songId: String
    public var path: String
    public var reason: String
}

public struct RelinkResult: Decodable, Sendable {
    public var songId: String
    public var error: String
    public var durationSec: Double?
}

public struct RelinkBatch: Decodable, Sendable {
    public var results: [RelinkResult]
    public var csv: String
}

public struct CorrectionView: Decodable, Sendable {
    public var utteranceId: Int64
    public var songId: String
    public var artist: String
    public var title: String
    public var text: String
    public var tokens: [TokenEdit]
    public var original: [TokenEdit]
    public var corrected: Bool
}

public struct Membership: Decodable, Sendable {
    public var bandId: Int64
    public var memberId: Int64
    public var bandName: String
    public var memberName: String
    public var part: String
    public var joined: String
    public var leftAt: String
    public var source: String
    public var bandImage: String
    public var memberImage: String
}

public struct MembershipSuggestion: Decodable, Sendable {
    public var bandId: Int64
    public var memberId: Int64
    public var shared: Int64
    public var written: Int64
    public var bandName: String
    public var memberName: String
}

public struct ArtistRow: Decodable, Sendable {
    public var name: String
    public var imagePath: String
    public var artistType: String
    public var country: String
    public var formed: String
    public var updatedAt: String
    public var trackCount: Int64
    public var hasImage: Bool
}

public struct ArtistOutcome: Decodable, Sendable {
    public var name: String
    public var canonicalName: String
    public var artistType: String
    public var country: String
    public var formed: String
    public var ended: String
    public var hasEnded: Bool
    public var mbid: String
    public var imagePath: String
    public var memberships: Int
    public var notFound: Bool
}

public struct DurationBackfillReport: Decodable, Sendable {
    public var scanned: Int
    public var written: Int
    public var missing: Int
    public var unknown: Int
    public var failed: Int
    public var samples: [String]
}

public struct WatchedFolder: Decodable, Sendable {
    public var path: String
    public var reason: String
    public var depth: Int
    public var songs: Int
    public var unknown: Int
    public var new: Int
}

public struct NewAudio: Decodable, Sendable {
    public var folders: [WatchedFolder]
    public var scan: ScanResult
}

public struct MigratePlan: Decodable, Sendable {
    public var sourceSongs: Int
    public var newSongs: Int
    public var duplicateSongs: Int
    public var lyricLines: Int
    public var tokens: Int
    public var people: Int
    public var albums: Int
    public var credits: Int
    public var plays: Int
    public var favorites: Int
    public var corrections: Int
    public var dictTerms: Int
    public var jlptRows: Int
    public var targetHasDictionaries: Bool
    public var samples: [String]
}

public struct MigrateOutcome: Decodable, Sendable {
    public var songs: Int
    public var lyricLines: Int
    public var tokens: Int
    public var people: Int
    public var albums: Int
    public var credits: Int
    public var plays: Int
    public var favorites: Int
    public var corrections: Int
    public var dictTerms: Int
    public var jlptRows: Int
    public var lyricVersions: Int
    public var revisions: Int
    public var skipped: Int
    public var coversCopied: Int
    public var lyricsCopied: Int
    public var artistPhotosCopied: Int
    public var dictionariesBytes: Int
    public var sudachiBytes: Int
    public var dictionariesCopied: Bool
    public var sudachiCopied: Bool
    public var idMap: [[String]]
    public var tokenized: Tokenized
    public var warnings: [String]
}

public enum LibraryJobResult: Decodable, Sendable {
    case durations(DurationBackfillReport), artwork(Int), migration(MigrateOutcome)
    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if let n = try? c.decode(Int.self) { self = .artwork(n) }
        else if let r = try? c.decode(DurationBackfillReport.self) { self = .durations(r) }
        else { self = .migration(try c.decode(MigrateOutcome.self)) }
    }
    public var summary: String {
        switch self {
        case .durations(let r): return "扫描 \(r.scanned) 首：写入 \(r.written)，文件缺失 \(r.missing)，无时长 \(r.unknown)，失败 \(r.failed)" + (r.samples.isEmpty ? "" : "\n" + r.samples.joined(separator: "\n"))
        case .artwork(let n): return "补齐 \(n) 张专辑封面"
        case .migration(let r): return "迁移 \(r.songs) 首，跳过 \(r.skipped) 首，补分词 \(r.tokenized.songs) 首\n" + r.warnings.joined(separator: "\n")
        }
    }
}

public struct LemmaForm: Codable, Sendable { public var lemma: String; public var normalized: String; public var reading: String }
