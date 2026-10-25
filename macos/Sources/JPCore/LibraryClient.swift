import Foundation

/// 首页、专辑、歌手、收藏、全库搜索、来源追踪、分析存档。方法名和 jp-ffi 的方法名一致
extension CorpusClient {
    public func homeSummary() async throws -> HomeSummary {
        let offset = TimeZone.current.secondsFromGMT() / 60
        return try await perform { try $0.call("homeSummary", ["utcOffsetMinutes": AnyEncodable(offset)]) }
    }

    public func albums() async throws -> [Album] {
        try await perform { try $0.call("albums") }
    }

    public func albumTracks(_ albumId: Int64) async throws -> [Track] {
        try await perform { try $0.call("albumTracks", ["albumId": AnyEncodable(albumId)]) }
    }

    public func peopleDirectory() async throws -> [DirectoryEntry] {
        try await perform { try $0.call("peopleDirectory") }
    }

    /// 不在名录里的人（比如没有自己作品的成员）只能按 id 读名字和照片
    public func personById(_ personId: Int64) async throws -> PersonSummary? {
        try await perform { try $0.call("personById", ["personId": AnyEncodable(personId)]) }
    }

    public func personWorks(_ personId: Int64) async throws -> [PersonWork] {
        try await perform { try $0.call("personWorks", ["personId": AnyEncodable(personId)]) }
    }

    public func collaborators(_ personId: Int64, limit: Int = 24) async throws -> [Collaborator] {
        try await perform { try $0.call("collaborators", ["personId": AnyEncodable(personId), "limit": AnyEncodable(limit)]) }
    }

    public func trackCredits(_ songId: String) async throws -> [Credit] {
        try await perform { try $0.call("trackCredits", ["songId": AnyEncodable(songId)]) }
    }

    /// 收藏一首歌（和 Windows 版同一张表，entityType 是 "song"）
    public func isFavorite(song songId: String) async throws -> Bool {
        try await perform { try $0.call("isFavorite", ["entityType": AnyEncodable("song"), "entityId": AnyEncodable(songId)]) }
    }

    /// 返回切换之后是不是收藏着
    public func toggleFavorite(song songId: String) async throws -> Bool {
        try await perform { try $0.call("toggleFavorite", ["entityType": AnyEncodable("song"), "entityId": AnyEncodable(songId)]) }
    }

    public func quickSearch(_ query: String, perKind: Int = 6) async throws -> QuickSearchResults {
        try await perform { try $0.call("quickSearch", ["query": AnyEncodable(query), "perKind": AnyEncodable(perKind)]) }
    }

    public func songLineage(_ songId: String) async throws -> SongLineage {
        try await perform { try $0.call("songLineage", ["songId": AnyEncodable(songId)]) }
    }

    public func qualityOverview(limit: Int = 50) async throws -> QualityOverview {
        try await perform { try $0.call("qualityOverview", ["limit": AnyEncodable(limit)]) }
    }

    // ── 分析存档 ──

    public func analysisList(limit: Int = 200) async throws -> [AnalysisSummary] {
        try await perform { try $0.call("analysisList", ["limit": AnyEncodable(limit)]) }
    }

    public func analysisGet(_ id: Int64) async throws -> AnalysisRunDetail? {
        try await perform { try $0.call("analysisGet", ["id": AnyEncodable(id)]) }
    }

    /// 把一次重复感知的词频存档
    public func archiveFrequency(_ query: FrequencyQuery, label: String) async throws -> AnalysisRunRef {
        try await archive(AnalysisSpec(kind: "corpusFrequency", query: AnyEncodable(query)), label: label)
    }

    public func archiveKeyness(_ query: KeynessQuery, label: String) async throws -> AnalysisRunRef {
        try await archive(AnalysisSpec(kind: "keyness", query: AnyEncodable(query)), label: label)
    }

    public func archiveCollocations(_ query: CollocationQuery, label: String) async throws -> AnalysisRunRef {
        try await archive(AnalysisSpec(kind: "collocations", query: AnyEncodable(query)), label: label)
    }

    private func archive(_ spec: AnalysisSpec, label: String) async throws -> AnalysisRunRef {
        let version = appVersion
        return try await perform {
            try $0.call("analysisRun", ["spec": AnyEncodable(spec), "label": AnyEncodable(label), "appVersion": AnyEncodable(version)])
        }
    }

    /// 拿记录里的条件在现在的库上再算一次，另存一条，并和原来那条比
    public func analysisRerun(_ id: Int64) async throws -> Comparison {
        let version = appVersion
        return try await perform { try $0.call("analysisRerun", ["id": AnyEncodable(id), "appVersion": AnyEncodable(version)]) }
    }

    public func analysisCompare(before: Int64, after: Int64) async throws -> Comparison {
        try await perform { try $0.call("analysisCompare", ["before": AnyEncodable(before), "after": AnyEncodable(after)]) }
    }

    public func analysisDelete(_ id: Int64) async throws {
        let _: Bool = try await perform { try $0.call("analysisDelete", ["id": AnyEncodable(id)]) }
    }

    /// 存档里记的程序版本
    private var appVersion: String {
        let v = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String
        return "macOS \(v ?? "dev")"
    }
}

/// Rust `AnalysisSpec`：`{"kind": "...", "query": {...}}`
private struct AnalysisSpec: Encodable, Sendable {
    var kind: String
    var query: AnyEncodable
}
