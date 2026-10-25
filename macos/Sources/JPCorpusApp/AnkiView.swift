import AppKit
import JPCore
import JPCorpusModel
import SwiftUI
import WebKit

@MainActor
struct AnkiView: View {
    @Environment(AppModel.self) private var app
    private var model: AnkiModel { app.anki }
    var body: some View {
        @Bindable var model = model
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                Text("把语料里的词做成卡片。例句取自你自己听的歌。").foregroundStyle(.secondary)
                connection
                MineSettingsView(status: model.status).padding(20).glassCard()
                miningReport
                wordPicker
                if let card = model.preview { preview(card) }
                exportControls
                refreshControls
            }.padding(24).frame(maxWidth: 1000, alignment: .leading).frame(maxWidth: .infinity)
        }
        .navigationTitle("Anki")
        .task(id: app.libraryURL?.path) { if let client = app.core { await model.load(client) } }
        .task(id: "\(model.pos)|\(model.skipStudied)") { if let client = app.core { await model.loadWords(client) } }
        .task(id: model.running) {
            guard let client = app.core else { return }
            while model.running && !Task.isCancelled {
                await model.poll(client)
                if model.running { do { try await Task.sleep(nanoseconds: 300_000_000) } catch { return } }
            }
        }
        .onChange(of: model.error) { _, error in if let error { app.errorMessage = error; model.error = nil } }
    }
    private var connection: some View {
        @Bindable var model = model
        return VStack(alignment: .leading, spacing: 14) {
            HStack { Text("连接").font(.title2.bold()); Spacer(); Text(model.status?.connected == true ? "AnkiConnect v\(model.status?.version ?? 0)" : "未连接").foregroundStyle(.secondary) }
            if let status = model.status {
                HStack(spacing: 36) {
                    StatBlock(value: "\(status.decks.count)", label: "牌组")
                    StatBlock(value: "\(status.knownWords)", label: "已进 Anki 的词")
                    StatBlock(value: "\(status.studiedWords)", label: "其中复习过")
                }
                if !status.connected {
                    Text(status.message).font(.callout).foregroundStyle(.secondary)
                    Text("选词和预览不需要 Anki——那些只读本地文件。只有真正推卡片才需要它开着。").font(.caption).foregroundStyle(.secondary)
                }
                if !status.collectionPath.isEmpty { Text("学习状态读自 \(status.collectionPath)").font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                if status.connected {
                    if !status.noteTypeReady { Text("还没有「JPOP Corpus」笔记类型，第一次导出时会自动建。").font(.caption).foregroundStyle(.secondary) }
                    Picker("牌组", selection: $model.options.deck) { ForEach(status.decks, id: \.self) { Text($0).tag($0) } }
                }
            } else { ProgressView() }
        }.padding(20).glassCard()
    }
    private var miningReport: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("挖词报告").font(.title2.bold())
            Text("按歌手、歌曲和 JLPT 统计已经挖进 Anki 的词（JPOP Corpus 和 Lyrics 两种卡）和复习进度，生成一个能搜索、能排序的网页，用浏览器打开。只读 Anki 的库，不需要 Anki 开着。").font(.callout).foregroundStyle(.secondary)
            Button { if let c = app.core { Task { await model.generateReport(c) } } } label: { Label(model.reporting ? "生成中…" : "生成挖词报告", systemImage: "doc.text") }.buttonStyle(.borderless).disabled(model.reporting)
            if let r = model.report {
                Text(r.noteTypes.joined(separator: "、")).font(.caption).foregroundStyle(.secondary)
                Text("\(r.cards) 张卡（\(r.studied) 张复习过）· \(r.words) 个词 · \(r.artists) 位歌手 · \(r.songs) 首歌")
                Text("\(r.openError.isEmpty ? "已在浏览器打开：" : "没能自动打开浏览器，文件在：")\(r.path)").font(.caption).textSelection(.enabled)
                if !r.openError.isEmpty { Text(r.openError).foregroundStyle(.orange) }
            }
        }.padding(20).glassCard()
    }
    private var wordPicker: some View {
        @Bindable var model = model
        return VStack(alignment: .leading, spacing: 14) {
            HStack { Text("选词").font(.title2.bold()); Spacer(); Text("\(model.picked.count) / \(model.words.count) 已选").foregroundStyle(.secondary) }
            HStack {
                ForEach(AnkiLogic.posChoices, id: \.self) { pos in
                    Toggle(posLabels[pos] ?? pos, isOn: Binding(get: { model.pos.contains(pos) }, set: { on in if on { model.pos.append(pos) } else { model.pos.removeAll { $0 == pos } } })).toggleStyle(.button)
                }
                Toggle("藏起已学过的", isOn: $model.skipStudied)
            }.controlSize(.small)
            ScrollView {
                LazyVStack(spacing: 10) {
                    ForEach(model.words) { word in
                        HStack {
                            Toggle(word.lemma, isOn: Binding(get: { model.picked.contains(word.lemma) }, set: { _ in model.picked = AnkiLogic.toggle(model.picked, lemma: word.lemma) })).toggleStyle(.checkbox).font(.headline)
                            Text("\(posLabels[word.pos] ?? word.pos) · \(word.count) 次 · \(word.songCount) 首\(word.studied ? " · 已学过" : word.inAnki ? " · 已在 Anki" : "")").font(.caption).foregroundStyle(.secondary)
                            Spacer()
                            Button { if let c = app.core { Task { await model.showPreview(word, client: c) } } } label: { Image(systemName: "eye") }.buttonStyle(.borderless).help("预览")
                        }
                    }
                    if model.words.isEmpty { Text("没有符合条件的词。").foregroundStyle(.secondary) }
                }.padding(4)
            }.frame(height: 280)
            HStack {
                Button { model.picked = Set(model.words.map(\.lemma)) } label: { Label("全选", systemImage: "checkmark") }.buttonStyle(.borderless)
                Button { model.picked = [] } label: { Label("清空", systemImage: "xmark") }.buttonStyle(.borderless)
                Stepper("每张卡例句 \(model.options.maxExamples)", value: $model.options.maxExamples, in: 1...8)
                Stepper("释义取前几部词典 \(model.options.maxDicts ?? 3)", value: Binding(get: { model.options.maxDicts ?? 3 }, set: { model.options.maxDicts = $0 }), in: 0...20)
            }.controlSize(.small)
            Text("取前几部就够——顺序就是你在词典管理里排的优先级。").font(.caption).foregroundStyle(.secondary)
        }.padding(20).glassCard()
    }
    private func preview(_ card: AnkiCard) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack { Text("预览 · \(card.expression)").font(.title2.bold()); Spacer(); Button { model.preview = nil } label: { Image(systemName: "xmark") }.buttonStyle(.borderless).help("收起") }
            Text([card.reading, card.partOfSpeech, card.jlpt, card.pitch, card.freq.isEmpty ? "" : "JPDB \(card.freq)"].filter { !$0.isEmpty }.joined(separator: " · ")).foregroundStyle(.secondary)
            if card.noExamples { Text("语料里没有这个词的例句，这张卡做出来没法用。").foregroundStyle(.orange) }
            else { AnkiHTML(html: card.sentence).frame(height: 180) }
            if card.noDefinitions { Text("本地词典里查不到释义。").foregroundStyle(.secondary) }
            else { DisclosureGroup("释义（\(card.meaning.utf16.count) 字符）") { AnkiHTML(html: card.meaning).frame(height: 260) } }
        }.padding(20).glassCard()
    }
    private var exportControls: some View {
        @Bindable var model = model
        return VStack(alignment: .leading, spacing: 14) {
            Text("导出").font(.title2.bold())
            HStack {
                Toggle("音频片段", isOn: $model.options.clipAudio)
                Picker("已有这个词时", selection: $model.options.dupMode) { Text("追加新例句").tag(AnkiDupMode.append); Text("跳过").tag(AnkiDupMode.skip) }
                Picker("查重范围", selection: $model.options.dupScope) { Text("当前牌组").tag(AnkiDupScope.deck); Text("主牌组及子牌组").tag(AnkiDupScope.root) }
            }
            if model.running, let p = model.progress { ProgressView(value: AnkiLogic.percent(p), total: 100); Text("\(p.done)/\(p.total) · \(p.lemma)").font(.caption) }
            if let p = model.progress, p.finished {
                Text((p.cancelled ? "已中断。" : "跑完了。") + completionSummary).font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Button { if let c = app.core { Task { await model.start(c) } } } label: { Label("导出 \(model.picked.count) 张", systemImage: "square.and.arrow.up") }.disabled(model.busy || model.running || model.picked.isEmpty || model.status?.connected != true)
                Button { if let c = app.core { Task { await model.start(c, update: true) } } } label: { Label("更新选中的 \(model.picked.count) 张", systemImage: "arrow.clockwise") }.disabled(model.busy || model.running || model.picked.isEmpty || model.status?.connected != true).help("只重查词典字段，不动例句、音频和出处")
                Button { if let c = app.core { Task { await model.cancel(c) } } } label: { Label("中断", systemImage: "stop") }.disabled(!model.running)
            }.buttonStyle(.borderless)
            if model.status?.connected != true { Text("Anki 没开，导出按钮不可用。").font(.caption).foregroundStyle(.secondary) }
            ForEach(Array(model.log.prefix(30).enumerated()), id: \.offset) { _, entry in
                Text("\(entry.lemma) · \(AnkiLogic.outcomeLabels[entry.outcome] ?? entry.outcome)\(entry.outcome == "updated" ? "（+\(entry.newSentences) 句）" : "")\(entry.message.isEmpty ? "" : " · \(entry.message)")").font(.caption).foregroundStyle(.secondary)
            }
        }.padding(20).glassCard()
    }
    private var completionSummary: String {
        let counts = AnkiLogic.counts(model.log)
        var seen: Set<String> = []
        let order = model.log.compactMap { seen.insert($0.outcome).inserted ? $0.outcome : nil }
        return order.map { "\(AnkiLogic.outcomeLabels[$0] ?? $0) \(counts[$0] ?? 0)" }.joined(separator: "，")
    }
    private var refreshControls: some View {
        @Bindable var model = model
        return VStack(alignment: .leading, spacing: 14) {
            Text("刷新旧牌组").font(.title2.bold())
            Text(model.options.deck.isEmpty ? "先在上面选牌组" : model.options.deck).foregroundStyle(.secondary)
            Text("按现在的词典重写范围内所有 JPOP Corpus 卡片的读音、释义、JLPT、音高、词频、词性，顺带更新卡片模板。不动例句、音频和出处。释义不按上面的「取前几部词典」截断。").font(.callout).foregroundStyle(.secondary)
            HStack {
                Picker("范围", selection: $model.refreshScope) {
                    Text("仅当前牌组").tag(AnkiRefreshScope.deck); Text("当前牌组及子牌组").tag(AnkiRefreshScope.deckAndChildren); Text("主牌组及子牌组").tag(AnkiRefreshScope.rootAndChildren)
                }
                if let count = model.refreshCount {
                    Button(count == 0 ? "范围内没有旧卡" : "刷新这 \(count) 张") { if let c = app.core { Task { await model.startRefresh(c) } } }.disabled(model.busy || model.running || count == 0)
                } else {
                    Button(model.counting ? "统计中…" : "先数一下有几张") { if let c = app.core { Task { await model.countRefresh(c) } } }.disabled(model.counting || model.running || model.options.deck.isEmpty || model.status?.connected != true)
                }
            }.buttonStyle(.borderless)
        }.padding(20).glassCard()
    }
}

@MainActor
private struct AnkiHTML: NSViewRepresentable {
    let html: String
    final class Coordinator { var html: String? }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> WKWebView { let config = WKWebViewConfiguration(); config.defaultWebpagePreferences.allowsContentJavaScript = false; return WKWebView(frame: .zero, configuration: config) }
    func updateNSView(_ view: WKWebView, context: Context) {
        if context.coordinator.html == html { return }; context.coordinator.html = html
        view.loadHTMLString("<meta name='color-scheme' content='light dark'><style>body{font:16px -apple-system;background:transparent}img{max-width:100%}</style>" + html, baseURL: nil)
    }
}
