import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct RepeatEffectPane: View {
    @Environment(AppModel.self) private var app
    let scope: Subcorpus; let jpOnly: Bool
    @State private var raw: FrequencyResult?
    @State private var distinct: FrequencyResult?
    @State private var archived = false
    private var inflated: [FrequencyItem] {
        Array((raw?.items ?? []).filter { $0.rawFreq >= 10 && $0.songs >= 2 && $0.pos != "AUX" && $0.pos != "ADP" }.enumerated().sorted {
            if $0.element.repeatShare != $1.element.repeatShare { return $0.element.repeatShare > $1.element.repeatShare }
            return $0.element.rawFreq == $1.element.rawFreq ? $0.offset < $1.offset : $0.element.rawFreq > $1.element.rawFreq
        }.prefix(12).map(\.element))
    }
    var body: some View {
        VStack(alignment: .leading,spacing: 12) {
            HStack {
                Text("副歌对词频的影响").font(.title2.bold()); Spacer()
                Button(archived ? "已存档" : "存档") {
                    Task { do { _ = try await app.core?.archiveFrequency(FrequencyQuery(scope: AnalysisScope(subcorpus: scope,unit: .distinctLines,jpOnly: jpOnly),limit: 500),label: "副歌效应（去重词频）"); archived = true } catch { app.errorMessage = error.localizedDescription } }
                }.disabled(archived || raw == nil)
            }
            if let raw,let distinct {
                let share = raw.provenance.tokens > 0 ? 1-Double(distinct.provenance.tokens)/Double(raw.provenance.tokens) : 0
                Text("同一首歌里重复出现的行占了全部词次的 \(String(format: "%.1f%%",share*100))（\(raw.provenance.tokens) → \(distinct.provenance.tokens) 词）。以下词至少出现于两首歌、全部词次至少 10 次。").font(.callout).foregroundStyle(.secondary)
                Table(inflated) {
                    TableColumn("词元") { w in Button(w.lemma) { search(w.lemma) }.buttonStyle(.borderless) }
                    TableColumn("词性") { Text(posLabels[$0.pos] ?? $0.pos) }
                    TableColumn("全部词次") { Text(String($0.rawFreq)) }
                    TableColumn("去重后") { Text(String($0.distinctFreq)) }
                    TableColumn("来自重复行") { Text(String(format: "%.0f%%",$0.repeatShare*100)) }
                    TableColumn("曲数") { Text(String($0.songs)) }
                }.clearScrollBackground()
                ProvenanceFooter(p: raw.provenance)
            } else { ProgressView().frame(maxWidth: .infinity,maxHeight: .infinity) }
        }.padding(16)
        .task(id: "\(scope)|\(jpOnly)|\(app.library.version)") {
            archived = false
            do {
                guard let core = app.core else { return }
                async let a = core.frequency(FrequencyQuery(scope: AnalysisScope(subcorpus: scope,unit: .tokens,jpOnly: jpOnly),limit: 100_000))
                async let b = core.frequency(FrequencyQuery(scope: AnalysisScope(subcorpus: scope,unit: .distinctLines,jpOnly: jpOnly),limit: 100_000))
                let results = try await (a,b); guard !Task.isCancelled else { return }; raw = results.0; distinct = results.1
            } catch { if !Task.isCancelled { app.errorMessage = error.localizedDescription } }
        }
    }
    private func search(_ lemma: String) { app.searchRequest = SearchRequest(keyword: lemma,field: .lemma,scope: scope); app.section = .search }
}
