import Foundation
import JPCore

public struct DatedRelease: Sendable { public var release: TimelineRelease; public var at: Double }
public struct TimelineRow: Identifiable, Sendable {
    public var id: Int64; public var name: String; public var imagePath: String; public var isBand: Bool
    public var releases: [DatedRelease]; public var start: Double; public var end: Double; public var endedAt: Double?; public var under: Int64?
}
public struct TimelineConnector: Sendable { public var fromRow: Int; public var toRow: Int; public var fromAt: Double; public var toAt: Double }
public struct TimelineLayout: Sendable { public var rows: [TimelineRow]; public var connectors: [TimelineConnector]; public var min: Double; public var max: Double }
public struct WarpItem: Sendable {
    public var at: Double; public var size: Double
    public init(at: Double, size: Double) { self.at = at; self.size = size }
}
public struct PackItem: Sendable {
    public var x: Double; public var size: Double
    public init(x: Double, size: Double) { self.x = x; self.size = size }
}
public struct WarpedYears: Sendable {
    public var first: Int; public var widths: [Double]; public var starts: [Double]; public var width: Double
    public func x(_ at: Double) -> Double {
        let k = Swift.min(widths.count-1, Swift.max(0,Int(floor(at))-first))
        let frac = Swift.min(1,Swift.max(0,at-Double(first+k)))
        return starts[k]+frac*widths[k]
    }
}
public struct TreeRelease: Sendable { public var release: DatedRelease; public var songs: [PersonWork]; public var personId: Int64 }
public struct TreeAfter: Sendable { public var bandName: String; public var ended: String }
public struct TreePart: Sendable { public var personId: Int64; public var name: String; public var after: TreeAfter?; public var items: [TreeRelease] }
public enum AxisColumn<T> { case year(Int, [T], ended: Bool); case gap(Int, Int) }

/// Windows timeline.ts 与 releaseAxis.ts；日期按原近似式，不引入日历差异。
public enum ReleaseTimeline {
    public static func yearValue(_ date: String) -> Double? {
        let s = date.trimmingCharacters(in: .whitespacesAndNewlines) as NSString
        let re = try! NSRegularExpression(pattern: "^([0-9]{4})(?:-([0-9]{2})(?:-([0-9]{2}))?)?")
        guard let m = re.firstMatch(in: s as String,range: NSRange(location: 0,length: s.length)), let year = Double(s.substring(with: m.range(at: 1))) else { return nil }
        if m.range(at: 2).location == NSNotFound { return year+0.5 }
        let month = min(12,max(1,Double(s.substring(with: m.range(at: 2)))!))
        let day = m.range(at: 3).location == NSNotFound ? 15 : min(31,max(1,Double(s.substring(with: m.range(at: 3)))!))
        return year+(month-1)/12+(day-1)/365
    }
    public static func buildTimeline(_ dir: LibraryDirectory, _ releases: [TimelineRelease], query: String = "") -> TimelineLayout {
        var byPerson: [Int64:[DatedRelease]] = [:]; var ids: [Int64] = []
        for r in releases { if let at = yearValue(r.date) { if byPerson[r.personId] == nil { ids.append(r.personId) }; byPerson[r.personId,default: []].append(DatedRelease(release: r,at: at)) } }
        for id in ids { byPerson[id] = byPerson[id]!.enumerated().sorted { $0.element.at == $1.element.at ? $0.offset < $1.offset : $0.element.at < $1.element.at }.map(\.element) }
        func makeRow(_ id: Int64,_ under: Int64?) -> TimelineRow? {
            guard let list = byPerson[id], let first = list.first, let last = list.last, let e = dir.byId[id] else { return nil }
            let ended = yearValue(e.ended)
            return TimelineRow(id: id,name: e.name,imagePath: e.imagePath,isBand: !(dir.membersOf[id] ?? []).isEmpty || e.artistType == "Group",releases: list,start: first.at,end: max(last.at,ended ?? last.at),endedAt: ended,under: under)
        }
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        func wanted(_ id: Int64) -> Bool { q.isEmpty || (dir.byId[id]?.name.lowercased().contains(q) ?? false) || (dir.membersOf[id] ?? []).contains { $0.memberName.lowercased().contains(q) } }
        let firsts = ids.sorted {
            let a = byPerson[$0]!.first!.at, b = byPerson[$1]!.first!.at
            return a == b ? (dir.byId[$0]?.name ?? "").compare(dir.byId[$1]?.name ?? "",locale: Locale(identifier: "ja")) == .orderedAscending : a < b
        }
        var rows: [TimelineRow] = [], connectors: [TimelineConnector] = []; var placed = Set<Int64>()
        for id in firsts {
            if placed.contains(id) { continue }
            if (dir.bandsOf[id] ?? []).contains(where: { byPerson[$0.bandId] != nil && !placed.contains($0.bandId) && wanted($0.bandId) }) { continue }
            guard wanted(id), let row = makeRow(id,nil) else { continue }
            placed.insert(id); let bi = rows.count; rows.append(row)
            for m in dir.membersOf[id] ?? [] {
                guard !placed.contains(m.memberId), let solo = makeRow(m.memberId,id) else { continue }
                placed.insert(m.memberId); let si = rows.count; rows.append(solo)
                if !m.leftAt.isEmpty, let left = yearValue(m.leftAt) ?? row.endedAt, let after = solo.releases.first(where: { $0.at >= left-0.01 }) {
                    connectors.append(TimelineConnector(fromRow: bi,toRow: si,fromAt: min(left,row.end),toAt: after.at))
                }
            }
        }
        for id in firsts where !placed.contains(id) && wanted(id) { if let row = makeRow(id,nil) { placed.insert(id); rows.append(row) } }
        let all = rows.flatMap { [$0.start,$0.end] }
        return TimelineLayout(rows: rows,connectors: connectors,min: all.min().map(floor) ?? 2000,max: all.max().map(ceil) ?? 2001)
    }
    public static func warpYears(_ rows: [[WarpItem]], min: Double, max: Double, perYear: Double, gap: Double = 6) -> WarpedYears {
        let first = Int(floor(min)), last = Swift.max(Int(floor(min)),Int(ceil(max)))
        var widths: [Double] = [], starts: [Double] = []; var acc = 0.0
        for year in first...last {
            var need = perYear
            for row in rows {
                let inside = row.filter { Int(floor($0.at)) == year }
                if !inside.isEmpty { need = Swift.max(need,inside.reduce(gap) { $0+$1.size+gap }) }
            }
            widths.append(need); starts.append(acc); acc += need
        }
        return WarpedYears(first: first,widths: widths,starts: starts,width: acc)
    }
    public static func packRow(_ items: [PackItem], gap: Double = 6) -> [Double] {
        var centers = items.map { _ in 0.0 }, right = -Double.infinity
        let order = items.indices.sorted { items[$0].x == items[$1].x ? $0 < $1 : items[$0].x < items[$1].x }
        for i in order { let it = items[i], cx = max(items[i].x,right+gap+items[i].size/2); centers[i] = cx; right = cx+it.size/2 }
        return centers
    }
    public static func buildTree(_ personId: Int64,_ dir: LibraryDirectory,_ releases: [TimelineRelease],_ worksOf: [Int64:[PersonWork]]) -> [TreePart] {
        func items(_ id: Int64,_ list: [TimelineRelease]) -> [TreeRelease] {
            list.enumerated().sorted { (yearValue($0.element.date) ?? 0) == (yearValue($1.element.date) ?? 0) ? $0.offset < $1.offset : (yearValue($0.element.date) ?? 0) < (yearValue($1.element.date) ?? 0) }.map { _, r in
                let own = (worksOf[id] ?? []).filter { $0.performerIds.contains(r.personId) }
                let songs = own.filter { r.albumId != nil ? $0.track.albumId == r.albumId : $0.id == r.songId }
                return TreeRelease(release: DatedRelease(release: r,at: yearValue(r.date) ?? 0),songs: songs,personId: id)
            }
        }
        let entry = dir.byId[personId]
        var parts = [TreePart(personId: personId,name: entry?.name ?? "",after: nil,items: items(personId,releases.filter { $0.personId == personId && yearValue($0.date) != nil }))]
        if let e = entry, let ended = yearValue(e.ended) {
            for m in dir.membersOf[personId] ?? [] where !m.leftAt.isEmpty {
                let later = releases.filter { $0.personId == m.memberId && (yearValue($0.date) ?? 0) >= ended-0.01 }
                if !later.isEmpty { parts.append(TreePart(personId: m.memberId,name: m.memberName,after: TreeAfter(bandName: e.name,ended: e.ended),items: items(m.memberId,later))) }
            }
        }
        return parts
    }
    public static func buildAxisColumns<T>(_ items: [T], endYear: Int?, year: (T) -> Int) -> [AxisColumn<T>] {
        guard !items.isEmpty else { return [] }; let byYear = Dictionary(grouping: items,by: year)
        let first = byYear.keys.min()!, last = max(byYear.keys.max()!,endYear ?? Int.min)
        var columns: [AxisColumn<T>] = []; var empty: Int?
        func flush(_ until: Int) {
            guard let start = empty else { return }
            if until-start >= 2 { columns.append(.gap(start,until-1)) } else { for y in start..<until { columns.append(.year(y,[],ended: false)) } }; empty = nil
        }
        for y in first...last {
            let list = byYear[y], ended = y == endYear
            if list == nil && !ended { if empty == nil { empty = y }; continue }
            flush(y); columns.append(.year(y,list ?? [],ended: ended))
        }
        return columns
    }
}
