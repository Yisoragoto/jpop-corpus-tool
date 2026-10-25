import SwiftUI
import JPCore
import JPCorpusModel

@MainActor
struct TokenCorrectionView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    let utteranceId: Int64
    @State private var splitAt = 1
    private var model: LibraryModel { app.library }
    private let tags = ["NOUN", "VERB", "ADJ", "ADV", "ADP", "AUX", "CCONJ", "SCONJ", "DET", "INTJ", "NUM", "PART", "PRON", "PROPN", "PUNCT", "SYM", "X"]
    private var splitLength: Int {
        guard model.selected.count == 1, let i = model.selected.first, model.tokens.indices.contains(i) else { return 0 }
        return Corrections.charCount(model.tokens[i].surface)
    }
    var body: some View {
        @Bindable var m = model
        VStack(alignment: .leading, spacing: 14) {
            Text(model.correction?.text ?? "加载中…").font(.title2)
            Text([model.correction?.artist, model.correction?.title].compactMap { $0 }.joined(separator: " · ")).foregroundStyle(.secondary)
            ScrollView {
                ForEach(model.tokens.indices, id: \.self) { i in
                    HStack {
                        Button("\(i + 1)") { if model.selected.contains(i) { model.selected.removeAll { $0 == i } } else { model.selected.append(i) } }
                            .tint(model.selected.contains(i) ? .accentColor : .secondary)
                        TextField("表层形", text: $m.tokens[i].surface)
                        TextField("词元", text: $m.tokens[i].lemma)
                        Picker("词性", selection: $m.tokens[i].pos) {
                            if !tags.contains(model.tokens[i].pos) { Text(model.tokens[i].pos.isEmpty ? "（空）" : model.tokens[i].pos).tag(model.tokens[i].pos) }
                            ForEach(tags, id: \.self) { Text($0).tag($0) }
                        }.labelsHidden().frame(width: 100)
                    }
                }
            }
            HStack {
                Button("合并选中的词") { let first = model.selected.min(); model.tokens = Corrections.mergeTokens(model.tokens, model.selected); model.selected = first.map { [$0] } ?? [] }.disabled(!Corrections.isContiguous(model.selected))
                Stepper("第 \(splitAt) 个字后", value: $splitAt, in: 1...max(1, splitLength - 1)).disabled(splitLength < 2)
                Button("拆开") { if let i = model.selected.first { model.tokens = Corrections.splitToken(model.tokens, i, splitAt); model.selected = [] } }.disabled(splitLength < 2 || splitAt >= splitLength)
            }.disabled(model.busy)
            if let c = model.correction, !Corrections.matchesLine(Corrections.cleanTokens(model.tokens), c.text) {
                Text("词拼起来和原句对不上。可以保存，但 KWIC 的左右语境会退回按词拼接。").font(.caption).foregroundStyle(.orange)
            }
            if let error = model.errorMessage { Text(error).foregroundStyle(.orange) }
            HStack {
                if model.correction?.corrected == true {
                    Button(model.confirmRevert ? "再点一次，撤销校正" : "撤销校正", role: .destructive) { Task { if await model.revertCorrection() { await changed(); dismiss() } } }
                } else { Button("放弃修改") { model.tokens = model.correction?.tokens ?? [] } }
                Spacer(); Button("取消") { dismiss() }
                Button("保存") { Task { if await model.saveCorrection() { await changed(); dismiss() } } }.disabled(!model.correctionDirty || Corrections.cleanTokens(model.tokens).isEmpty)
            }.disabled(model.busy)
        }.padding(24).frame(width: 680, height: 480).glass(.sidebar)
            .task(id: utteranceId) { await model.openCorrection(utteranceId) }
    }
    private func changed() async {
        if let id = model.correction?.songId { await app.player.refreshLyrics(songId: id) }
        await app.reloadTracks()
        if let request = app.searchRequest { app.search(request.keyword, field: request.field) }
    }
}
