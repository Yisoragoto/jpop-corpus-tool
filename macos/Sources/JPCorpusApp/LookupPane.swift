import JPCore
import SwiftUI
import JPCorpusModel

/// 查什么、从哪查的。从歌词里点的词带着歌、行、第几个词，制卡时用这一行做例句、切这一句的音频
struct LookupRequest: Equatable {
    var text: String
    var songId: String?
    var utteranceId: Int64?
    var tokenIndex: Int?
    var scope: Subcorpus? = nil
    /// 点的那个词的原形。`text` 是从这个词往后的一串（词典要按最长匹配查），
    /// 语料统计和例句要按原形查，不然拿一整串去查什么都找不到
    var lemma: String? = nil
    var corpusLemma: String { lemma ?? text }
}

/// 制卡设置。存在这台机器的偏好里（和 Windows 版一样是每台机器各自的），默认值也和 Windows 版一致：
/// 自己的 Lyrics 笔记类型、JPOP 牌组、词+句卡
enum MineDefaults {
    static let deck = "mine.deck"
    static let model = "mine.model"
    static let kind = "mine.kind"
    static let artistSubdeck = "mine.artistSubdeck"
    /// 空串 = 用排在最前、有释义的那本
    static let mainDictionary = "mine.mainDictionary"
}

/// 查词结果（语料库目录里的 Yomitan 词典库 dictionaries.db，和 Windows 版同一份、同一套查法）。
/// 每个词条右边一个「制卡」：Anki 开着就直接加进去，已经有卡的显示「已有」
@MainActor
struct LookupPane: View {
    @Environment(AppModel.self) private var app
    let request: LookupRequest
    /// false：内容按实际高度全部展开，由外面的页面滚动（词典页）。弹出的小面板里仍然自己滚
    var scrolls = true
    let close: () -> Void
    @AppStorage("dict.collapsed") private var collapsedJSON = "[]"
    private var collapsed: Set<String> { Set((try? JSONDecoder().decode([String].self, from: Data(collapsedJSON.utf8))) ?? []) }
    @State private var result: LookupText?
    @State private var noDictionaries = false
    @State private var error: String?
    /// 每个词条（按第一个词头）在 Anki 里已有的卡；Anki 没开就是空
    @State private var existing: [String: [Int64]] = [:]
    @State private var mining: String?
    @State private var outcome: [String: String] = [:]
    /// 这个词在语料里的概况（出现几次、几首、几位歌手、词性）
    @State private var word: WordInCorpus?
    @AppStorage("lookup.tab") private var tab = "dict"

    @AppStorage(MineDefaults.deck) private var deck = "JPOP"
    @AppStorage(MineDefaults.model) private var model = lyricsNoteType
    @AppStorage(MineDefaults.kind) private var kind: LapisCardKind = .wordAndSentence
    @AppStorage(MineDefaults.artistSubdeck) private var artistSubdeck = false
    @AppStorage(MineDefaults.mainDictionary) private var mainDictionary = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                Text(word?.lemma ?? request.corpusLemma).font(.system(size: 26, weight: .bold)).lineLimit(1)
                Spacer()
                Button(action: close) { Image(systemName: "xmark.circle.fill").font(.title3) }
                    .buttonStyle(.borderless)
                    .foregroundStyle(.secondary)
                    .help("收起查词")
            }
            corpusSummary
            Picker("", selection: $tab) {
                Text("词典").tag("dict")
                Text("例句").tag("examples")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            if scrolls {
                ScrollView { tabContent }
            } else {
                tabContent
            }
        }
        .padding(14)
        .task(id: request) { await load() }
        .task(id: "\(request.corpusLemma)|\(String(describing: request.scope))|\(String(describing: app.scopes.active))|\(app.library.version)") { await loadWord() }
    }

    @ViewBuilder private var tabContent: some View {
        if tab == "examples" {
            WordExamplesView(lemma: request.corpusLemma, localScope: request.scope)
                .frame(maxWidth: .infinity, alignment: .leading)
        } else {
            dictionary
        }
    }

    /// 和 Windows 的查词栏一样：词的原形下面先给出现次数、歌曲、歌手，年份和主要词性
    @ViewBuilder private var corpusSummary: some View {
        if let word {
            HStack(spacing: 22) {
                summaryNumber(word.occurrences, "出现")
                summaryNumber(word.songCount, "歌曲")
                summaryNumber(word.artistCount, "歌手")
                Spacer(minLength: 0)
            }
            let years = [word.firstYear, word.lastYear].compactMap { $0 }.filter { !$0.isEmpty }
            if !years.isEmpty || !word.posDistribution.isEmpty {
                HStack(spacing: 6) {
                    if !years.isEmpty {
                        Text(years.count == 2 && years[0] != years[1] ? "\(years[0]) — \(years[1])" : years[0])
                            .font(.caption).foregroundStyle(.secondary).padding(.trailing, 4)
                    }
                    ForEach(Array(word.posDistribution.prefix(4).enumerated()), id: \.offset) { _, p in
                        Text("\(posLabels[p.pos] ?? p.pos) \(p.count)")
                            .font(.caption)
                            .padding(.horizontal, 7).padding(.vertical, 2)
                            .background(Capsule().fill(.quaternary))
                    }
                }
            }
        }
    }

    private func summaryNumber(_ value: Int64, _ label: String) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(grouped(value)).font(.title3.weight(.semibold)).monospacedDigit()
            Text(label).font(.caption).foregroundStyle(.secondary)
        }
    }

    private var dictionary: some View {
                VStack(alignment: .leading, spacing: 10) {
                    if let result, result.originalTextLength > 0, String(request.text.prefix(result.originalTextLength)) != request.corpusLemma {
                        Text("按「\(String(request.text.prefix(result.originalTextLength)))」查到").font(.caption).foregroundStyle(.secondary)
                    }
                    if noDictionaries {
                        Text("这个语料库还没有词典。到「词典」页导入 Yomitan 格式的词典包（zip），或者把 Windows 版的 dictionaries.db 拷过来。")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                        Button("去词典页") { app.section = .dictionaries }
                    } else if let error {
                        Text(error).font(.callout).foregroundStyle(.orange)
                    } else if let result {
                        if result.entries.isEmpty {
                            Text("没查到").foregroundStyle(.secondary)
                        }
                        ForEach(Array(result.entries.prefix(8).enumerated()), id: \.offset) { _, entry in
                            entryView(entry)
                        }
                    } else {
                        ProgressView().controlSize(.small)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func loadWord() async {
        guard let client = app.core else { return }
        let scope = SubcorpusScope.mergeSubcorpus(request.scope ?? Subcorpus(), app.scopes.active)
        do {
            let fresh = try await client.wordInCorpus(request.corpusLemma, currentSongId: request.songId, scope: scope, limit: 1)
            if !Task.isCancelled { word = fresh.occurrences > 0 ? fresh : nil }
        } catch {
            if !Task.isCancelled { word = nil }
        }
    }

    private func entryView(_ entry: LookupEntry) -> some View {
        let key = entry.headwords.first.map { "\($0.term)|\($0.reading)" } ?? ""
        return VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                ForEach(Array(entry.headwords.enumerated()), id: \.offset) { _, h in
                    Text(h.term).font(.title3.weight(.semibold))
                    if !h.reading.isEmpty && h.reading != h.term {
                        Text(h.reading).foregroundStyle(.secondary)
                    }
                }
                Spacer()
                if let first = entry.headwords.first {
                    Button("检索") { app.search(first.term) }
                        .buttonStyle(.borderless)
                        .font(.caption)
                    mineButton(first, key: key)
                }
            }
            if let message = outcome[key] {
                Text(message).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
            ForEach(Array(entry.definitions.enumerated()), id: \.offset) { _, d in
                VStack(alignment: .leading, spacing: 2) {
                    Button {
                        var names = collapsed
                        if names.contains(d.dictionary) { names.remove(d.dictionary) } else { names.insert(d.dictionary) }
                        if let data = try? JSONEncoder().encode(names.sorted()) { collapsedJSON = String(decoding: data, as: UTF8.self) }
                    } label: { Label(d.dictionary, systemImage: collapsed.contains(d.dictionary) ? "chevron.right" : "chevron.down").font(.caption2) }.buttonStyle(.plain)
                    if !collapsed.contains(d.dictionary), let core = app.core { DictionaryDefinitionView(definition: d, client: core) }
                }
            }
        }
    }

    @ViewBuilder
    private func mineButton(_ head: LookupHeadword, key: String) -> some View {
        let has = !(existing[head.term]?.isEmpty ?? true)
        let artist = app.tracks.first(where: { $0.id == request.songId })?.artist ?? ""
        let targetDeck = artistSubdeck ? AnkiLogic.artistSubdeck(deck, artist: artist) ?? deck : deck
        Button {
            Task {
                if has, let ids = existing[head.term] {
                    do { try await app.core?.ankiBrowseNotes(ids) } catch { app.errorMessage = error.localizedDescription }
                } else { await mine(head, key: key) }
            }
        } label: {
            if mining == key {
                ProgressView().controlSize(.mini)
            } else {
                Label(has ? "已有" : "制卡", systemImage: has ? "checkmark.circle" : "plus.rectangle.on.rectangle")
            }
        }
        .buttonStyle(.borderless)
        .font(.caption)
        .disabled(mining != nil)
        .help(has ? "Anki 里已经有这个词的卡，点击打开" : "加进 Anki「\(targetDeck)」牌组（\(kind.label)）。例句是这一行，音频切这一句")
    }

    private func load() async {
        guard let client = app.core else { return }
        result = nil
        error = nil
        outcome = [:]
        existing = [:]
        do {
            noDictionaries = try await client.dictionaries().isEmpty
            if !noDictionaries {
                let found = try await client.lookup(request.text)
                result = found
                await checkExisting(found, client: client)
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// 哪些词条已经有卡了。Anki 没开不算错，按钮照常显示
    private func checkExisting(_ found: LookupText, client: CorpusClient) async {
        let terms = found.entries.prefix(8).compactMap { $0.headwords.first?.term }
        guard !terms.isEmpty, let check = try? await client.ankiMineCheck(model: model, expressions: terms), check.connected else { return }
        for (term, notes) in zip(terms, check.notes) { existing[term] = notes }
    }

    private func mine(_ head: LookupHeadword, key: String) async {
        guard let client = app.core else { return }
        mining = key
        defer { mining = nil }
        let mineRequest = MineRequest(
            lookupText: request.text, term: head.term, reading: head.reading,
            songId: request.songId, utteranceId: request.utteranceId, tokenIndex: request.tokenIndex,
            deck: deck, artistSubdeck: artistSubdeck, model: model,
            mainDictionary: mainDictionary.isEmpty ? nil : mainDictionary, kind: kind
        )
        do {
            switch try await client.ankiMine(mineRequest) {
            case let .added(noteId, deck, created, skipped, warnings):
                var message = "已加进「\(deck)」" + (created ? "（新建了这个子牌组）" : "")
                if !skipped.isEmpty { message += "。这个笔记类型没有 \(skipped.joined(separator: "、")) 字段，这些没写" }
                if !warnings.isEmpty { message += "。" + warnings.joined(separator: "；") }
                outcome[key] = message
                existing[head.term] = [noteId]
            case let .duplicate(ids):
                outcome[key] = "Anki 里已经有这个词的卡了（\(ids.count) 张），没有再加"
                existing[head.term] = ids
            }
        } catch {
            outcome[key] = error.localizedDescription
        }
    }
}
