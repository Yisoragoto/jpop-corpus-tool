import JPCore
import SwiftUI
import UniformTypeIdentifiers

/// 词典页：导入 Yomitan 格式的词典包、启用 / 停用、拖动排序、删除；Anki 制卡的设置；试查一个词。
/// 词典库是语料库目录里的 dictionaries.db，和 Windows 版同一个文件、同一套导入代码（jp-dict）
@MainActor
struct DictionaryView: View {
    @Environment(AppModel.self) private var app
    @State private var dictionaries: [DictionaryInfo] = []
    @State private var picking = false
    @State private var job: JobProgress<DictImportBatch>?
    @State private var lastBatch: DictImportBatch?
    @State private var probe = ""
    @State private var probeRequest: LookupRequest?
    @State private var anki: AnkiStatus?

    @AppStorage(MineDefaults.deck) private var deck = "JPOP"
    @AppStorage(MineDefaults.model) private var model = lyricsNoteType
    @AppStorage(MineDefaults.kind) private var kind: LapisCardKind = .wordAndSentence
    @AppStorage(MineDefaults.artistSubdeck) private var artistSubdeck = false
    @AppStorage(MineDefaults.mainDictionary) private var mainDictionary = ""

    private var importing: Bool { job?.running == true }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                dictionariesSection
                probeSection
                ankiSection
            }
            .padding(24)
            .frame(maxWidth: 900, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .clearScrollBackground()
        .navigationTitle("词典")
        .task(id: app.dictionaryVersion) {
            await reload()
            if !app.lookupSeed.isEmpty {
                probe = app.lookupSeed
                app.lookupSeed = ""
                submitProbe()
            }
        }
        .task { await refreshAnki() }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.zip, .folder], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result { Task { await importDictionaries(urls) } }
        }
    }

    // MARK: - 词典

    private var dictionariesSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                SectionHeader(title: "词典", detail: dictionaries.isEmpty ? nil : "\(dictionaries.count) 本 · 拖动调整顺序，排在前面的释义先出")
                Button {
                    picking = true
                } label: {
                    Label("导入词典…", systemImage: "square.and.arrow.down")
                }
                .disabled(importing)
            }
            if let job, job.running {
                HStack(spacing: 10) {
                    if let f = job.fraction {
                        ProgressView(value: f).frame(maxWidth: 260)
                    } else {
                        ProgressView().controlSize(.small)
                    }
                    Text(job.stage).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    Button("停止") { Task { try? await app.client?.dictImportCancel() } }
                        .controlSize(.small)
                }
                Text("大词典要几分钟。导入时照样能查词、检索；停止的话正在导的那本整本不留。")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            if let batch = lastBatch {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(batch.results, id: \.path) { r in
                        if let s = r.summary {
                            Label("「\(s.title)」\(s.terms.formatted()) 条", systemImage: "checkmark.circle").foregroundStyle(.green)
                        } else {
                            Label("\(URL(fileURLWithPath: r.path).lastPathComponent)：\(r.error ?? "失败")", systemImage: "exclamationmark.triangle")
                                .foregroundStyle(.orange)
                        }
                    }
                }
                .font(.callout)
            }
            if dictionaries.isEmpty {
                Text("还没有词典。导入 Yomitan 格式的词典包（.zip，比如 JMdict、大辞林、JPDB 频率表），可以一次选好几个。也可以把 Windows 版语料库目录里的 dictionaries.db 拷过来。")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .padding(14)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .glassCard(cornerRadius: 12)
            } else {
                List {
                    ForEach(dictionaries) { d in
                        HStack(spacing: 10) {
                            Image(systemName: "line.3.horizontal").foregroundStyle(.tertiary)
                            Toggle("", isOn: Binding(get: { d.enabled }, set: { on in Task { await setEnabled(d, on) } }))
                                .labelsHidden()
                                .toggleStyle(.switch)
                                .controlSize(.small)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(d.title).foregroundStyle(d.enabled ? .primary : .secondary)
                                Text([d.summary, d.author ?? ""].filter { !$0.isEmpty }.joined(separator: " · "))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                            Spacer()
                            if d.frequencyMode != nil {
                                Text("频率").font(.caption2).padding(.horizontal, 6).padding(.vertical, 1)
                                    .background(.quaternary, in: Capsule())
                            }
                            Button(role: .destructive) {
                                Task { await delete(d) }
                            } label: {
                                Image(systemName: "trash")
                            }
                            .buttonStyle(.borderless)
                            .help("删除这本词典")
                        }
                        .padding(.vertical, 3)
                    }
                    .onMove { from, to in
                        var ids = dictionaries.map(\.id)
                        ids.move(fromOffsets: from, toOffset: to)
                        Task { await setOrder(ids) }
                    }
                }
                .listStyle(.plain)
                .clearScrollBackground()
                .frame(height: min(CGFloat(dictionaries.count) * 46 + 12, 360))
                .glassCard(cornerRadius: 12)
                .disabled(importing)
            }
        }
    }

    private var probeSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader(title: "试查", detail: "和歌词面板里右键「查词」同一套查法（会把活用形还原）")
            HStack {
                TextField("输入一个词或一段话，从开头查", text: $probe)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { submitProbe() }
                Button("查") { submitProbe() }
                    .disabled(probe.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let probeRequest {
                LookupPane(request: probeRequest) { self.probeRequest = nil }
                    .frame(height: 260)
                    .glassCard(cornerRadius: 12)
            }
        }
    }

    // MARK: - Anki

    private var ankiSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                SectionHeader(title: "Anki 制卡", detail: nil)
                Button {
                    Task { await refreshAnki() }
                } label: {
                    Label("重新连接", systemImage: "arrow.clockwise")
                }
                .controlSize(.small)
            }
            Group {
                if let anki, anki.connected {
                    Label("Anki 已连接（AnkiConnect \(anki.version)）", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                } else if let anki {
                    VStack(alignment: .leading, spacing: 4) {
                        Label("连不上 Anki", systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                        Text(anki.message.isEmpty ? "打开 Anki，并装上 AnkiConnect 插件（代码 2055492159）。" : anki.message)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                } else {
                    ProgressView().controlSize(.small)
                }
            }
            Form {
                Picker("牌组", selection: $deck) {
                    ForEach(deckChoices, id: \.self) { Text($0).tag($0) }
                }
                Toggle("按歌手放进子牌组（「\(deck)::歌手」，没有就建）", isOn: $artistSubdeck)
                Picker("笔记类型", selection: $model) {
                    ForEach(modelChoices, id: \.self) { m in
                        Text(m == lyricsNoteType ? "Lyrics（本应用的，第一次制卡时自动建）" : m).tag(m)
                    }
                }
                Picker("卡片", selection: $kind) {
                    ForEach(LapisCardKind.allCases, id: \.self) { Text($0.label).tag($0) }
                }
                Picker("主释义", selection: $mainDictionary) {
                    Text("排在最前、有释义的那本").tag("")
                    ForEach(dictionaries.filter { $0.frequencyMode == nil }) { Text($0.title).tag($0.title) }
                }
            }
            .formStyle(.grouped)
            .clearScrollBackground()
            .frame(height: 250)
            Text("在歌词面板里右键一行 →「查词」→ 词条右边点「制卡」：释义来自这里的词典，例句是那一行（同一首歌里其他含这个词的行也带上），例句音频切那一句。和 Windows 版做出来的卡一样。")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    /// 牌组选项：Anki 里有的，加上现在选着的（Anki 没开时也能看到）
    private var deckChoices: [String] {
        var decks = anki?.decks ?? []
        if !decks.contains(deck) { decks.insert(deck, at: 0) }
        return decks
    }

    private var modelChoices: [String] {
        var models = (anki?.models ?? []).filter { $0 != lyricsNoteType }
        models.insert(lyricsNoteType, at: 0)
        if !models.contains(model) { models.append(model) }
        return models
    }

    // MARK: - 动作

    private func reload() async {
        do {
            dictionaries = try await app.client?.dictionaries() ?? []
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func refreshAnki() async {
        anki = try? await app.client?.ankiStatus()
    }

    private func submitProbe() {
        let text = probe.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return }
        probeRequest = LookupRequest(text: text)
    }

    private func importDictionaries(_ urls: [URL]) async {
        guard let client = app.client, !urls.isEmpty else { return }
        lastBatch = nil
        let access = urls.map { $0.startAccessingSecurityScopedResource() }
        defer { for (url, ok) in zip(urls, access) where ok { url.stopAccessingSecurityScopedResource() } }
        do {
            try await client.dictImport(urls.map(\.path))
            while true {
                let progress = try await client.dictImportProgress()
                job = progress
                if progress.finished { break }
                try await Task.sleep(for: .milliseconds(300))
            }
            if let error = job?.error { app.errorMessage = error }
            lastBatch = job?.result
            job = nil
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func setEnabled(_ d: DictionaryInfo, _ on: Bool) async {
        do {
            try await app.client?.dictSetEnabled(d.id, on)
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func setOrder(_ ids: [Int64]) async {
        // 先在界面上挪过去，不等往返
        dictionaries = ids.compactMap { id in dictionaries.first { $0.id == id } }
        do {
            try await app.client?.dictSetOrder(ids)
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func delete(_ d: DictionaryInfo) async {
        do {
            try await app.client?.dictDelete(d.id)
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }
}
