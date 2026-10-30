import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct NgramPane: View {
    @Environment(AppModel.self) private var app
    let scope: AnalysisScope
    @State private var n = 3
    @State private var field: MatchField = .lemma
    @State private var minSongs: Int64 = 2
    @State private var containsDraft = ""
    @State private var contains = ""
    @State private var result: NgramResult?
    @State private var busy = false

    private var query: NgramQuery {
        let text = contains.trimmingCharacters(in: .whitespacesAndNewlines)
        return NgramQuery(scope: scope, n: n, field: field, minFreq: 2, minSongs: minSongs,
                          contains: text.isEmpty ? nil : text, limit: 2000)
    }
    private var shown: [Ngram] { Array((result?.items ?? []).prefix(100)) }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            heading
            Text("连续几个词的固定组合，按次数排。不跨行、不跨标点；至少出现在几首歌里可调——只在一首歌里反复出现的多半是那首歌的副歌。点「检索」看具体是哪几句。")
                .font(.callout).foregroundStyle(.secondary)
            controls
            table
            if let result {
                if result.items.count > 100 {
                    Text("只显示前 100 个；导出 CSV 有全部 \(result.items.count) 个").font(.caption).foregroundStyle(.secondary)
                }
                ProvenanceFooter(p: result.provenance)
            }
        }
        .padding(20)
        .task(id: AnalysisExtrasLoadKey(query: query, library: app.libraryURL, version: app.library.version)) { await load() }
    }

    private var heading: some View {
        HStack {
            Text("N-gram（词簇）").font(.title2.bold())
            Spacer()
            if busy { ProgressView().controlSize(.small) }
            if let result { Text("\(result.items.count) / \(result.types) 种").font(.caption).foregroundStyle(.secondary) }
            AnalysisExtrasExportButton(name: "ngram_\(n).csv", disabled: busy || shown.isEmpty, content: csv)
        }
    }

    private var controls: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 14) { choices; containsField }
            VStack(alignment: .leading, spacing: 10) { choices; containsField }
        }.controlSize(.small)
    }

    private var choices: some View {
        HStack(spacing: 14) {
            Picker("词簇长度", selection: $n) {
                ForEach(2...5, id: \.self) { Text("\($0) 词").tag($0) }
            }.pickerStyle(.segmented).frame(width: 220)
            Picker("组合依据", selection: $field) {
                Text("词元").tag(MatchField.lemma)
                Text("表层").tag(MatchField.surface)
            }.pickerStyle(.segmented).frame(width: 130)
            Stepper("至少 \(minSongs) 首歌", value: $minSongs, in: 1...Int64.max).fixedSize()
        }
    }

    private var containsField: some View {
        TextField("只看含某个词的（回车）", text: $containsDraft)
            .textFieldStyle(.roundedBorder).frame(minWidth: 180, maxWidth: 280)
            .onSubmit { contains = containsDraft }
            .onChange(of: containsDraft) { _, value in if value.isEmpty { contains = "" } }
    }

    private var table: some View {
        Table(shown) {
            TableColumn("词组") { item in
                Text(item.words.joined(separator: " · ")).help(item.pos.map { posLabels[$0] ?? $0 }.joined(separator: " + "))
            }.width(min: 160, ideal: 230)
            TableColumn("最常见的写法") { Text($0.surface).foregroundStyle(.secondary) }
            TableColumn("次数") { Text(String($0.freq)).monospacedDigit() }.width(60)
            TableColumn("每万词") { Text(String(format: "%.1f", $0.per10k)).monospacedDigit() }.width(65)
            TableColumn("出现曲数") { Text(String($0.songs)).monospacedDigit() }.width(65)
            TableColumn("DP") { Text(String(format: "%.2f", $0.dispersion.dp)).monospacedDigit() }.width(50)
            TableColumn("ARF") { Text(String(format: "%.1f", $0.dispersion.arf)).monospacedDigit() }.width(60)
            TableColumn("") { item in Button("检索") { search(item) }.buttonStyle(.link) }.width(45)
        }
        .clearScrollBackground().alternatingRowBackgrounds(.disabled)
        .overlay {
            if result != nil && shown.isEmpty && !busy { Text("没有符合条件的词组").foregroundStyle(.secondary) }
        }
    }

    private func search(_ item: Ngram) {
        app.searchRequest = SearchRequest(keyword: item.words.joined(separator: "+"), field: field, scope: scope.subcorpus)
        app.section = .search
    }

    private func csv() -> String {
        let rows = (result?.items ?? []).enumerated().map { index, item in
            [String(index + 1), item.words.joined(separator: " "), item.surface, item.pos.joined(separator: " "),
             String(item.freq), String(item.per10k), String(item.songs), String(item.dispersion.dp), String(item.dispersion.arf)]
        }
        return AnalysisExtrasCSV.make(["rank", "ngram", "surface", "pos", "freq", "per10k", "songs", "DP", "ARF"], rows: rows)
    }

    private func load() async {
        guard let core = app.core else { return }
        busy = true; result = nil
        do {
            let fresh = try await core.ngrams(query)
            guard !Task.isCancelled else { return }
            result = fresh; busy = false
        } catch {
            guard !Task.isCancelled else { return }
            busy = false; app.errorMessage = error.localizedDescription
        }
    }
}
