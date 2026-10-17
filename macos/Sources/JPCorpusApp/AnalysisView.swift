import JPCore
import SwiftUI

/// 分析：重复感知的词频、子语料对比、分析存档（重跑 / 对比）、语料质量。
/// 每个词都能双击回到检索页看具体的句子、再去听。
@MainActor
struct AnalysisView: View {
    @Environment(AppModel.self) private var app
    @AppStorage("analysisTab") private var tab = 0
    @State private var yearFrom = ""
    @State private var yearTo = ""

    /// 顶上那条范围：歌手（从歌手页带过来）+ 年份
    private var scope: Subcorpus {
        Subcorpus(performerIds: app.analysisScope?.performerIds ?? [], yearFrom: Int(yearFrom), yearTo: Int(yearTo))
    }

    private var scopeName: String {
        var parts: [String] = []
        if let s = app.analysisScope { parts.append(s.name) }
        switch (Int(yearFrom), Int(yearTo)) {
        case let (f?, t?): parts.append("\(f)–\(t)")
        case let (f?, nil): parts.append("\(f) 起")
        case let (nil, t?): parts.append("到 \(t)")
        default: break
        }
        return parts.isEmpty ? "全库" : parts.joined(separator: " · ")
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 14) {
                Picker("", selection: $tab) {
                    Text("词频").tag(0)
                    Text("子语料对比").tag(1)
                    Text("存档").tag(2)
                    Text("语料质量").tag(3)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                if tab < 2 { scopeBar }
                Spacer()
            }
            .padding(10)
            Divider()
            switch tab {
            case 0: FrequencyPane(scope: scope, scopeName: scopeName)
            case 1: KeynessPane(scope: scope, scopeName: scopeName)
            case 2: ArchivePane()
            default: QualityPane()
            }
        }
        .navigationTitle("分析")
    }

    private var scopeBar: some View {
        HStack(spacing: 6) {
            Text("范围").foregroundStyle(.secondary)
            if let s = app.analysisScope {
                HStack(spacing: 4) {
                    Image(systemName: "person.fill").font(.caption)
                    Text(s.name)
                    Button {
                        app.analysisScope = nil
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                    }
                    .buttonStyle(.borderless)
                    .help("不再限定这位歌手")
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 3)
                .background(.tint.opacity(0.15), in: Capsule())
            }
            TextField("年份", text: $yearFrom).frame(width: 52)
            Text("–")
            TextField("年份", text: $yearTo).frame(width: 52)
        }
        .textFieldStyle(.roundedBorder)
        .controlSize(.small)
    }
}

// MARK: - 词频

@MainActor
private struct FrequencyPane: View {
    @Environment(AppModel.self) private var app
    let scope: Subcorpus
    let scopeName: String
    @State private var unit: CountUnit = .distinctLines
    @State private var pos = "NOUN"
    @State private var jpOnly = true
    @State private var result: FrequencyResult?
    @State private var selection: Set<FrequencyItem.ID> = []
    @State private var archived: String?

    private let posChoices = ["", "NOUN", "VERB", "ADJ", "ADV", "PRON", "PROPN", "INTJ"]

    private var query: FrequencyQuery {
        FrequencyQuery(scope: AnalysisScope(subcorpus: scope, unit: unit, jpOnly: jpOnly), pos: pos.isEmpty ? nil : pos, limit: 500)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 14) {
                Picker("计数", selection: $unit) {
                    Text("去掉重复行").tag(CountUnit.distinctLines)
                    Text("全部词次").tag(CountUnit.tokens)
                }
                .pickerStyle(.segmented)
                .fixedSize()
                .help("副歌会原样唱好几遍。去掉重复行 = 同一首歌里相同的行只算第一次")
                Picker("词性", selection: $pos) {
                    ForEach(posChoices, id: \.self) { p in
                        Text(p.isEmpty ? "全部" : (posLabels[p] ?? p)).tag(p)
                    }
                }
                .fixedSize()
                Toggle("仅日文", isOn: $jpOnly)
                Spacer()
                ArchiveButton(archived: $archived) {
                    let label = "词频 · \(scopeName) · \(pos.isEmpty ? "全部词性" : (posLabels[pos] ?? pos))"
                    return try await app.client?.archiveFrequency(query, label: label)
                }
            }
            .controlSize(.small)
            .padding(10)

            Table(result?.items ?? [], selection: $selection) {
                TableColumn("词元") { Text($0.lemma) }
                TableColumn("词性") { Text(posLabels[$0.pos] ?? $0.pos).foregroundStyle(.secondary) }.width(60)
                TableColumn("频次") { Text("\($0.freq)").monospacedDigit() }.width(56)
                TableColumn("每万词") { Text(String(format: "%.1f", $0.per10k)).monospacedDigit() }.width(60)
                TableColumn("全部 / 去重") { Text("\($0.rawFreq) / \($0.distinctFreq)").monospacedDigit().foregroundStyle(.secondary) }.width(90)
                TableColumn("来自重复行") { item in
                    Text(String(format: "%.0f%%", item.repeatShare * 100))
                        .monospacedDigit()
                        .foregroundStyle(item.repeatShare >= 0.4 ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
                }
                .width(80)
                TableColumn("曲数") { Text("\($0.songs)").monospacedDigit() }.width(48)
                TableColumn("写法") { Text($0.surfaces.joined(separator: "、")).foregroundStyle(.secondary).lineLimit(1) }
            }
            .clearScrollBackground()
            .alternatingRowBackgrounds(.disabled)
            .contextMenu(forSelectionType: FrequencyItem.ID.self) { ids in
                if let item = result?.items.first(where: { ids.contains($0.id) }) {
                    Button("检索「\(item.lemma)」") { app.search(item.lemma) }
                }
            } primaryAction: { ids in
                if let item = result?.items.first(where: { ids.contains($0.id) }) { app.search(item.lemma) }
            }

            if let p = result?.provenance {
                ProvenanceFooter(p: p)
            }
        }
        .task(id: query) {
            archived = nil
            do {
                result = try await app.client?.frequency(query)
            } catch {
                app.errorMessage = error.localizedDescription
            }
        }
    }
}

/// 「存档」按钮：把当前这次分析连同范围、参数、库指纹存进 analysis_runs，之后能重跑对比
@MainActor
private struct ArchiveButton: View {
    @Environment(AppModel.self) private var app
    @Binding var archived: String?
    let save: @MainActor () async throws -> AnalysisRunRef?

    var body: some View {
        HStack(spacing: 6) {
            if let archived {
                Text(archived).font(.caption).foregroundStyle(.secondary)
            }
            Button {
                Task {
                    do {
                        if let run = try await save() { archived = "已存档 #\(run.id)" }
                    } catch {
                        app.errorMessage = error.localizedDescription
                    }
                }
            } label: {
                Label("存档", systemImage: "archivebox")
            }
            .help("把这次分析的范围、参数和结果存下来。之后库变了可以在「存档」里重跑，看结果为什么变")
        }
    }
}

// MARK: - 子语料对比

@MainActor
private struct KeynessPane: View {
    @Environment(AppModel.self) private var app
    let scope: Subcorpus
    let scopeName: String
    @State private var refFrom = ""
    @State private var refTo = ""
    @State private var unit: CountUnit = .distinctLines
    @State private var result: KeynessResult?
    @State private var busy = false
    @State private var archived: String?

    private var query: KeynessQuery? {
        guard !scope.isAll else { return nil }
        let reference = Subcorpus(yearFrom: Int(refFrom), yearTo: Int(refTo))
        var q = KeynessQuery(target: scope, reference: reference.isAll ? nil : reference)
        q.unit = unit
        return q
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 16) {
                Text("目标：\(scopeName)").fontWeight(.medium)
                HStack(spacing: 4) {
                    Text("参照").foregroundStyle(.secondary)
                    TextField("年份", text: $refFrom).frame(width: 52)
                    Text("–")
                    TextField("年份", text: $refTo).frame(width: 52)
                }
                .textFieldStyle(.roundedBorder)
                Picker("", selection: $unit) {
                    Text("去掉重复行").tag(CountUnit.distinctLines)
                    Text("全部词次").tag(CountUnit.tokens)
                }
                .pickerStyle(.segmented)
                .fixedSize()
                Button(busy ? "计算中…" : "对比") { Task { await run() } }
                    .disabled(busy || query == nil)
                    .keyboardShortcut(.defaultAction)
                Spacer()
                if result != nil, let q = query {
                    ArchiveButton(archived: $archived) {
                        try await app.client?.archiveKeyness(q, label: "对比 · \(scopeName)")
                    }
                }
            }
            .controlSize(.small)
            Text(scope.isAll
                 ? "目标是上面那条「范围」：填一个年份段，或者从歌手页点「分析这个人的歌」带一位歌手过来。"
                 : "参照留空 = 语料库里其余的歌。按对数似然排序，两边合计少于 5 次的词不列出。双击一个词去检索。")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let result {
                HStack(alignment: .top, spacing: 16) {
                    keyList("目标里偏多", result.over)
                    keyList("目标里偏少", result.under)
                }
                ProvenanceFooter(p: result.target, label: "目标")
                ProvenanceFooter(p: result.reference, label: "参照")
            } else {
                Spacer()
            }
        }
        .padding(12)
        .onChange(of: scope) { _, _ in
            result = nil
            archived = nil
        }
    }

    private func keyList(_ title: String, _ items: [KeyItem]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.headline)
            List(items) { k in
                HStack {
                    Text(k.lemma)
                    Text(posLabels[k.pos] ?? k.pos).font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Text(String(format: "%.1f vs %.1f /万词 · LL %.1f", k.targetPer10k, k.referencePer10k, abs(k.logLikelihood)))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                .contentShape(Rectangle())
                .onTapGesture(count: 2) { app.search(k.lemma) }
            }
            .listStyle(.plain)
            .clearScrollBackground()
            .glassCard(cornerRadius: 12)
        }
    }

    private func run() async {
        guard let query else { return }
        busy = true
        defer { busy = false }
        archived = nil
        do {
            result = try await app.client?.keyness(query)
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }
}

// MARK: - 存档

@MainActor
private struct ArchivePane: View {
    @Environment(AppModel.self) private var app
    @State private var runs: [AnalysisSummary] = []
    @State private var selection: Set<AnalysisSummary.ID> = []
    @State private var comparison: Comparison?
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("每条存档记着当时的范围（精确到每首歌的歌词版本）、分词器、参数和库指纹。库变了之后重跑，会逐项说明结果为什么不同。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button("重跑并对比") { Task { await rerun() } }
                    .disabled(selection.count != 1 || busy)
                Button("对比两条") { Task { await compare() } }
                    .disabled(selection.count != 2 || busy)
                    .help("选中两条（⌘ 点击）")
            }
            .controlSize(.small)
            .padding(10)
            Table(runs, selection: $selection) {
                TableColumn("#") { Text("\($0.id)").monospacedDigit().foregroundStyle(.secondary) }.width(36)
                TableColumn("种类") { Text($0.kindLabel) }.width(110)
                TableColumn("标签") { r in
                    HStack(spacing: 4) {
                        if r.rerunOf != nil { Image(systemName: "arrow.clockwise").font(.caption).foregroundStyle(.secondary) }
                        Text(r.label.isEmpty ? "—" : r.label)
                    }
                }
                TableColumn("范围") { Text($0.filter.summary).foregroundStyle(.secondary) }
                TableColumn("曲数") { Text("\($0.songCount)").monospacedDigit() }.width(44)
                TableColumn("结果") { Text(String($0.resultHash.prefix(8))).font(.caption.monospaced()).foregroundStyle(.secondary) }.width(72)
                TableColumn("时间") { Text(relativeTime($0.createdAt)).foregroundStyle(.secondary) }.width(90)
                TableColumn("程序") { Text($0.appVersion).font(.caption).foregroundStyle(.tertiary) }.width(90)
            }
            .clearScrollBackground()
            .alternatingRowBackgrounds(.disabled)
            .contextMenu(forSelectionType: AnalysisSummary.ID.self) { ids in
                if ids.count == 1, let id = ids.first {
                    Button("重跑并对比") { Task { await rerun(id) } }
                    Divider()
                    Button("删除", role: .destructive) { Task { await delete(id) } }
                }
            }
            .overlay {
                if runs.isEmpty {
                    ContentUnavailableView("还没有存档", systemImage: "archivebox", description: Text("在「词频」「子语料对比」里点「存档」。Windows 版存的也在这里。"))
                }
            }
        }
        .task { await load() }
        .sheet(item: Binding(get: { comparison.map(IdentifiedComparison.init) }, set: { if $0 == nil { comparison = nil } })) { c in
            ComparisonView(c: c.value)
        }
    }

    private func load() async {
        do {
            runs = try await app.client?.analysisList() ?? []
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func rerun(_ id: Int64? = nil) async {
        guard let id = id ?? selection.first, let client = app.client else { return }
        busy = true
        defer { busy = false }
        do {
            comparison = try await client.analysisRerun(id)
            await load()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func compare() async {
        let ids = selection.sorted()
        guard ids.count == 2, let client = app.client else { return }
        do {
            comparison = try await client.analysisCompare(before: ids[0], after: ids[1])
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func delete(_ id: Int64) async {
        do {
            try await app.client?.analysisDelete(id)
            selection.remove(id)
            await load()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }
}

private struct IdentifiedComparison: Identifiable {
    let value: Comparison
    var id: String { "\(value.before.id)-\(value.after.id)" }
}

/// 两次分析的差别：先给结论和原因，再列范围、指标、词频的变化
@MainActor
private struct ComparisonView: View {
    @Environment(\.dismiss) private var dismiss
    let c: Comparison

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Image(systemName: c.resultIdentical ? "checkmark.seal.fill" : "exclamationmark.triangle.fill")
                    .foregroundStyle(c.resultIdentical ? .green : .orange)
                    .font(.title2)
                VStack(alignment: .leading) {
                    Text(c.resultIdentical ? "结果完全相同" : "结果不一样").font(.title3.weight(.semibold))
                    Text("#\(c.before.id)（\(relativeTime(c.before.createdAt))）→ #\(c.after.id)（\(relativeTime(c.after.createdAt))）")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("完成") { dismiss() }.keyboardShortcut(.defaultAction)
            }
            if !c.reasons.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(c.reasons, id: \.self) { Text("· \($0)") }
                }
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .glassCard(cornerRadius: 10)
            }
            if !(c.scope.added.isEmpty && c.scope.removed.isEmpty && c.scope.lyricsChanged.isEmpty) {
                Text("范围：新增 \(c.scope.added.count) 首，移出 \(c.scope.removed.count) 首，换了歌词 \(c.scope.lyricsChanged.count) 首")
                    .font(.callout)
            }
            if !c.metrics.isEmpty {
                Table(c.metrics) {
                    TableColumn("指标") { Text($0.name) }
                    TableColumn("之前") { Text($0.before.map { String(format: "%.4g", $0) } ?? "—").monospacedDigit() }
                    TableColumn("之后") { Text($0.after.map { String(format: "%.4g", $0) } ?? "—").monospacedDigit() }
                }
                .frame(height: 140)
            }
            if !c.words.isEmpty {
                Text("词频有变化的词（\(c.words.count)）").font(.headline)
                Table(c.words) {
                    TableColumn("词元") { Text($0.lemma) }
                    TableColumn("词性") { Text(posLabels[$0.pos] ?? $0.pos).foregroundStyle(.secondary) }.width(60)
                    TableColumn("名次") { w in Text("\(w.beforeRank.map(String.init) ?? "—") → \(w.afterRank.map(String.init) ?? "—")").monospacedDigit() }
                    TableColumn("频次") { w in Text("\(w.beforeFreq.map(String.init) ?? "—") → \(w.afterFreq.map(String.init) ?? "—")").monospacedDigit() }
                }
            }
        }
        .padding(20)
        .frame(width: 620, height: c.words.isEmpty && c.metrics.isEmpty ? 260 : 560)
    }
}

// MARK: - 语料质量

@MainActor
private struct QualityPane: View {
    @Environment(AppModel.self) private var app
    @State private var q: QualityOverview?

    var body: some View {
        ScrollView {
            if let q {
                VStack(alignment: .leading, spacing: 18) {
                    HStack(spacing: 28) {
                        StatBlock(value: "\(q.songsWithLyrics) / \(q.songs)", label: "首有歌词")
                        StatBlock(value: q.avgCompleteness.map { "\(Int($0 * 100))%" } ?? "—", label: "平均完整度")
                        StatBlock(value: grouped(q.linesLowTimeConfidence), label: "行时间轴存疑")
                        StatBlock(value: grouped(q.legacyVersions), label: "首没有来源记录（老库）")
                    }
                    HStack(alignment: .top, spacing: 16) {
                        counts("歌词来源", q.bySource)
                        counts("语言", q.byLanguage)
                        counts("问题", q.issues)
                        counts("分词状态", q.tokenStatus)
                    }
                    if !q.worst.isEmpty {
                        SectionHeader(title: "最需要看一眼的歌", detail: "点一首去详情")
                        VStack(spacing: 0) {
                            ForEach(q.worst) { s in
                                Button {
                                    app.show(.song(s.songId))
                                } label: {
                                    HStack {
                                        VStack(alignment: .leading, spacing: 1) {
                                            Text(s.title).lineLimit(1)
                                            Text(s.artist).font(.caption).foregroundStyle(.secondary)
                                        }
                                        Spacer()
                                        Text(s.codes.joined(separator: " ")).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(1)
                                        if s.errors > 0 { Text("\(s.errors) 错").foregroundStyle(.red).font(.caption) }
                                        if s.warnings > 0 { Text("\(s.warnings) 警告").foregroundStyle(.orange).font(.caption) }
                                        Text(s.completeness.map { "\(Int($0 * 100))%" } ?? "—").monospacedDigit().frame(width: 44, alignment: .trailing)
                                    }
                                    .padding(.horizontal, 12)
                                    .padding(.vertical, 7)
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                if s.id != q.worst.last?.id { Divider() }
                            }
                        }
                        .glassCard(cornerRadius: 12)
                    }
                }
                .padding(20)
            } else {
                ProgressView().frame(maxWidth: .infinity, minHeight: 200)
            }
        }
        .clearScrollBackground()
        .task {
            do {
                q = try await app.client?.qualityOverview()
            } catch {
                app.errorMessage = error.localizedDescription
            }
        }
    }

    private func counts(_ title: String, _ rows: [LabelCount]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.headline)
            if rows.isEmpty {
                Text("—").foregroundStyle(.secondary)
            }
            ForEach(rows, id: \.label) { r in
                HStack {
                    Text(r.label.isEmpty ? "（空）" : r.label).lineLimit(1)
                    Spacer()
                    Text(grouped(r.count)).monospacedDigit().foregroundStyle(.secondary)
                }
                .font(.callout)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .glassCard(cornerRadius: 12)
    }
}

/// 结果的出处：哪些歌、多少词、怎么数、库指纹
@MainActor
struct ProvenanceFooter: View {
    let p: Provenance
    var label: String?

    var body: some View {
        Text("\(label.map { "\($0)：" } ?? "")\(p.scope) · \(p.songs) 首 / \(p.tokens) 词（\(p.unit == .distinctLines ? "重复行只算一次" : "全部词次")）· 规则 \(p.rules) · 指纹 \(String(p.snapshot.fingerprint.prefix(8)))")
            .font(.caption2)
            .foregroundStyle(.tertiary)
            .textSelection(.enabled)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
    }
}
