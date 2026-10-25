import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct WordExamplesView: View {
    @Environment(AppModel.self) private var app
    let lemma: String
    var localScope: Subcorpus?
    @State private var result: WordInCorpus?
    private var scope: Subcorpus { SubcorpusScope.mergeSubcorpus(localScope ?? Subcorpus(),app.scopes.active) }
    var body: some View {
        LazyVStack(alignment: .leading,spacing: 8) {
            SubcorpusPicker()
            if let result {
                Text("语料例句 · \(result.occurrences) 次 / \(result.songCount) 首").font(.headline)
                ForEach(result.examples) { e in
                    Button {
                        Task { await app.player.playFrom(songId: e.songId,utteranceId: e.utteranceId,timeSec: e.timeSec) }
                    } label: {
                        VStack(alignment: .leading,spacing: 2) { Text(e.text); Text("\(e.title) · \(e.artist)").font(.caption).foregroundStyle(.secondary) }.frame(maxWidth: .infinity,alignment: .leading)
                    }.buttonStyle(.plain).help(e.timeSec == nil ? "定位这一句歌词" : "从这一句接着播放")
                }
            }
        }.task(id: "\(lemma)|\(scope)|\(app.library.version)") {
            do { let fresh = try await app.core?.wordInCorpus(lemma,currentSongId: app.player.track?.id,scope: scope); if !Task.isCancelled { result = fresh } }
            catch { if !Task.isCancelled { app.errorMessage = error.localizedDescription } }
        }
    }
}
