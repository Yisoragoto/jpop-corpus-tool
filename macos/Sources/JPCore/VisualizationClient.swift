import Foundation

public struct CollabEdge: Codable, Hashable, Sendable {
    public var source: Int64; public var target: Int64; public var shared: Int64
    public init(source: Int64, target: Int64, shared: Int64) { self.source = source; self.target = target; self.shared = shared }
}
public struct TimelineRelease: Codable, Hashable, Sendable, Identifiable {
    public var personId: Int64; public var albumId: Int64?; public var title: String; public var date: String
    public var tracks: Int64; public var coverPath: String; public var songId: String
    public var id: String { "\(personId)|\(albumId.map(String.init) ?? songId)" }
    public init(personId: Int64, albumId: Int64? = nil, title: String, date: String, tracks: Int64 = 1, coverPath: String = "", songId: String = "") {
        self.personId = personId; self.albumId = albumId; self.title = title; self.date = date
        self.tracks = tracks; self.coverPath = coverPath; self.songId = songId
    }
}
public struct CloudFrequencyRow: Codable, Sendable {
    public var lemma: String; public var pos: String; public var freq: Int64; public var songCount: Int64
    public var surfaces: [String]; public var jlpt: String
    public init(lemma: String, pos: String = "", freq: Int64, songCount: Int64 = 1, surfaces: [String]? = nil, jlpt: String = "") {
        self.lemma = lemma; self.pos = pos; self.freq = freq; self.songCount = songCount; self.surfaces = surfaces ?? [lemma]; self.jlpt = jlpt
    }
}
public struct SavedSubcorpus: Codable, Sendable, Identifiable {
    public var id: Int64; public var name: String; public var filter: StatsFilter; public var note: String
    public var createdAt: String; public var updatedAt: String; public var songCount: Int; public var lineCount: Int64
}
public struct ScopePreview: Decodable, Sendable {
    public var songCount: Int; public var lineCount: Int64
}
public struct FacetCount: Decodable, Sendable, Identifiable {
    public var value: String; public var count: Int64
    public var id: String { value }
    public init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer(); value = try c.decode(String.self); count = try c.decode(Int64.self)
    }
}
public struct FilterFacets: Decodable, Sendable {
    public var yearMin: Int?; public var yearMax: Int?; public var songsWithoutYear: Int64
    public var genres: [FacetCount]; public var languages: [FacetCount]
}
public struct YearStats: Decodable, Sendable, Identifiable {
    public var year: String; public var trackCount: Int64; public var albumCount: Int64; public var vocabulary: Int64
    public var id: String { year }
}
public struct StatsReport: Decodable, Sendable {
    public var report: CorpusReport; public var performers: [String]; public var jpOnly: Bool; public var text: String
}
public struct CorpusReport: Decodable, Sendable {
    public var songCount: Int64; public var utteranceCount: Int64; public var tokenCount: Int64; public var typeCount: Int64
    public var ttr: Double; public var sttr: Double?; public var sttrChunk: Int?; public var hapaxCount: Int64; public var hapaxRatio: Double
    public var avgTypesPerSong: Double; public var avgTokensPerLine: Double; public var posDist: [ReportPos]; public var coverage: [ReportCoverage]; public var topWords: [FacetCount]
}
public struct ReportPos: Decodable, Sendable, Identifiable {
    public var pos: String; public var tokenCount: Int64; public var tokenPct: Double; public var typeCount: Int64
    public var id: String { pos }
}
public struct ReportCoverage: Decodable, Sendable, Identifiable { public var n: Int; public var pct: Double; public var id: Int { n } }
extension CorpusClient {
    public func timeline() async throws -> [YearStats] { try await perform { try $0.call("timeline") } }
    public func statsReport(_ filter: StatsFilter) async throws -> StatsReport { try await perform { try $0.call("statsReport",["filter": AnyEncodable(filter)]) } }
    public func collaborationEdges() async throws -> [CollabEdge] { try await perform { try $0.call("collaborationEdges") } }
    public func timelineReleases() async throws -> [TimelineRelease] { try await perform { try $0.call("timelineReleases") } }
    public func statsFrequency(filter: StatsFilter, pos: String? = nil, limit: Int = 300) async throws -> [CloudFrequencyRow] {
        try await perform { try $0.call("statsFrequency", ["filter": AnyEncodable(filter), "pos": AnyEncodable(pos), "limit": AnyEncodable(limit)]) }
    }
    public func subcorpusFacets() async throws -> FilterFacets { try await perform { try $0.call("subcorpusFacets") } }
    public func subcorpusList() async throws -> [SavedSubcorpus] { try await perform { try $0.call("subcorpusList") } }
    public func subcorpusPreview(_ filter: StatsFilter) async throws -> ScopePreview {
        try await perform { try $0.call("subcorpusPreview", ["filter": AnyEncodable(filter)]) }
    }
    public func subcorpusSave(name: String, filter: StatsFilter) async throws -> SavedSubcorpus {
        try await perform { try $0.call("subcorpusSave", ["name": AnyEncodable(name), "filter": AnyEncodable(filter)]) }
    }
    public func subcorpusDelete(_ id: Int64) async throws {
        let _: Bool = try await perform { try $0.call("subcorpusDelete", ["id": AnyEncodable(id)]) }
    }
}
