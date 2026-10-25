import JPCore
import SwiftUI
import UniformTypeIdentifiers
import JPCorpusModel

/// 词典页：导入 Yomitan 格式的词典包、启用 / 停用、拖动排序、删除；试查一个词。
/// 词典库是语料库目录里的 dictionaries.db，和 Windows 版同一个文件、同一套导入代码（jp-dict）
@MainActor
struct DictionaryView: View {
    @Environment(AppModel.self) private var app
    @State private var dictionaries: [DictionaryInfo] = []
    @State private var picking = false
    @State private var job: JobProgress<DictImportBatch>?
    @State private var lastBatch: DictImportBatch?
    @State private var legacy: [LegacyDictionarySource] = []
    @State private var probe = ""
    @State private var probeRequest: LookupRequest?


    private var importing: Bool { job?.running == true }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                dictionariesSection
                if !legacy.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("从旧版迁移词典").font(.headline)
                        ForEach(legacy) { source in
                            HStack {
                                Text(source.name); Spacer()
                                if source.imported { Text("已导入").foregroundStyle(.secondary) }
                                else if !source.exists { Text("文件不存在，请重新选择词典包").foregroundStyle(.secondary) }
                                else { Button("导入") { Task { await importDictionaries([URL(fileURLWithPath: source.path)]) } }.disabled(importing) }
                            }
                        }
                    }
                }
                probeSection
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
        .fileImporter(isPresented: $picking, allowedContentTypes: [.zip, .folder], allowsMultipleSelection: true) { result in
            switch result {
            case .failure(let error):
                app.errorMessage = error.localizedDescription
            case .success(let urls):
                Task { await importDictionaries(urls) }
            }
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
                    Button("停止") { Task { try? await app.core?.dictImportCancel() } }
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

    // MARK: - 动作

    private func reload() async {
        do { legacy = try await app.core?.dictLegacySources() ?? [] }
        catch { app.errorMessage = error.localizedDescription }
        do {
            dictionaries = try await app.core?.dictionaries() ?? []
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }


    private func submitProbe() {
        let text = probe.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return }
        probeRequest = LookupRequest(text: text)
    }

    private func importDictionaries(_ urls: [URL]) async {
        guard let client = app.core, !urls.isEmpty else { return }
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
            try await app.core?.dictSetEnabled(d.id, on)
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func setOrder(_ ids: [Int64]) async {
        // 先在界面上挪过去，不等往返
        dictionaries = ids.compactMap { id in dictionaries.first { $0.id == id } }
        do {
            try await app.core?.dictSetOrder(ids)
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func delete(_ d: DictionaryInfo) async {
        do {
            try await app.core?.dictDelete(d.id)
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }
}
