import JPCorpusModel
import JPCore
import SwiftUI

/// 检索页：搜索 → KWIC → 试听 / 跳到歌 → 歌词面板跟着高亮。
///
/// - 空格：试听选中的那一句（到行尾自动停）
/// - 回车 / 双击：从这一句接着放，歌词面板滚到这一行
/// - 右键：试听、连前后文试听、查搭配
/// - 右边的搭配表点一个词：只看「附近有这个词」的行
@MainActor
struct SearchView: View {
    @Environment(AppModel.self) private var app

    private enum Mode: String { case kwic, cql, fulltext }
    @State private var mode: Mode = .kwic
    @State private var cqlError = ""
    @State private var lastSearch: KwicSearchSpec?
    @State private var searchRevision = UUID()
    @State private var resultMode: KwicResultMode = .list
    @State private var suggestions: [WordFrequency] = []
    @State private var text = ""
    @State private var field: MatchField = .lemma
    @State private var dedup = true
    @State private var jpOnly = false
    @State private var crossLine = false
    @State private var yearFrom = ""
    @State private var yearTo = ""
    @State private var performerIds: [Int64] = []
    @State private var extraScope = Subcorpus()
    @State private var requestedPos: String?
    @State private var requestSequence = 0
    @State private var near: NearFilter?
    @State private var rows: [HitRow] = []
    @State private var searched: (query: String, field: MatchField)?
    @State private var selection: Set<HitRow.ID> = []
    @State private var busy = false
    @State private var exportNote = ""
    @State private var showColloc = false
    @FocusState private var tableFocused: Bool

    struct HitRow: Identifiable, Hashable {
        let id: Int
        let hit: KwicHit
    }

    private var subcorpus: Subcorpus {
        var local = extraScope
        local.performerIds = performerIds; local.yearFrom = Int(yearFrom.trimmingCharacters(in: .whitespaces)); local.yearTo = Int(yearTo.trimmingCharacters(in: .whitespaces))
        return SubcorpusScope.mergeSubcorpus(local,app.scopes.active)
    }

    /// 搭配只对单个词有意义
    private var node: String? {
        lastSearch?.network?.collocation.node
    }

    var body: some View {
        VStack(spacing: 0) {
            Picker("检索方式", selection: Binding(get: { mode }, set: { setMode($0) })) {
                Text("检索").tag(Mode.kwic)
                Text("CQL").tag(Mode.cql)
                Text("全文").tag(Mode.fulltext)
            }
                .pickerStyle(.segmented).fixedSize().padding(10)
            if mode == .fulltext { LyricTextSearchView() } else { kwicBody }
        }
        .onChange(of: app.searchRequest) { _, request in if request != nil { setMode(.kwic) } }
        .onChange(of: text) { _, _ in cqlError = "" }
        .task(id: mode == .cql ? text : "") { await checkCql() }
    }

    private var kwicBody: some View {
        VStack(spacing: 0) {
            controls
            Divider()
            if rows.isEmpty && searched == nil {
                intro
            } else {
                HSplitView {
                    resultBody
                        .frame(minWidth: 480)
                    if showColloc, let network = lastSearch?.network, let searched {
                        CollocationPanel(
                            node: network.collocation.node,
                            field: network.collocation.field,
                            scope: network.collocation.scope,
                            selected: near?.word,
                            onPick: { lemma in
                                near = NearFilter(word: lemma)
                                Task { await run(searched.query, searched.field) }
                            }
                        )
                        .frame(minWidth: 260, idealWidth: 320, maxWidth: 420)
                    }
                }
            }
        }
        .navigationTitle("检索")
        .task(id: app.libraryURL) {
            do { suggestions = try await app.core?.wordFrequency() ?? [] }
            catch { app.errorMessage = error.localizedDescription }
        }
        .onChange(of: app.library.version) { _, _ in
            Task { await rerunLastSearch() }
        }
        .onChange(of: app.scopes.active) { _, _ in Task { await rerunLastSearch() } }
        .onChange(of: app.searchRequest) { _, request in
            guard let request else { return }
            setMode(.kwic)
            text = request.keyword
            field = request.field
            applyScope(request)
            near = nil
            app.searchRequest = nil
            Task { await run(request.keyword, request.field) }
        }
        .task {
            if let request = app.searchRequest {
                setMode(.kwic)
                text = request.keyword
                field = request.field
                applyScope(request)
                app.searchRequest = nil
                await run(request.keyword, request.field)
            }
        }
    }

    // MARK: - 条件

    private var controls: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                TextField(mode == .cql ? #"[lemma="君"] [pos="ADP"]"# : "词元 / 表层形；空格或 | 分隔多个，+ 连接多词表达（て+しまう）", text: $text)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { Task { await search() } }
                if mode == .kwic {
                    Picker("", selection: $field) {
                        Text("词元").tag(MatchField.lemma)
                        Text("表层").tag(MatchField.surface)
                    }
                    .pickerStyle(.segmented)
                    .fixedSize()
                    .help("词元把「駆ける / 駆けた / 駆けろ」归到一起；表层只按写法")
                }
                Button(busy ? "检索中…" : "检索") { Task { await search() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(busy || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || (mode == .cql && !cqlError.isEmpty))
            }
            if mode == .cql {
                if !cqlError.isEmpty {
                    Text(cqlError).font(.caption).foregroundStyle(.red).textSelection(.enabled)
                }
                CqlExamplesView { text = $0 }
            }
            HStack(spacing: 14) {
                SubcorpusPicker()
                PerformerPicker(selected: $performerIds)
                Toggle("去重", isOn: $dedup).help("同一首歌里重复的相同歌词行折叠成一条；搭配也只算第一次")
                if mode == .kwic { Toggle("仅日文", isOn: $jpOnly) }
                Toggle("跨行语境", isOn: $crossLine)
                HStack(spacing: 4) {
                    Text("年份").foregroundStyle(.secondary)
                    TextField("从", text: $yearFrom).frame(width: 52)
                    Text("–")
                    TextField("到", text: $yearTo).frame(width: 52)
                }
                .textFieldStyle(.roundedBorder)
                Spacer()
                if let near {
                    Button {
                        self.near = nil
                        if let searched { Task { await run(searched.query, searched.field) } }
                    } label: {
                        Label("附近有「\(near.word)」", systemImage: "xmark.circle.fill")
                    }
                    .buttonStyle(.borderless)
                    .help("取消附近词筛选")
                }
                if searched != nil {
                    Text(summary).foregroundStyle(.secondary).monospacedDigit()
                    Button { Task { await exportCsv() } } label: { Image(systemName: "square.and.arrow.up") }.buttonStyle(.borderless).help("导出 CSV：完整语境，UTF-8（带 BOM）").disabled(rows.isEmpty || busy)
                    Toggle(isOn: $showColloc) { Label("搭配", systemImage: "point.3.connected.trianglepath.dotted") }
                        .toggleStyle(.button)
                        .disabled(node == nil)
                        .help(node == nil ? "搭配只对单个关键词有意义" : "关键词左右 3 个词内常一起出现的词")
                }
            }
            .controlSize(.small)
            if !exportNote.isEmpty { Text(exportNote).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
        }
        .padding(12)
    }

    private var summary: String {
        let songs = Set(rows.map(\.hit.songId)).count
        return "\(rows.count) 条 · \(songs) 首"
    }

    private func exportCsv() async {
        guard let client = app.core, !rows.isEmpty else { return }
        let hits = rows.map(\.hit)
        do { if let path = try await SaveExport.text(ExportText.kwicCsv(hits), name: ExportText.kwicFilename(text), title: "保存检索结果", client: client) { exportNote = "已保存 \(hits.count) 条：\(path)" } }
        catch { app.errorMessage = error.localizedDescription }
    }

    private var intro: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("KWIC 把关键词居中对齐，一眼看出它在不同歌里怎么用。")
            Text("选中一条按空格试听那一句；回车或双击跳到那首歌从这一句接着放，右边的歌词跟着走。右键可以连前后文一起听。")
            if mode == .kwic && !suggestions.isEmpty {
                ScrollView(.horizontal) { HStack(spacing: 6) {
                    ForEach(suggestions) { suggestion in
                        Button(suggestion.lemma) { text = suggestion.lemma; Task { await run(text, .lemma) } }
                    }
                } }
            }
            Text("精度到行：时间戳来自 LRC，没有词级时间。")
                .foregroundStyle(.secondary)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    // MARK: - 结果

    private var resultBody: some View {
        VStack(spacing: 0) {
            if !rows.isEmpty, let lastSearch {
                Picker("结果视图", selection: $resultMode) {
                    ForEach(KwicResultMode.allCases, id: \.self) { value in
                        if value != .network || lastSearch.network != nil {
                            Text(value.rawValue).tag(value)
                        }
                    }
                }
                .pickerStyle(.segmented).fixedSize().padding(10)
            }
            resultContent
        }
    }

    @ViewBuilder
    private var resultContent: some View {
        if rows.isEmpty || resultMode == .list {
            results
        } else if let lastSearch {
            switch resultMode {
            case .list: results
            case .context: KwicContextBreakdownView(hits: rows.map(\.hit)).id(searchRevision)
            case .breakdown: KwicBreakdownView(search: lastSearch).id(searchRevision)
            case .network:
                if let query = lastSearch.network {
                    CollocationNetworkView(query: query) { word in
                        setMode(.kwic)
                        text = word
                        field = .lemma
                        near = nil
                        requestedPos = nil
                        resultMode = .list
                        Task { await run(word, .lemma) }
                    }
                    .id(searchRevision)
                }
            }
        }
    }

    private var results: some View {
        Table(rows, selection: $selection) {
            TableColumn("") { row in
                PlayingMark(hit: row.hit)
            }
            .width(22)
            TableColumn("左语境") { row in
                Text(row.hit.left)
                    .lineLimit(1)
                    .truncationMode(.head)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .width(min: 100, ideal: 200)
            TableColumn("关键词") { row in
                // 选中行的底色就是强调色：蓝字落在蓝底上看不见，选中时改用会跟着反白的 .primary
                Text(row.hit.keyword)
                    .fontWeight(.semibold)
                    .foregroundStyle(selection.contains(row.id) ? AnyShapeStyle(.primary) : AnyShapeStyle(.tint))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 1)
                    .background(
                        Capsule().fill(selection.contains(row.id) ? Color.clear : Color.accentColor.opacity(0.16))
                    )
            }
            .width(min: 40, ideal: 64)
            TableColumn("右语境") { row in
                Text(row.hit.right)
                    .lineLimit(1)
                    .foregroundStyle(.secondary)
            }
            .width(min: 100, ideal: 200)
            TableColumn("歌曲") { row in
                HStack(spacing: 4) {
                    Text("\(row.hit.artist) · \(row.hit.title)").lineLimit(1)
                    if row.hit.repeatCount > 1 {
                        Text("×\(row.hit.repeatCount)").foregroundStyle(.orange).help("这首歌里重复了 \(row.hit.repeatCount) 次")
                    }
                }
            }
            // 理想宽度加起来要放得进「侧栏 + 歌词面板」之间（1280 宽的窗口约 830pt），不然时间列被挤出去
            .width(min: 100, ideal: 170)
            TableColumn("时间") { row in
                Text(row.hit.timeSec.map(formatTime) ?? "—")
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .help(row.hit.endKind == .estimated ? "行尾离下一行太远（间奏），结尾按字数估" : "")
            }
            .width(min: 40, ideal: 44)
        }
        .clearScrollBackground()
        .alternatingRowBackgrounds(.disabled)
        .focused($tableFocused)
        .contextMenu(forSelectionType: HitRow.ID.self) { ids in
            if let row = rows.first(where: { ids.contains($0.id) }) {
                Button("试听这一句") { Task { await app.player.audition(row.hit.utteranceId) } }
                    .disabled(row.hit.timeSec == nil)
                Button("连前后各一行试听") { Task { await app.player.audition(row.hit.utteranceId, before: 1, after: 1) } }
                    .disabled(row.hit.timeSec == nil)
                Button("从这一句接着放") { open(row.hit) }
                Divider()
                Button("循环这一句") {
                    Task {
                        await app.player.playFrom(songId: row.hit.songId, utteranceId: row.hit.utteranceId, timeSec: row.hit.timeSec)
                        await app.player.toggleLoop(row.hit.utteranceId)
                    }
                }
                .disabled(row.hit.timeSec == nil)
            }
        } primaryAction: { ids in
            if let row = rows.first(where: { ids.contains($0.id) }) { open(row.hit) }
        }
        .onKeyPress(.space) {
            guard let id = selection.first, let row = rows.first(where: { $0.id == id }) else { return .ignored }
            Task { await app.player.audition(row.hit.utteranceId) }
            return .handled
        }
        .overlay {
            if rows.isEmpty && !busy && searched != nil {
                ContentUnavailableView.search(text: searched?.query ?? "")
            }
        }
    }

    private func open(_ hit: KwicHit) {
        app.showLyrics = true
        let queue = app.queue(songIds: rows.map(\.hit.songId))
        Task { await app.player.playFrom(songId: hit.songId, utteranceId: hit.utteranceId, timeSec: hit.timeSec, queue: queue) }
    }

    private func search() async {
        guard !busy else { return }
        near = nil
        if mode == .cql {
            guard cqlError.isEmpty else { return }
            await runCql(text)
        } else {
            await run(text, field)
        }
    }

    private func setMode(_ value: Mode) {
        guard mode != value else { return }
        requestSequence += 1
        busy = false
        cqlError = ""
        mode = value
    }

    private func checkCql() async {
        let query = text
        guard mode == .cql, !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              let client = app.core else { return }
        do {
            try await Task.sleep(for: .milliseconds(300))
            try Task.checkCancellation()
            try await client.cqlCheck(query)
            guard !Task.isCancelled, mode == .cql, text == query else { return }
            cqlError = ""
        } catch {
            guard !Task.isCancelled, mode == .cql, text == query else { return }
            cqlError = error.localizedDescription
        }
    }

    private func rerunLastSearch() async {
        guard let lastSearch else { return }
        switch lastSearch {
        case .kwic:
            if let searched { await run(searched.query, searched.field) }
        case .cql(let query): await runCql(query.query)
        }
    }

    private func runCql(_ raw: String) async {
        let text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, let client = app.core else { return }
        let query = CqlQuery(query: text, subcorpus: subcorpus, dedup: dedup,
                             crossLine: crossLine, limit: 5000)
        busy = true
        exportNote = ""
        requestSequence += 1
        let sequence = requestSequence
        defer { if sequence == requestSequence { busy = false } }
        do {
            let hits = try await client.cqlSearch(query)
            guard sequence == requestSequence else { return }
            rows = hits.enumerated().map { HitRow(id: $0.offset, hit: $0.element) }
            selection = rows.first.map { [$0.id] } ?? []
            searched = (text, .lemma)
            lastSearch = .cql(query)
            searchRevision = UUID()
            near = nil
            showColloc = false
            if resultMode == .network { resultMode = .list }
            if self.text.trimmingCharacters(in: .whitespacesAndNewlines) == text { cqlError = "" }
            tableFocused = true
        } catch {
            if sequence == requestSequence && mode == .cql && self.text.trimmingCharacters(in: .whitespacesAndNewlines) == text {
                cqlError = error.localizedDescription
            }
        }
    }

    private func run(_ raw: String, _ field: MatchField) async {
        let keywords = KwicQuery.parseKeywords(raw)
        guard !keywords.isEmpty, let client = app.client else { return }
        busy = true
        exportNote = ""
        requestSequence += 1; let sequence = requestSequence
        defer { if sequence == requestSequence { busy = false } }
        var query = KwicQuery(keywords: keywords, field: field)
        query.dedup = dedup
        query.jpOnly = jpOnly
        query.crossLine = crossLine
        query.subcorpus = subcorpus
        query.near = near
        query.pos = requestedPos
        do {
            let hits = try await client.kwic(query)
            guard sequence == requestSequence else { return }
            rows = hits.enumerated().map { HitRow(id: $0.offset, hit: $0.element) }
            selection = rows.first.map { [$0.id] } ?? []
            searched = (raw, field)
            lastSearch = .kwic(query)
            searchRevision = UUID()
            if resultMode == .network && node == nil { resultMode = .list }
            tableFocused = true
        } catch {
            if sequence == requestSequence { app.errorMessage = error.localizedDescription }
        }
    }
    private func applyScope(_ request: SearchRequest) {
        extraScope = request.scope ?? Subcorpus(); performerIds = extraScope.performerIds
        yearFrom = extraScope.yearFrom.map(String.init) ?? ""; yearTo = extraScope.yearTo.map(String.init) ?? ""; requestedPos = request.pos
    }
}

/// 正在放的那一条前面亮一个喇叭
@MainActor
private struct PlayingMark: View {
    @Environment(AppModel.self) private var app
    let hit: KwicHit

    var body: some View {
        let player = app.player
        let playing = InterfaceLogic.isPlayingSpan(currentSong: player.track?.id,playing: player.isPlaying,position: player.coarsePosition,songId: hit.songId,startSec: hit.timeSec,endSec: hit.endSec)
        Image(systemName: playing ? "speaker.wave.2.fill" : "play.fill")
            .font(.system(size: 9))
            .foregroundStyle(playing ? AnyShapeStyle(.tint) : AnyShapeStyle(.tertiary))
            .opacity(hit.timeSec == nil ? 0 : 1)
            .help(hit.timeSec == nil ? "这一行没有时间戳" : "空格试听")
    }
}

func formatTime(_ seconds: Double) -> String {
    guard seconds.isFinite, seconds >= 0 else { return "—" }
    let s = Int(seconds.rounded(.down))
    return String(format: "%d:%02d", s / 60, s % 60)
}

@MainActor
private struct LyricTextSearchView: View {
    @Environment(AppModel.self) private var app
    @State private var text = ""
    @State private var rows: [LyricHit] = []
    @State private var selection: Set<Int64> = []
    @State private var busy = false
    @State private var sequence = 0
    @State private var searched = false
    var body: some View {
        VStack {
            HStack {
                TextField("搜索歌词原文", text: $text).onSubmit { Task { await search() } }
                Button(busy ? "搜索中…" : "搜索") { Task { await search() } }.disabled(busy || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                if searched { Text("\(rows.count) 条").foregroundStyle(.secondary) }
            }.padding(12)
            Table(rows, selection: $selection) {
                TableColumn("歌词") { Text($0.text).textSelection(.enabled) }
                TableColumn("歌曲") { Text($0.artist + " · " + $0.title) }.width(min: 150, ideal: 230)
            }
            .contextMenu(forSelectionType: Int64.self) { ids in
                if let hit = rows.first(where: { ids.contains($0.id) }) {
                    Button("试听这一句") { Task { await app.player.audition(hit.utteranceId) } }.disabled(hit.timeSec == nil)
                    Button("连前后各一行试听") { Task { await app.player.audition(hit.utteranceId, before: 1, after: 1) } }.disabled(hit.timeSec == nil)
                    Button("从这一句接着放") { open(hit) }
                }
            } primaryAction: { ids in if let hit = rows.first(where: { ids.contains($0.id) }) { open(hit) } }
            .onKeyPress(.space) {
                guard let hit = rows.first(where: { selection.contains($0.id) }) else { return .ignored }
                Task { await app.player.audition(hit.utteranceId) }; return .handled
            }
        }
    }
    private func open(_ hit: LyricHit) {
        app.showLyrics = true
        let queue = app.queue(songIds: rows.map(\.songId))
        Task { await app.player.playFrom(songId: hit.songId, utteranceId: hit.utteranceId, timeSec: hit.timeSec, queue: queue) }
    }
    private func search() async {
        guard let client = app.core else { return }
        let query = text.trimmingCharacters(in: .whitespacesAndNewlines); guard !query.isEmpty else { return }
        sequence += 1; let request = sequence; busy = true
        defer { if sequence == request { busy = false } }
        do { let found = try await client.searchLyrics(query); if sequence == request { rows = found; searched = true } }
        catch { if sequence == request { app.errorMessage = error.localizedDescription } }
    }
}
