import Foundation
import JPCore

public enum BehindRole: String, CaseIterable { case all, composer, lyricist, arranger }
public struct MemberRow { public var membership: Membership; public var entry: DirectoryEntry?; public var former: Bool }
public struct MusicianRow { public var entry: DirectoryEntry; public var isBand: Bool; public var members: [MemberRow]; public var bands: [Membership] }
public struct LibraryDirectory {
    public var musicians: [MusicianRow]; public var behind: [DirectoryEntry]; public var byId: [Int64: DirectoryEntry]
    public var bandsOf: [Int64: [Membership]]; public var membersOf: [Int64: [Membership]]
}
public struct MusicianHit { public var row: MusicianRow; public var memberHit: Bool }
public struct WorkSection: Identifiable {
    public var key: String; public var title: String; public var note: String; public var collapsed: Bool; public var works: [PersonWork]
    public var id: String { key }
}
/// Windows libraryIndex.ts，同一演唱署名归属规则。
public enum LibraryIndex {
    private static let ja = Locale(identifier: "ja")
    private static func before(_ a: String, _ b: String) -> Bool { a.compare(b, locale: ja) == .orderedAscending }
    public static func yearOf(_ date: String) -> String {
        let s = date.trimmingCharacters(in: .whitespacesAndNewlines)
        return s.range(of: "^[0-9]{4}", options: .regularExpression).map { String(s[$0]) } ?? s
    }
    public static func validDate(_ s: String) -> Bool {
        s.isEmpty || s == "?" || s.range(of: "^[0-9]{4}(-[0-9]{2}(-[0-9]{2})?)?$", options: .regularExpression) != nil
    }
    public static func spanOf(_ joined: String, _ leftAt: String) -> String {
        let from = yearOf(joined)
        return "\(from.isEmpty ? "?" : from)–\(leftAt.isEmpty ? "至今" : yearOf(leftAt))"
    }
    private static func memberOrder(_ a: Membership, _ b: Membership) -> Bool {
        let former = !a.leftAt.isEmpty
        if former != !b.leftAt.isEmpty { return !former }
        let ak = (former ? a.leftAt : a.joined), bk = (former ? b.leftAt : b.joined)
        let x = ak.isEmpty ? "9999" : ak, y = bk.isEmpty ? "9999" : bk
        return x == y ? before(a.memberName, b.memberName) : before(x, y)
    }
    public static func buildDirectory(_ entries: [DirectoryEntry], _ memberships: [Membership]) -> LibraryDirectory {
        let byId = Dictionary(uniqueKeysWithValues: entries.map { ($0.id, $0) })
        let members = Dictionary(grouping: memberships, by: \.bandId).mapValues { $0.sorted(by: memberOrder) }
        let bands = Dictionary(grouping: memberships, by: \.memberId)
        let musicians = entries.filter { $0.performer > 0 }.map { e -> MusicianRow in
            let rows = (members[e.id] ?? []).map { MemberRow(membership: $0, entry: byId[$0.memberId], former: !$0.leftAt.isEmpty) }
            return MusicianRow(entry: e, isBand: !rows.isEmpty || e.artistType == "Group", members: rows, bands: bands[e.id] ?? [])
        }.sorted { $0.entry.performer == $1.entry.performer ? before($0.entry.name, $1.entry.name) : $0.entry.performer > $1.entry.performer }
        let behind = entries.filter { $0.performer == 0 && bands[$0.id] == nil }.sorted { $0.songs == $1.songs ? before($0.name, $1.name) : $0.songs > $1.songs }
        return LibraryDirectory(musicians: musicians, behind: behind, byId: byId, bandsOf: bands, membersOf: members)
    }
    public static func filterMusicians(_ rows: [MusicianRow], _ query: String) -> [MusicianHit] {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return rows.compactMap { row in
            let hit = !q.isEmpty && row.members.contains { $0.membership.memberName.lowercased().contains(q) }
            return q.isEmpty || row.entry.name.lowercased().contains(q) || hit ? MusicianHit(row: row, memberHit: hit) : nil
        }
    }
    public static func roleCount(_ entry: DirectoryEntry, _ role: BehindRole) -> Int64 {
        switch role { case .all: return entry.songs; case .composer: return entry.composer; case .lyricist: return entry.lyricist; case .arranger: return entry.arranger }
    }
    public static func filterBehind(_ entries: [DirectoryEntry], _ role: BehindRole, _ query: String) -> [DirectoryEntry] {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return entries.filter { roleCount($0, role) > 0 && (q.isEmpty || $0.name.lowercased().contains(q)) }.sorted {
            roleCount($0, role) == roleCount($1, role) ? before($0.name, $1.name) : roleCount($0, role) > roleCount($1, role)
        }
    }
    public static func rolesOf(_ e: DirectoryEntry) -> [String] {
        let roles: [(String, Int64)] = [("performer", e.performer), ("composer", e.composer), ("lyricist", e.lyricist), ("arranger", e.arranger)]
        return roles.enumerated().filter { $0.element.1 > 0 }.sorted { $0.element.1 == $1.element.1 ? $0.offset < $1.offset : $0.element.1 > $1.element.1 }.map { $0.element.0 }
    }
    public static func musicianSubtitle(_ row: MusicianRow) -> String {
        if row.isBand { return "乐队 · \(row.entry.performer) 首" + (row.entry.ended.isEmpty ? "" : " · \(yearOf(row.entry.ended)) 解散") }
        if let b = row.bands.first { return "个人 · \(row.entry.performer) 首 · \(b.leftAt.isEmpty ? "也在" : "曾属") \(b.bandName)" }
        return "\(row.entry.performer) 首"
    }
    public static func memberSubtitle(_ m: MemberRow) -> String { m.former ? "前成员 · \(spanOf(m.membership.joined, m.membership.leftAt))" : (m.membership.part.isEmpty ? "成员" : m.membership.part) }
    public static func albumOwner(_ artist: String) -> String { String(artist.split(separator: "/", omittingEmptySubsequences: false).first ?? "").trimmingCharacters(in: .whitespacesAndNewlines) }
    public static func sameView(_ a: LibraryRoute, _ b: LibraryRoute) -> Bool {
        switch (a, b) { case (.song, .song): return true; case (.person(let x), .person(let y)): return x == y; case (.album(let x), .album(let y)): return x.id == y.id; default: return false }
    }
    public static func sectionWorks(_ personId: Int64, _ works: [PersonWork], _ bands: [Membership]) -> [WorkSection] {
        let own = works.filter { $0.performerIds.contains(personId) }; var taken = Set(own.map(\.id))
        let groups = bands.map { b -> (Membership, [PersonWork]) in
            let list = works.filter { !taken.contains($0.id) && $0.performerIds.contains(b.bandId) }; taken.formUnion(list.map(\.id)); return (b, list)
        }
        let rest = works.filter { !taken.contains($0.id) }; var sections: [WorkSection] = []
        if !own.isEmpty { sections.append(.init(key: "own", title: bands.isEmpty ? "作品" : "个人作品", note: "", collapsed: false, works: own)) }
        for (b, list) in groups where !list.isEmpty {
            let wrote = list.filter { !$0.roles.isEmpty }.count
            let note = "\(spanOf(b.joined, b.leftAt)) · 这些歌属于乐队" + (wrote > 0 ? "，其中 \(wrote) 首有署名" : "")
            sections.append(.init(key: "band-\(b.bandId)", title: "在 \(b.bandName) 时", note: note, collapsed: !own.isEmpty || !rest.isEmpty, works: list))
        }
        if !rest.isEmpty { sections.append(.init(key: "credits", title: sections.isEmpty ? "作品" : "合作 / 幕后", note: "", collapsed: false, works: rest)) }
        if sections.count == 1 { sections[0].title = "" }; return sections
    }
}
