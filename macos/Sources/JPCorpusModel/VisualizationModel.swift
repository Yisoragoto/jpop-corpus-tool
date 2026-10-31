import Foundation
import JPCore
import Observation

public enum LibraryMode: String, CaseIterable { case songs = "歌曲", albums = "专辑", artists = "歌手", graph = "关系网", timeline = "时间轴", cloud = "词云" }

@MainActor @Observable
public final class VisualizationModel {
    public private(set) var entries: [DirectoryEntry] = []
    public private(set) var edges: [CollabEdge] = []
    public private(set) var memberships: [Membership] = []
    public private(set) var releases: [TimelineRelease] = []
    public private(set) var dataVersion = 0
    public private(set) var graph = RelationGraph()
    public var selected: Int64?
    public var around: Int64?
    public var graphQuery = ""
    public var roles: Set<String> = ["performer","composer","lyricist","arranger"]
    public var minShared: Int64 = 1
    public var timelineQuery = ""
    public var treePerson: Int64?
    public var treeFocus: String?
    public var perYear = 90.0
    public var cloudFilter: StatsFilter = { var f = StatsFilter(); f.jpOnly = true; return f }()
    public var cloudPos: [String] = ["NOUN"]
    public var picked: CloudWord?
    public private(set) var words: [CloudWord] = []
    public var placed: [PlacedWord] = []
    public var cloudSongs: [SongHits] = []
    public var cloudTruncated = false
    public var cloudPresent = false
    public var error: String?
    public private(set) var graphBusy = false
    public private(set) var cloudBusy = false
    @ObservationIgnored private var graphRequest = UUID()
    @ObservationIgnored private var cloudRequest = UUID()
    @ObservationIgnored private var loadedKey = ""
    @ObservationIgnored private var loadRequest = UUID()
    public init() {}
    public var directory: LibraryDirectory { LibraryIndex.buildDirectory(entries,memberships) }
    public var timeline: TimelineLayout { ReleaseTimeline.buildTimeline(directory,releases,query: timelineQuery) }

    public func load(_ client: CorpusClient, key: String) async {
        guard key != loadedKey else { return }
        let request = UUID(); loadRequest = request
        do {
            async let es = client.peopleDirectory()
            async let ed = client.collaborationEdges()
            async let ms = client.memberships()
            async let rs = client.timelineReleases()
            let data = try await (es,ed,ms,rs)
            guard !Task.isCancelled, request == loadRequest else { return }
            entries = data.0; edges = data.1; memberships = data.2; releases = data.3; loadedKey = key
            dataVersion += 1
        } catch { if !Task.isCancelled { self.error = error.localizedDescription } }
    }
    public func rebuildGraph() async {
        let request = UUID(); graphRequest = request; graphBusy = true
        let entries = entries, edges = edges, memberships = memberships
        let options = GraphOptions(focus: around,depth: 2,roles: roles,minShared: minShared)
        let task = Task.detached(priority: .userInitiated) { () -> RelationGraph in
            var sim = GraphSimulation(GraphLayout.buildGraph(entries,edges,memberships,options))
            for _ in 0..<400 { if Task.isCancelled || !sim.running { break }; sim.tick() }
            return sim.graph
        }
        let result = await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
        guard request == graphRequest, !Task.isCancelled else { return }; graph = result; graphBusy = false
    }
    public func moveNode(_ index: Int,x: Double,y: Double) { guard graph.nodes.indices.contains(index) else { return }; graphRequest = UUID(); graph.nodes[index].x = x; graph.nodes[index].y = y; graph.nodes[index].fx = x; graph.nodes[index].fy = y }
    public func relaxGraph() async {
        let request = UUID(); graphRequest = request; let current = graph
        let task = Task.detached(priority: .userInitiated) { () -> RelationGraph in
            var sim = GraphSimulation(current,alpha: 0.25); sim.settle(); return sim.graph
        }
        let result = await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
        if request == graphRequest && !Task.isCancelled { graph = result }
    }
    public func loadCloud(_ client: CorpusClient,scope: ActiveScope?,count: Int,hideFunctionWords: Bool,hidden: Set<String>) async {
        let request = UUID(); cloudRequest = request; cloudBusy = true
        var filter = SubcorpusScope.withScope(cloudFilter,scope)
        filter.yearFrom = cloudFilter.yearFrom ?? filter.yearFrom; filter.yearTo = cloudFilter.yearTo ?? filter.yearTo
        do {
            var rows: [CloudFrequencyRow] = []
            if cloudPos.isEmpty { rows = try await client.statsFrequency(filter: filter,limit: 800) }
            else { for pos in cloudPos { rows += try await client.statsFrequency(filter: filter,pos: pos,limit: 400) } }
            let filtered = CloudLayout.filterCloudRows(rows,hideFunctionWords: hideFunctionWords,hidden: hidden)
            let forms = (try? await client.lemmaForms(Array(Set(filtered.map(\.lemma))))) ?? []
            guard request == cloudRequest, !Task.isCancelled else { return }
            let byLemma = Dictionary(uniqueKeysWithValues: forms.map { ($0.lemma,$0) })
            words = CloudLayout.mergeByLemma(filtered,limit: count) { CloudLayout.variantKey($0,byLemma[$0]) }
            if let picked { self.picked = words.first { $0.lemma == picked.lemma } }
        } catch { if !Task.isCancelled { self.error = error.localizedDescription } }
        if request == cloudRequest { cloudBusy = false }
    }
    public func loadSongs(_ client: CorpusClient,scope: ActiveScope?) async {
        guard let word = picked else { cloudSongs = []; return }
        var q = KwicQuery(keywords: word.variants.isEmpty ? [word.lemma] : word.variants,field: .lemma)
        q.pos = word.pos; q.personIds = cloudFilter.performerIds; q.limit = 3000
        q.subcorpus = SubcorpusScope.mergeSubcorpus(Subcorpus(yearFrom: cloudFilter.yearFrom,yearTo: cloudFilter.yearTo),scope)
        do {
            let hits = try await client.kwic(q)
            guard !Task.isCancelled, picked?.lemma == word.lemma else { return }
            cloudSongs = CloudLayout.groupHitsBySong(hits); cloudTruncated = hits.count >= 3000
        } catch { if !Task.isCancelled { self.error = error.localizedDescription } }
    }
}
