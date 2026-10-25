import Foundation
import JPCore

public struct NewSongIgnores: Codable, Equatable, Sendable {
    public var folders: [String] = []
    public var files: [String] = []
    public init(folders: [String] = [], files: [String] = []) { self.folders = folders; self.files = files }
}
public struct ImportOutcomeSummary: Equatable {
    public struct Failure: Equatable { public var title: String; public var error: String }
    public var imported = 0
    public var artists: [String] = []
    public var withoutLyrics: [String] = []
    public var failed: [Failure] = []
    public var cancelled = false
}
/// Windows newSongs.ts，存储/订阅交给 LibraryModel。
public enum NewSongs {
    public static let directImportLimit = 20
    public static func newItems(_ scan: ScanResult) -> [PlanItem] { scan.items.filter { $0.action.kind == .new } }
    public static func reviewReason(_ items: [PlanItem]) -> String? {
        if items.count > directImportLimit { return "一次 \(items.count) 首，先过一眼计划" }
        let unreadable = items.filter { $0.warning != nil }.count
        if unreadable > 0 { return "\(unreadable) 首读不出标签，曲名歌手是猜的" }
        let guessed = items.filter { $0.artistSource == "folder" }.count
        if guessed > 0 { return "\(guessed) 首的歌手是从文件夹名猜的" }
        let anonymous = items.filter { $0.artistSource == "none" || $0.artist.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }.count
        if anonymous > 0 { return "\(anonymous) 首定不出歌手" }; return nil
    }
    public static func describeNewSongs(_ items: [PlanItem], max: Int = 3) -> String {
        let named = items.prefix(max).map { $0.artist.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? $0.title : "\($0.title) — \($0.artist)" }.joined(separator: "、")
        return items.count > max ? "\(named) 等 \(items.count) 首" : named
    }
    public static func pathKey(_ path: String) -> String {
        path.replacingOccurrences(of: "\\", with: "/").replacingOccurrences(of: "/+$", with: "", options: .regularExpression).lowercased()
    }
    public static func folderOf(_ folders: [WatchedFolder], _ path: String) -> WatchedFolder? {
        folders.filter { pathKey(path).hasPrefix(pathKey($0.path) + "/") }.sorted { pathKey($0.path).count > pathKey($1.path).count }.first
    }
    public static func foldersOf(_ found: NewAudio, _ items: [PlanItem]) -> [String] {
        var counts: [String: Int] = [:], order: [String] = []
        for item in items { if let path = folderOf(found.folders, item.path)?.path { if counts[path] == nil { order.append(path) }; counts[path, default: 0] += 1 } }
        return order.sorted { counts[$0, default: 0] > counts[$1, default: 0] }
    }
    public static func summarizeImport(_ report: ImportResult, _ planned: [PlanItem]) -> ImportOutcomeSummary {
        var artists: [String: String] = [:]
        for item in planned { artists[pathKey(item.path)] = item.artist.trimmingCharacters(in: .whitespacesAndNewlines) }
        var r = ImportOutcomeSummary(); r.cancelled = report.cancelled
        for track in report.tracks {
            if track.outcome.kind == "failed" { r.failed.append(.init(title: track.title.isEmpty ? track.path : track.title, error: track.outcome.error ?? "")); continue }
            r.imported += 1
            if track.outcome.lyricLines == 0 { r.withoutLyrics.append(track.songId) }
            let a = artists[pathKey(track.path)] ?? ""
            if !a.isEmpty && !r.artists.contains(a) { r.artists.append(a) }
        }; return r
    }
    public static func parseIgnores(_ raw: String?) -> NewSongIgnores {
        guard let raw, let data = raw.data(using: .utf8), let obj = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return .init() }
        func strings(_ key: String) -> [String] { (obj[key] as? [Any] ?? []).compactMap { $0 as? String }.filter { !$0.isEmpty } }
        return .init(folders: strings("folders"), files: strings("files"))
    }
    private static func union(_ current: [String], _ added: [String]) -> [String] {
        var seen = Set(current.map(pathKey)), out = current
        for path in added { if seen.insert(pathKey(path)).inserted { out.append(path) } }; return out
    }
    public static func withIgnored(_ current: NewSongIgnores, _ added: NewSongIgnores) -> NewSongIgnores {
        .init(folders: union(current.folders, added.folders), files: union(current.files, added.files))
    }
    public static func withoutIgnoredFolder(_ current: NewSongIgnores, _ folder: String) -> NewSongIgnores {
        .init(folders: current.folders.filter { pathKey($0) != pathKey(folder) }, files: current.files)
    }
}
