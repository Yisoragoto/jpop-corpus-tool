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
    let close: () -> Void
    @State private var result: LookupText?
    @State private var noDictionaries = false
    @State private var error: String?
    /// 每个词条（按第一个词头）在 Anki 里已有的卡；Anki 没开就是空
    @State private var existing: [String: [Int64]] = [:]
    @State private var mining: String?
    @State private var outcome: [String: String] = [:]

    @AppStorage(MineDefaults.deck) private var deck = "JPOP"
    @AppStorage(MineDefaults.model) private var model = lyricsNoteType
    @AppStorage(MineDefaults.kind) private var kind: LapisCardKind = .wordAndSentence
    @AppStorage(MineDefaults.artistSubdeck) private var artistSubdeck = false
    @AppStorage(MineDefaults.mainDictionary) private var mainDictionary = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("查词").font(.headline)
                if let result, result.originalTextLength > 0 {
                    Text(String(request.text.prefix(result.originalTextLength))).foregroundStyle(.secondary)
                }
                Spacer()
                Button(action: close) { Image(systemName: "xmark.circle.fill") }
                    .buttonStyle(.borderless)
                    .foregroundStyle(.secondary)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
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
                    WordExamplesView(lemma: request.text,localScope: request.scope)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(12)
        .task(id: request) { await load() }
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
                    Text(d.dictionary).font(.caption2).foregroundStyle(.tertiary)
                    ForEach(Array(d.text.enumerated()), id: \.offset) { _, line in
                        Text(line).font(.callout).textSelection(.enabled)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func mineButton(_ head: LookupHeadword, key: String) -> some View {
        let has = !(existing[head.term]?.isEmpty ?? true)
        Button {
            Task { await mine(head, key: key) }
        } label: {
            if mining == key {
                ProgressView().controlSize(.mini)
            } else {
                Label(has ? "已有" : "制卡", systemImage: has ? "checkmark.circle" : "plus.rectangle.on.rectangle")
            }
        }
        .buttonStyle(.borderless)
        .font(.caption)
        .disabled(mining != nil || has)
        .help(has ? "Anki 里已经有这个词的卡了" : "加进 Anki「\(deck)」牌组（\(kind.label)）。例句是这一行，音频切这一句")
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
            case let .added(_, deck, created, skipped, warnings):
                var message = "已加进「\(deck)」" + (created ? "（新建了这个子牌组）" : "")
                if !skipped.isEmpty { message += "。这个笔记类型没有 \(skipped.joined(separator: "、")) 字段，这些没写" }
                if !warnings.isEmpty { message += "。" + warnings.joined(separator: "；") }
                outcome[key] = message
                existing[head.term] = [1]
            case let .duplicate(ids):
                outcome[key] = "Anki 里已经有这个词的卡了（\(ids.count) 张），没有再加"
                existing[head.term] = ids
            }
        } catch {
            outcome[key] = error.localizedDescription
        }
    }
}
