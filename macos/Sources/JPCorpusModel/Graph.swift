import Foundation
import JPCore

public struct GraphNode: Sendable {
    public var id: Int64; public var name: String; public var imagePath: String; public var role: String
    public var isBand: Bool; public var songs: Int64; public var r: Double
    public var x = 0.0, y = 0.0, vx = 0.0, vy = 0.0
    public var fx: Double?, fy: Double?; public var depth: Int
}
public struct GraphLink: Sendable { public var s: Int; public var t: Int; public var shared: Int64; public var member: Bool }
public struct RelationGraph: Sendable {
    public var nodes: [GraphNode] = []; public var links: [GraphLink] = []; public var neighbors: [[Int]] = []
    public init() {}
}
public struct GraphOptions: Sendable {
    public var focus: Int64?; public var depth = 1; public var roles: Set<String>?; public var minShared: Int64 = 1
    public init(focus: Int64? = nil, depth: Int = 1, roles: Set<String>? = nil, minShared: Int64 = 1) {
        self.focus = focus; self.depth = depth; self.roles = roles; self.minShared = minShared
    }
}
public struct GraphForces: Sendable {
    public var repel = 900.0, linkDistance = 70.0, center = 0.035
    public init() {}
}
/// graph.ts：数组保留 JS Map 插入顺序，力参数与运算顺序一致。
public enum GraphLayout {
    public static func radiusFor(_ songs: Int64) -> Double { min(23, 15 + log2(1 + Double(max(0, songs))) * 1.1) }
    private static func mainRole(_ e: DirectoryEntry) -> String {
        let roles = [("performer", e.performer), ("composer", e.composer), ("lyricist", e.lyricist), ("arranger", e.arranger)]
        var best = "performer", count: Int64 = -1
        for (role,n) in roles where n > count { best = role; count = n }; return best
    }
    public static func buildGraph(_ entries: [DirectoryEntry], _ edges: [CollabEdge], _ memberships: [Membership], _ opts: GraphOptions = GraphOptions()) -> RelationGraph {
        let byId = Dictionary(uniqueKeysWithValues: entries.map { ($0.id, $0) })
        var bands = Set(memberships.map(\.bandId)); bands.formUnion(entries.filter { $0.artistType == "Group" }.map(\.id))
        struct Pair { var a: Int64; var b: Int64; var shared: Int64; var member: Bool }
        var pairs: [Pair] = []; var pairIndex: [String: Int] = [:]
        func key(_ a: Int64, _ b: Int64) -> String { "\(min(a,b))-\(max(a,b))" }
        func put(_ p: Pair) {
            let k = key(p.a,p.b)
            if let i = pairIndex[k] { pairs[i] = p } else { pairIndex[k] = pairs.count; pairs.append(p) }
        }
        for e in edges where e.shared >= opts.minShared { put(Pair(a: e.source, b: e.target, shared: e.shared, member: false)) }
        for m in memberships {
            let old = pairIndex[key(m.bandId, m.memberId)].map { pairs[$0].shared } ?? 0
            put(Pair(a: m.bandId, b: m.memberId, shared: old, member: true))
        }
        var adjacency: [Int64: [Int64]] = [:]
        for p in pairs { adjacency[p.a, default: []].append(p.b); adjacency[p.b, default: []].append(p.a) }
        var depths: [(Int64, Int)] = []; var seen = Set<Int64>()
        func add(_ id: Int64, _ depth: Int) { if seen.insert(id).inserted { depths.append((id, depth)) } }
        if let focus = opts.focus {
            add(focus, 0); var frontier = [focus]
            if opts.depth > 0 { for d in 1...opts.depth {
                var next: [Int64] = []
                for id in frontier { for n in adjacency[id] ?? [] where !seen.contains(n) { add(n,d); next.append(n) } }
                frontier = next
            } }
        } else {
            for e in entries { add(e.id,0) }; for m in memberships { add(m.memberId,0) }
        }
        let memberIds = Set(memberships.map(\.memberId)); var g = RelationGraph(); var index: [Int64: Int] = [:]
        for (id, depth) in depths {
            let e = byId[id], m = memberships.first { $0.memberId == id || $0.bandId == id }
            let name = e?.name ?? m.map { $0.memberId == id ? $0.memberName : $0.bandName } ?? ""
            if name.isEmpty { continue }; let role = e.map(mainRole) ?? "performer"
            if let roles = opts.roles, id != opts.focus, !bands.contains(id), !memberIds.contains(id), !roles.contains(role) { continue }
            let fallback = m.map { $0.memberId == id ? $0.memberImage : $0.bandImage } ?? ""
            let image = e?.imagePath ?? ""
            index[id] = g.nodes.count
            g.nodes.append(GraphNode(id: id, name: name, imagePath: image.isEmpty ? fallback : image, role: role, isBand: bands.contains(id), songs: e?.songs ?? 0, r: radiusFor(e?.songs ?? 0) + (id == opts.focus ? 3 : 0), depth: depth))
        }
        for p in pairs { if let s = index[p.a], let t = index[p.b] { g.links.append(GraphLink(s: s,t: t,shared: p.shared,member: p.member)) } }
        g.neighbors = g.nodes.map { _ in [] }
        for l in g.links { g.neighbors[l.s].append(l.t); g.neighbors[l.t].append(l.s) }
        let order = g.nodes.indices.sorted { g.neighbors[$0].count == g.neighbors[$1].count ? $0 < $1 : g.neighbors[$0].count > g.neighbors[$1].count }
        let golden = Double.pi * (3 - sqrt(5))
        for (k,i) in order.enumerated() {
            let r = 14 * sqrt(Double(k) + 0.5) * (g.neighbors[i].isEmpty ? 1.6 : 1)
            g.nodes[i].x = r * cos(Double(k) * golden); g.nodes[i].y = r * sin(Double(k) * golden)
        }
        return g
    }
    public static func nodeAt(_ nodes: [GraphNode], x: Double, y: Double, slop: Double = 4, maxRadius: Double = .infinity) -> Int {
        var best = -1, bestD = Double.infinity
        for (i,n) in nodes.enumerated() { let d = hypot(n.x-x,n.y-y); if d <= min(n.r,maxRadius)+slop && d < bestD { best = i; bestD = d } }
        return best
    }
}
public struct GraphSimulation: Sendable {
    public var graph: RelationGraph; public var forces: GraphForces
    public private(set) var alpha = 1.0
    public let alphaMin = 0.003
    public var running: Bool { alpha > alphaMin }
    public init(_ graph: RelationGraph, forces: GraphForces = GraphForces(), alpha: Double = 1) { self.graph = graph; self.forces = forces; self.alpha = alpha }
    public mutating func reheat(_ to: Double = 0.6) { alpha = max(alpha,to) }
    public mutating func tick() {
        var nodes = graph.nodes; let a = alpha, repel = forces.repel
        for l in graph.links {
            let s = nodes[l.s], t = nodes[l.t]
            let dx = t.x+t.vx-s.x-s.vx, dy = t.y+t.vy-s.y-s.vy, raw = hypot(dx,dy)
            let d = raw == 0 ? 1 : raw
            let want = forces.linkDistance * (l.member ? 0.7 : 1) + s.r+t.r
            let strength = min(1,0.25+log2(1+Double(l.shared))*0.15)
            let k = ((d-want)/d)*a*strength*0.5
            nodes[l.t].vx -= dx*k; nodes[l.t].vy -= dy*k; nodes[l.s].vx += dx*k; nodes[l.s].vy += dy*k
        }
        let cell = sqrt(repel)*4
        struct Cell: Hashable { var x: Int; var y: Int }
        var grid: [Cell:[Int]] = [:]
        for (i,n) in nodes.enumerated() { grid[Cell(x: Int(floor(n.x/cell)),y: Int(floor(n.y/cell))),default: []].append(i) }
        for i in nodes.indices {
            let cx = Int(floor(nodes[i].x/cell)), cy = Int(floor(nodes[i].y/cell))
            for gx in (cx-1)...(cx+1) { for gy in (cy-1)...(cy+1) { for j in grid[Cell(x: gx,y: gy)] ?? [] where j > i {
                var dx = nodes[j].x-nodes[i].x, dy = nodes[j].y-nodes[i].y, d2 = dx*dx+dy*dy
                if d2 == 0 { dx = Double((i*7919)%13-6); dy = Double((j*104729)%13-6); if dx == 0 { dx = 1 }; if dy == 0 { dy = 1 }; d2 = dx*dx+dy*dy }
                let minD = nodes[i].r+nodes[j].r+18
                let f = repel*a/max(d2,minD*minD*0.25)+max(0,minD-sqrt(d2))*0.9, d = sqrt(d2)
                let fx = dx/d*f, fy = dy/d*f
                nodes[i].vx -= fx; nodes[i].vy -= fy; nodes[j].vx += fx; nodes[j].vy += fy
            } } }
        }
        for i in nodes.indices {
            nodes[i].vx -= nodes[i].x*forces.center*a; nodes[i].vy -= nodes[i].y*forces.center*a
            if let x = nodes[i].fx, let y = nodes[i].fy { nodes[i].x = x; nodes[i].y = y; nodes[i].vx = 0; nodes[i].vy = 0; continue }
            nodes[i].vx *= 0.58; nodes[i].vy *= 0.58; nodes[i].x += nodes[i].vx; nodes[i].y += nodes[i].vy
        }
        graph.nodes = nodes; alpha += (0-alpha)*(1-pow(0.003,1.0/300))
    }
    public mutating func settle(_ maxTicks: Int = 400) { for _ in 0..<maxTicks { if !running { break }; tick() } }
}
