import Foundation
import JPCore
import Observation

public struct ActiveScope: Codable, Equatable, Sendable {
    public var id: Int64; public var name: String; public var filter: StatsFilter
    public init(id: Int64,name: String,filter: StatsFilter) { self.id = id; self.name = name; self.filter = filter }
}
public enum SubcorpusScope {
    public static func withScope(_ filter: StatsFilter,_ scope: ActiveScope?) -> StatsFilter {
        guard let scope else { return filter }; var s = scope.filter
        s.performerIds = filter.performerIds.isEmpty ? s.performerIds : filter.performerIds
        s.jpOnly = filter.jpOnly; s.distinctLines = filter.distinctLines; return s
    }
    public static func mergeSubcorpus(_ local: Subcorpus,_ scope: ActiveScope?) -> Subcorpus {
        guard let scope else { return local }; let s = scope.filter; var out = local
        if out.performerIds.isEmpty { out.performerIds = s.performerIds }
        out.yearFrom = out.yearFrom ?? s.yearFrom; out.yearTo = out.yearTo ?? s.yearTo
        if out.albumIds.isEmpty { out.albumIds = s.albumIds }; if out.genres.isEmpty { out.genres = s.genres }
        if out.songIds.isEmpty { out.songIds = s.songIds }; if out.languages.isEmpty { out.languages = s.languages }
        out.minCompleteness = out.minCompleteness ?? s.minCompleteness; return out
    }
    public static func subcorpus(_ f: StatsFilter) -> Subcorpus {
        var s = Subcorpus(performerIds: f.performerIds,yearFrom: f.yearFrom,yearTo: f.yearTo,songIds: f.songIds)
        s.albumIds = f.albumIds; s.genres = f.genres; s.languages = f.languages; s.minCompleteness = f.minCompleteness; return s
    }
    public static func normalizeFilter(_ f: StatsFilter) -> StatsFilter { var n = f; n.performerIds.sort(); return n }
    public static func sameFilter(_ a: StatsFilter,_ b: StatsFilter) -> Bool { normalizeFilter(a) == normalizeFilter(b) }
    public static func isWholeCorpus(_ f: StatsFilter) -> Bool { subcorpus(f).isAll }
    public static let languageLabels = ["ja":"日文","en":"英文","ko":"韩文","zh":"中文","mixed":"混合","unknown":"未知","":"未判断"]
    public static func describeFilter(_ f: StatsFilter, performerNames: [String] = []) -> String {
        var parts: [String] = []
        if !f.performerIds.isEmpty { parts.append(performerNames.isEmpty ? "\(f.performerIds.count) 位演唱者" : performerNames.joined(separator: "、")) }
        switch (f.yearFrom,f.yearTo) {
        case let (a?,b?): parts.append(a == b ? "\(a) 年" : "\(a)–\(b) 年")
        case let (a?,nil): parts.append("\(a) 年起")
        case let (nil,b?): parts.append("\(b) 年前")
        default: break
        }
        if !f.genres.isEmpty { parts.append(f.genres.joined(separator: " / ")) }
        if !f.languages.isEmpty { parts.append(f.languages.map { languageLabels[$0] ?? $0 }.joined(separator: " / ")) }
        if !f.albumIds.isEmpty { parts.append("\(f.albumIds.count) 张专辑") }; if !f.songIds.isEmpty { parts.append("\(f.songIds.count) 首指定的歌") }
        if let min = f.minCompleteness { parts.append("歌词完整度 ≥ \(Int(floor(min*100+0.5)))%") }
        let method = [f.jpOnly ? "仅日文" : "", f.distinctLines ? "副歌只算一遍" : ""].filter { !$0.isEmpty }.joined(separator: "，")
        return (parts.isEmpty ? "整个库" : parts.joined(separator: " · ")) + (method.isEmpty ? "" : "（\(method)）")
    }
}
@MainActor @Observable
public final class SubcorpusStore {
    public var active: ActiveScope? { didSet { if let active, let data = try? JSONEncoder().encode(active) { defaults.set(data,forKey: key) } else { defaults.removeObject(forKey: key) } } }
    public private(set) var saved: [SavedSubcorpus] = []
    @ObservationIgnored private let defaults: UserDefaults
    private let key = "jpop.activeSubcorpus"
    public init(defaults: UserDefaults) {
        self.defaults = defaults
        active = defaults.data(forKey: key).flatMap { try? JSONDecoder().decode(ActiveScope.self,from: $0) }
    }
    public func reload(_ client: CorpusClient) async throws {
        saved = try await client.subcorpusList()
        if let active {
            self.active = saved.first { $0.id == active.id }.map { ActiveScope(id: $0.id,name: $0.name,filter: $0.filter) }
        }
    }
    public func select(_ id: Int64?) { active = saved.first { $0.id == id }.map { ActiveScope(id: $0.id,name: $0.name,filter: $0.filter) } }
}
