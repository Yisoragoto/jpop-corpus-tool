import Foundation
import JPCore

public struct AlbumGroup: Identifiable, Sendable {
    public var key: String; public var albumId: Int64?; public var title: String; public var year: String; public var coverPath: String; public var tracks: [Track]
    public var kind = "album"
    public var id: String { "\(key)|\(tracks.first?.id ?? "")" }
    public init(key: String,albumId: Int64?,title: String,year: String,coverPath: String = "",tracks: [Track]) { self.key = key; self.albumId = albumId; self.title = title; self.year = year; self.coverPath = coverPath; self.tracks = tracks }
}
public enum PersonReleases {
    public static func albumKey(_ t: Track) -> String { t.albumId.map { "id:\($0)" } ?? (t.album.isEmpty ? "single" : "t:\(t.album)") }
    public static func groupByAlbum(_ tracks: [Track]) -> [AlbumGroup] {
        var groups: [AlbumGroup] = []; var indices: [String:Int] = [:]
        for t in tracks {
            let key = albumKey(t)
            if indices[key] == nil { indices[key] = groups.count; groups.append(AlbumGroup(key: key,albumId: t.albumId,title: t.album.isEmpty ? "单曲" : t.album,year: t.year,coverPath: t.coverPath,tracks: [])) }
            let i = indices[key]!
            if !groups[i].tracks.contains(where: { $0.id == t.id }) { groups[i].tracks.append(t) }
            if !t.year.isEmpty && (groups[i].year.isEmpty || t.year < groups[i].year) { groups[i].year = t.year }
            if groups[i].coverPath.isEmpty { groups[i].coverPath = t.coverPath }
        }
        return groups.sorted { a,b in
            let x = a.year.isEmpty ? "9999" : a.year, y = b.year.isEmpty ? "9999" : b.year
            return x == y ? a.title.compare(b.title,locale: Locale(identifier: "ja")) == .orderedAscending : x < y
        }
    }
    private static let suffix = #"\s*[-–—]\s*(single|ep)\s*$"#
    public static func releaseTitle(_ title: String) -> String {
        let stripped = title.replacingOccurrences(of: suffix,with: "",options: [.regularExpression,.caseInsensitive]); return stripped.isEmpty ? title : stripped
    }
    private static func titleKey(_ text: String) -> String { text.precomposedStringWithCompatibilityMapping.lowercased().replacingOccurrences(of: #"\s+"#,with: "",options: .regularExpression) }
    public static func releaseKind(_ g: AlbumGroup,releaseType: String = "") -> String {
        if g.key == "single" { return "single" }; let type = releaseType.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if type == "single" || type == "ep" { return type }; if !type.isEmpty { return "album" }
        if let range = g.title.range(of: suffix,options: [.regularExpression,.caseInsensitive]) { return g.title[range].lowercased().hasSuffix("ep") ? "ep" : "single" }
        let bare = titleKey(releaseTitle(g.title)); return g.tracks.count <= 3 && g.tracks.contains { titleKey($0.title) == bare } ? "single" : "album"
    }
    public static func splitReleases(_ groups: [AlbumGroup],types: [Int64:String] = [:]) -> (singles: [AlbumGroup],albums: [AlbumGroup]) {
        var singles: [AlbumGroup] = [], albums: [AlbumGroup] = []
        for var g in groups {
            if g.key == "single" {
                for t in g.tracks { var single = g; single.title = t.title; single.year = t.year.isEmpty ? g.year : t.year; single.coverPath = t.coverPath.isEmpty ? g.coverPath : t.coverPath; single.tracks = [t]; single.kind = "single"; singles.append(single) }; continue
            }
            g.kind = releaseKind(g,releaseType: g.albumId.flatMap { types[$0] } ?? ""); if g.kind == "album" { albums.append(g) } else { singles.append(g) }
        }
        func newest(_ items: [AlbumGroup]) -> [AlbumGroup] { items.enumerated().sorted { $0.element.year == $1.element.year ? $0.offset < $1.offset : $0.element.year > $1.element.year }.map(\.element) }
        return (newest(singles),newest(albums))
    }
    public static func allSongs(_ groups: [AlbumGroup]) -> [Track] {
        var seen = Set<String>(); var songs: [(track: Track,year: String,order: Int)] = []
        for g in groups { for t in g.tracks where seen.insert(t.id).inserted { songs.append((t,t.year.isEmpty ? g.year : t.year,songs.count)) } }
        return songs.sorted { $0.year == $1.year ? $0.order < $1.order : $0.year > $1.year }.map(\.track)
    }
}
