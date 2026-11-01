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
    @ObservationIgnored private var liveGraph: GraphSimulation?
    @ObservationIgnored private var liveTask: Task<Void, Never>?
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
        stopLiveGraph()
        let entries = entries, edges = edges, memberships = memberships
        let options = GraphOptions(focus: around,depth: 2,roles: roles,minShared: minShared)
        let task = Task.detached(priority: .userInitiated) { () -> RelationGraph in
            var sim = GraphSimulation(GraphLayout.buildGraph(entries,edges,memberships,options),forces: .roomy)
            for _ in 0..<400 { if Task.isCancelled || !sim.running { break }; sim.tick() }
            return sim.graph
        }
        let result = await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
        guard request == graphRequest, !Task.isCancelled else { return }; graph = result; graphBusy = false
    }
    /// 拖一个人：钉在手指下，整张图边拖边演算（相连的人被拉着走、挨着的被推开），
    /// 而不是只挪这一个点、松手后整张图一下跳到算好的位置
    public func dragNode(_ index: Int,x: Double,y: Double) {
        guard graph.nodes.indices.contains(index) else { return }
        if liveGraph == nil {
            graphRequest = UUID()
            liveGraph = GraphSimulation(graph,forces: .roomy,alpha: 0)
        }
        liveGraph?.graph.nodes[index].x = x; liveGraph?.graph.nodes[index].y = y
        liveGraph?.graph.nodes[index].fx = x; liveGraph?.graph.nodes[index].fy = y
        liveGraph?.reheat(0.3)
        if let sim = liveGraph { graph = sim.graph }
        if liveTask == nil { liveTask = Task { @MainActor [weak self] in await self?.runLiveGraph() } }
    }

    /// 松手：放下的人留在原地，其余的继续动，慢慢停稳
    public func endDrag() { liveGraph?.reheat(0.12) }

    private func runLiveGraph() async {
        while !Task.isCancelled, var sim = liveGraph, sim.running {
            sim.tick(); liveGraph = sim; graph = sim.graph
            try? await Task.sleep(for: .milliseconds(16))
        }
        // 被 stopLiveGraph 取消时那边已经清过了，这里再清会把新开的那一轮也清掉
        if !Task.isCancelled { liveGraph = nil; liveTask = nil }
    }

    private func stopLiveGraph() { liveTask?.cancel(); liveTask = nil; liveGraph = nil }
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
