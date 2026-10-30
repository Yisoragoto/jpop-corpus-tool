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
        VStack(alignment: .leading, spacing: 8) {
            deckSettings
            noteSettings.padding(.top, 16)
            if connection?.connected != true {
                SettingNote(connection?.message.isEmpty == false ? (connection?.message ?? "") : "Anki 没连上，连上后可选择真实的牌组列表。")
            }
            if artistSubdeck { SettingNote("子牌组没有就新建，合作曲放进第一位歌手的；不是从歌里查的词放回主牌组。") }
            SettingNote("例句使用点选的歌词行，查的词加粗；同歌的其他例句接在后面，每句都从歌里切一段音频。")
            if connection?.connected == true {
                if !models.isEmpty && !models.contains(model) {
                    Text(model == lyricsNoteType ? "Anki 里还没有「Lyrics」笔记类型，第一次制卡时会自动建。" : "Anki 里没有「\(model)」笔记类型，换一个。").font(.caption).foregroundStyle(.secondary)
                }
                if let actual = connection?.decks, !actual.isEmpty && !actual.contains(deck) { Text("Anki 里没有「\(deck)」牌组，制卡会失败。").font(.caption).foregroundStyle(.orange) }
            }
        }
        .task(id: "\(app.libraryURL?.path ?? "")|\(app.dictionaryVersion)|\(status?.connected == true)") {
            if status == nil { fetched = try? await app.core?.ankiStatus() }
            if connection?.connected == true { models = (try? await app.core?.ankiModelNames()) ?? [] }
            dictionaries = (try? await app.core?.dictionaries()) ?? []
        }
    }

    private var deckSettings: some View {
        SettingGroup("Anki") {
            SettingRow(icon: "rectangle.stack", title: "牌组", description: "查词结果上的「＋ 制卡」会放进这里") {
                Picker("牌组", selection: $deck) {
                    ForEach(decks, id: \.self) { Text($0).tag($0) }
                }.frame(width: 190).disabled(connection?.connected != true)
            }
            SettingDivider()
            SettingRow(icon: "person.crop.rectangle.stack", title: "按歌手放进子牌组",
                       description: artistSubdeck ? "放进「\(deck)::歌手」" : "子牌组没有就新建") {
                Toggle("按歌手放进子牌组", isOn: $artistSubdeck).toggleStyle(.switch)
            }
        }
    }

    private var noteSettings: some View {
        SettingGroup("笔记与释义") {
            SettingRow(icon: "doc.text", title: "笔记类型", description: "默认「Lyrics」：Lapis 风格，背面显示封面与歌曲信息") {
                Picker("笔记类型", selection: $model) {
                    ForEach(modelChoices, id: \.self) { Text($0).tag($0) }
                }.frame(width: 190)
            }
            SettingDivider()
            SettingRow(icon: "character.book.closed", title: "首选释义", description: "首选词典没有释义时，使用排在最前的可用词典") {
                Picker("首选释义", selection: $mainDictionary) {
                    Text("排在最前的词典").tag("")
                    ForEach(terms) { Text($0.title).tag($0.title) }
                    if !mainDictionary.isEmpty && !terms.contains(where: { $0.title == mainDictionary }) {
                        Text("\(mainDictionary)（未启用）").tag(mainDictionary)
                    }
                }.frame(width: 190)
            }
            SettingDivider()
            SettingRow(icon: "rectangle.on.rectangle", title: "卡片类型", description: "选择单词、词句、句子或音频等卡片样式") {
                Picker("卡片类型", selection: $kind) {
                    ForEach(LapisCardKind.allCases, id: \.self) { Text($0.label).tag($0) }
                }.frame(width: 190)
            }
        }
    }
}
