import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct MineSettingsView: View {
    @Environment(AppModel.self) private var app
    var status: AnkiStatus?
    @State private var fetched: AnkiStatus?
    @State private var models: [String] = []
    @State private var dictionaries: [DictionaryInfo] = []
    @AppStorage(MineDefaults.deck) private var deck = "JPOP"
    @AppStorage(MineDefaults.model) private var model = lyricsNoteType
    @AppStorage(MineDefaults.kind) private var kind = LapisCardKind.wordAndSentence
    @AppStorage(MineDefaults.artistSubdeck) private var artistSubdeck = false
    @AppStorage(MineDefaults.mainDictionary) private var mainDictionary = ""
    private var connection: AnkiStatus? { status ?? fetched }
    private var decks: [String] { let list = connection?.decks ?? []; return list.contains(deck) ? list : [deck] + list }
    private var modelChoices: [String] { var list: [String] = []; for item in [model, lyricsNoteType] + models where !list.contains(item) { list.append(item) }; return list }
    private var terms: [DictionaryInfo] { dictionaries.filter { $0.enabled && ($0.counts?.terms ?? 0) > 0 } }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("一键制卡").font(.title2.bold())
            LabeledContent("牌组") { Picker("牌组", selection: $deck) { ForEach(decks, id: \.self) { Text($0).tag($0) } }.labelsHidden().disabled(connection?.connected != true) }
            Text(connection?.connected == true ? "在曲库里点词，查词结果上的「＋ 制卡」就放进这里" : "Anki 没连上，列不出牌组；连上之后这里会是真实的牌组列表").font(.caption).foregroundStyle(.secondary)
            Toggle("按歌手放进子牌组", isOn: $artistSubdeck)
            Text(artistSubdeck ? "放进「\(deck)::歌手」；子牌组没有就新建，合作曲放进第一位歌手的。不是从歌里查的词还放在上面那个牌组" : "开了以后制卡放进「牌组::歌手」的子牌组，没有就新建").font(.caption).foregroundStyle(.secondary)
            Picker("笔记类型", selection: $model) { ForEach(modelChoices, id: \.self) { Text($0).tag($0) } }
            Text("默认「Lyrics」：照 Lapis 做的，背面右侧是封面，封面右边竖排歌名、歌手、专辑").font(.caption).foregroundStyle(.secondary)
            Picker("首选释义", selection: $mainDictionary) {
                Text("排在最前的词典").tag("")
                ForEach(terms) { Text($0.title).tag($0.title) }
                if !mainDictionary.isEmpty && !terms.contains(where: { $0.title == mainDictionary }) { Text("\(mainDictionary)（未启用）").tag(mainDictionary) }
            }
            Text("MainDefinition 用哪本词典；这个词那本里没有时，用排在最前、有释义的那本").font(.caption).foregroundStyle(.secondary)
            Picker("卡片类型", selection: $kind) { ForEach(LapisCardKind.allCases, id: \.self) { Text($0.label).tag($0) } }
            Text("例句是点的那一行（查的词加粗），这首歌里其他含这个词的句子接在后面，每句都从歌里切一段音频").font(.caption).foregroundStyle(.secondary)
            if connection?.connected == true {
                if !models.isEmpty && !models.contains(model) {
                    Text(model == lyricsNoteType ? "Anki 里还没有「Lyrics」笔记类型，第一次制卡时会自动建。" : "Anki 里没有「\(model)」笔记类型，换一个。").font(.caption).foregroundStyle(.secondary)
                }
                if let actual = connection?.decks, !actual.isEmpty && !actual.contains(deck) { Text("Anki 里没有「\(deck)」牌组，制卡会失败。").font(.caption).foregroundStyle(.orange) }
            } else if let connection { Text(connection.message).font(.caption).foregroundStyle(.secondary) }
        }
        .task(id: "\(app.libraryURL?.path ?? "")|\(app.dictionaryVersion)|\(status?.connected == true)") {
            if status == nil { fetched = try? await app.core?.ankiStatus() }
            if connection?.connected == true { models = (try? await app.core?.ankiModelNames()) ?? [] }
            dictionaries = (try? await app.core?.dictionaries()) ?? []
        }
    }
}
