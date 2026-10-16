import JPCore
import SwiftUI

/// 检索页：搜索 → KWIC → 试听 / 跳到歌 → 歌词面板跟着高亮。
///
/// - 空格：试听选中的那一句（到行尾自动停）
/// - 回车 / 双击：从这一句接着放，歌词面板滚到这一行
/// - 右键：试听、连前后文试听、查搭配
/// - 右边的搭配表点一个词：只看「附近有这个词」的行
struct SearchView: View {
    @Environment(AppModel.self) private var app

    @State private var text = ""
    @State private var field: MatchField = .lemma
    @State private var dedup = true
    @State private var jpOnly = false
    @State private var crossLine = false
    @State private var yearFrom = ""
    @State private var yearTo = ""
    @State private var near: NearFilter?
    @State private var rows: [HitRow] = []
    @State private var searched: (query: String, field: MatchField)?
    @State private var selection: Set<HitRow.ID> = []
    @State private var busy = false
    @State private var showColloc = false
    @FocusState private var tableFocused: Bool

    struct HitRow: Identifiable, Hashable {
        let id: Int
        let hit: KwicHit
    }

    private var subcorpus: Subcorpus {
        Subcorpus(yearFrom: Int(yearFrom.trimmingCharacters(in: .whitespaces)), yearTo: Int(yearTo.trimmingCharacters(in: .whitespaces)))
    }

    /// 搭配只对单个词有意义
    private var node: String? {
        guard let searched else { return nil }
        let words = KwicQuery.parseKeywords(searched.query)
        return words.count == 1 && !words[0].contains("+") ? words[0] : nil
    }

    var body: some View {
        VStack(spacing: 0) {
            controls
            Divider()
            if rows.isEmpty && searched == nil {
                intro
            } else {
                HSplitView {
                    results
                        .frame(minWidth: 480)
                    if showColloc, let node, let searched {
                        CollocationPanel(
                            node: node,
                            field: searched.field,
                            scope: AnalysisScope(subcorpus: subcorpus, unit: dedup ? .distinctLines : .tokens, jpOnly: jpOnly),
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
        .onChange(of: app.searchRequest) { _, request in
            guard let request else { return }
            text = request.keyword
            field = request.field
            near = nil
            app.searchRequest = nil
            Task { await run(request.keyword, request.field) }
        }
        .task {
            if let request = app.searchRequest {
                text = request.keyword
                field = request.field
                app.searchRequest = nil
                await run(request.keyword, request.field)
            }
        }
    }

    // MARK: - 条件

    private var controls: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                TextField("词元 / 表层形；空格或 | 分隔多个，+ 连接多词表达（て+しまう）", text: $text)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { near = nil; Task { await run(text, field) } }
                Picker("", selection: $field) {
                    Text("词元").tag(MatchField.lemma)
                    Text("表层").tag(MatchField.surface)
                }
                .pickerStyle(.segmented)
                .fixedSize()
                .help("词元把「駆ける / 駆けた / 駆けろ」归到一起；表层只按写法")
                Button(busy ? "检索中…" : "检索") { near = nil; Task { await run(text, field) } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(busy || text.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            HStack(spacing: 14) {
                Toggle("去重", isOn: $dedup).help("同一首歌里重复的相同歌词行折叠成一条；搭配也只算第一次")
                Toggle("仅日文", isOn: $jpOnly)
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
                    Toggle(isOn: $showColloc) { Label("搭配", systemImage: "point.3.connected.trianglepath.dotted") }
                        .toggleStyle(.button)
                        .disabled(node == nil)
                        .help(node == nil ? "搭配只对单个关键词有意义" : "关键词左右 3 个词内常一起出现的词")
                }
            }
            .controlSize(.small)
        }
        .padding(12)
    }

    private var summary: String {
        let songs = Set(rows.map(\.hit.songId)).count
        return "\(rows.count) 条 · \(songs) 首"
    }

    private var intro: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("KWIC 把关键词居中对齐，一眼看出它在不同歌里怎么用。")
            Text("选中一条按空格试听那一句；回车或双击跳到那首歌从这一句接着放，右边的歌词跟着走。右键可以连前后文一起听。")
            Text("精度到行：时间戳来自 LRC，没有词级时间。")
                .foregroundStyle(.secondary)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    // MARK: - 结果

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
            .width(min: 120, ideal: 260)
            TableColumn("关键词") { row in
                Text(row.hit.keyword)
                    .fontWeight(.semibold)
                    .foregroundStyle(.tint)
            }
            .width(min: 40, ideal: 80)
            TableColumn("右语境") { row in
                Text(row.hit.right)
                    .lineLimit(1)
                    .foregroundStyle(.secondary)
            }
            .width(min: 120, ideal: 260)
            TableColumn("歌曲") { row in
                HStack(spacing: 4) {
                    Text("\(row.hit.artist) · \(row.hit.title)").lineLimit(1)
                    if row.hit.repeatCount > 1 {
                        Text("×\(row.hit.repeatCount)").foregroundStyle(.orange).help("这首歌里重复了 \(row.hit.repeatCount) 次")
                    }
                }
            }
            .width(min: 120, ideal: 220)
            TableColumn("时间") { row in
                Text(row.hit.timeSec.map(formatTime) ?? "—")
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .help(row.hit.endKind == .estimated ? "行尾离下一行太远（间奏），结尾按字数估" : "")
            }
            .width(48)
        }
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
        Task { await app.player.playFrom(songId: hit.songId, utteranceId: hit.utteranceId, timeSec: hit.timeSec) }
    }

    private func run(_ raw: String, _ field: MatchField) async {
        let keywords = KwicQuery.parseKeywords(raw)
        guard !keywords.isEmpty, let client = app.client else { return }
        busy = true
        defer { busy = false }
        var query = KwicQuery(keywords: keywords, field: field)
        query.dedup = dedup
        query.jpOnly = jpOnly
        query.crossLine = crossLine
        query.subcorpus = subcorpus
        query.near = near
        do {
            let hits = try await client.kwic(query)
            rows = hits.enumerated().map { HitRow(id: $0.offset, hit: $0.element) }
            selection = rows.first.map { [$0.id] } ?? []
            searched = (raw, field)
            tableFocused = true
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }
}

/// 正在放的那一条前面亮一个喇叭
private struct PlayingMark: View {
    @Environment(AppModel.self) private var app
    let hit: KwicHit

    var body: some View {
        let player = app.player
        let playing = player.isPlaying && player.track?.id == hit.songId && {
            guard let start = hit.timeSec else { return false }
            let end = hit.endSec ?? start + 5
            return player.position >= start - 0.5 && player.position < end
        }()
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
