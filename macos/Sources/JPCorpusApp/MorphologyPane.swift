import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct MorphologyPane: View {
    let scope: AnalysisScope

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                InflectionSection(scope: scope).padding(18).glassCard()
                OrthographySection(scope: scope).padding(18).glassCard()
            }.padding(20)
        }.clearScrollBackground()
    }
}

@MainActor
private struct InflectionSection: View {
    @Environment(AppModel.self) private var app
    let scope: AnalysisScope
    @State private var pos = "VERB"
    @State private var result: InflectionResult?
    @State private var busy = false
    private var query: InflectionQuery { InflectionQuery(scope: scope, pos: pos.isEmpty ? nil : pos, examples: 8) }

    private struct CoarseShare: Identifiable {
        let id: String
        let share: Double
    }
    private var coarse: [CoarseShare] {
        var sums: [String: Double] = [:]
        for form in result?.forms ?? [] { sums[form.coarse, default: 0] += form.share }
        return sums.map { CoarseShare(id: $0.key, share: $0.value) }.sorted {
            $0.share == $1.share ? $0.id < $1.id : $0.share > $1.share
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            heading
            Text("动词、形容词、助动词按活用形数。歌词里命令形、意志形往往比书面语多。点例词去检索。活用形的名字照 UniDic（Sudachi）的写法。")
                .font(.callout).foregroundStyle(.secondary)
            if let result {
                MorphologyPending(count: result.unannotated)
                coarseBar
                if result.forms.isEmpty { Text("没有数据").foregroundStyle(.secondary) }
                else { forms }
                ProvenanceFooter(p: result.provenance)
            }
        }
        .task(id: AnalysisExtrasLoadKey(query: query, library: app.libraryURL, version: app.library.version)) { await load() }
    }

    private var heading: some View {
        HStack(spacing: 12) {
            Text("活用形").font(.title2.bold())
            if busy { ProgressView().controlSize(.small) }
            if let result { Text("\(grouped(result.total)) 个有活用的词").font(.caption).foregroundStyle(.secondary) }
            Spacer()
            Picker("词性", selection: $pos) {
                Text("全部").tag("")
                Text("动词").tag("VERB")
                Text("形容词").tag("ADJ")
                Text("助动词").tag("AUX")
            }.frame(width: 120)
            AnalysisExtrasExportButton(name: "inflection.csv", disabled: busy || (result?.forms.isEmpty ?? true), content: csv)
        }.controlSize(.small)
    }

    private var coarseBar: some View {
        VStack(alignment: .leading, spacing: 8) {
            GeometryReader { geo in
                HStack(spacing: 0) {
                    ForEach(coarse.indices, id: \.self) { i in
                        Rectangle().fill(AnalysisScriptBar.colors[i % AnalysisScriptBar.colors.count])
                            .frame(width: geo.size.width * coarse[i].share)
                            .help("\(coarse[i].id) \(String(format: "%.1f%%", coarse[i].share * 100))")
                    }
                }
            }.frame(height: 14).clipShape(RoundedRectangle(cornerRadius: 3))
            Text(coarse.map { "\($0.id) \(String(format: "%.1f%%", $0.share * 100))" }.joined(separator: " · "))
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private var forms: some View {
        ScrollView(.horizontal) {
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 12) {
                GridRow {
                    Text("活用形").frame(width: 180, alignment: .leading)
                    Text("次数").frame(width: 65, alignment: .trailing)
                    Text("比例").frame(width: 150, alignment: .trailing)
                    Text("出现曲数").frame(width: 65, alignment: .trailing)
                    Text("常见的词")
                }.font(.caption).foregroundStyle(.secondary)
                ForEach(result?.forms ?? []) { form in
                    GridRow {
                        Text(form.cform)
                        Text(grouped(form.freq)).monospacedDigit().gridColumnAlignment(.trailing)
                        shareBar(form)
                        Text(String(form.songs)).monospacedDigit().gridColumnAlignment(.trailing)
                        examples(form)
                    }
                }
            }.font(.callout)
        }
    }

    private func shareBar(_ form: InflectionForm) -> some View {
        HStack(spacing: 8) {
            ProgressView(value: form.share, total: max(result?.forms.first?.share ?? 1, 0.001)).frame(width: 80)
            Text(String(format: "%.1f%%", form.share * 100)).monospacedDigit().frame(width: 54, alignment: .trailing)
        }
    }

    private func examples(_ form: InflectionForm) -> some View {
        HStack(spacing: 12) {
            ForEach(form.lemmas.indices, id: \.self) { i in
                let example = form.lemmas[i]
                Button("\(example.word) \(example.count)") {
                    app.searchRequest = SearchRequest(keyword: example.word, field: .lemma, scope: scope.subcorpus)
                    app.section = .search
                }.buttonStyle(.link).help("检索「\(example.word)」")
            }
        }
    }

    private func csv() -> String {
        let rows = (result?.forms ?? []).map { form in
            [form.cform, form.coarse, String(form.freq), String(form.share), String(form.songs),
             form.lemmas.map { "\($0.word):\($0.count)" }.joined(separator: " ")]
        }
        return AnalysisExtrasCSV.make(["cform", "coarse", "freq", "share", "songs", "lemmas"], rows: rows)
    }

    private func load() async {
        guard let core = app.core else { return }
        busy = true; result = nil
        do {
            let fresh = try await core.inflection(query)
            guard !Task.isCancelled else { return }
            result = fresh; busy = false
        } catch {
            guard !Task.isCancelled else { return }
            busy = false; app.errorMessage = error.localizedDescription
        }
    }
}

@MainActor
private struct OrthographySection: View {
    @Environment(AppModel.self) private var app
    let scope: AnalysisScope
    @State private var result: OrthographyResult?
    @State private var busy = false
    @State private var mixedOnly = true
    private var query: OrthographyQuery { OrthographyQuery(scope: scope, minFreq: 3, limit: 2000) }
    private var groups: [VariantGroup] {
        (result?.groups ?? []).filter { !mixedOnly || Set($0.forms.map(\.writing)).count >= 2 }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            heading
            Text("同一个词在歌词里的不同写法，按 Sudachi 的正规化表记归组；全是假名的写法按读音并到同词性的汉字写法（只对不活用的词）。只看实词。点写法看是哪几句。")
                .font(.callout).foregroundStyle(.secondary)
            if let result {
                MorphologyPending(count: result.unannotated)
                legend
                if groups.isEmpty { Text("没有符合条件的词").foregroundStyle(.secondary) }
                else { variants }
                ProvenanceFooter(p: result.provenance)
            }
        }
        .task(id: AnalysisExtrasLoadKey(query: query, library: app.libraryURL, version: app.library.version)) { await load() }
    }

    private var heading: some View {
        HStack(spacing: 12) {
            Text("表记变体").font(.title2.bold())
            if busy { ProgressView().controlSize(.small) }
            if let result { Text("\(groups.count) / \(result.totalGroups) 组").font(.caption).foregroundStyle(.secondary) }
            Spacer()
            Picker("写法", selection: $mixedOnly) {
                Text("汉字与假名混用").tag(true)
                Text("全部").tag(false)
            }.pickerStyle(.segmented).frame(width: 220)
            AnalysisExtrasExportButton(name: "orthography.csv", disabled: busy || groups.isEmpty, content: csv)
        }.controlSize(.small)
    }

    private var legend: some View {
        HStack(spacing: 16) {
            ForEach(Writing.allCases, id: \.self) { writing in
                HStack(spacing: 5) {
                    Circle().fill(color(writing)).frame(width: 8, height: 8)
                    Text(label(writing))
                }
            }
        }.font(.caption).foregroundStyle(.secondary)
    }

    private var variants: some View {
        LazyVStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 14) {
                Text("词").frame(width: 110, alignment: .leading)
                Text("读音").frame(width: 85, alignment: .leading)
                Text("词性").frame(width: 55, alignment: .leading)
                Text("合计").frame(width: 55, alignment: .trailing)
                Text("写法（次数）")
            }.font(.caption).foregroundStyle(.secondary)
            ForEach(groups) { group in
                OrthographyRow(group: group, scope: scope.subcorpus)
                Divider()
            }
        }
    }

    private func csv() -> String {
        let rows = groups.map { group in
            [group.normalized, group.reading, group.pos, String(group.freq),
             group.forms.map { "\($0.lemma):\($0.freq):\($0.writing.rawValue)" }.joined(separator: " ")]
        }
        return AnalysisExtrasCSV.make(["normalized", "reading", "pos", "freq", "forms"], rows: rows)
    }

    private func load() async {
        guard let core = app.core else { return }
        busy = true; result = nil
        do {
            let fresh = try await core.orthography(query)
            guard !Task.isCancelled else { return }
            result = fresh; busy = false
        } catch {
            guard !Task.isCancelled else { return }
            busy = false; app.errorMessage = error.localizedDescription
        }
    }
}

@MainActor
private struct OrthographyRow: View {
    @Environment(AppModel.self) private var app
    let group: VariantGroup
    let scope: Subcorpus

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            Text(group.normalized).fontWeight(.medium).frame(width: 110, alignment: .leading)
            Text(group.reading).foregroundStyle(.secondary).frame(width: 85, alignment: .leading)
            Text(posLabels[group.pos] ?? group.pos).foregroundStyle(.secondary).frame(width: 55, alignment: .leading)
            Text(String(group.freq)).monospacedDigit().frame(width: 55, alignment: .trailing)
            VStack(alignment: .leading, spacing: 8) {
                variantBar
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 100), alignment: .leading)], alignment: .leading, spacing: 8) {
                    ForEach(group.forms) { form in
                        Button("\(form.lemma) \(form.freq)") {
                            app.searchRequest = SearchRequest(keyword: form.lemma, field: .lemma, scope: scope)
                            app.section = .search
                        }
                        .buttonStyle(.link).tint(color(form.writing))
                        .help("\(label(form.writing)) · \(form.songs) 首歌 · 检索这种写法")
                    }
                }
            }.frame(minWidth: 180, maxWidth: .infinity, alignment: .leading)
        }.font(.callout)
    }

    private var variantBar: some View {
        GeometryReader { geo in
            HStack(spacing: 0) {
                ForEach(group.forms) { form in
                    Rectangle().fill(color(form.writing))
                        .frame(width: geo.size.width * Double(form.freq) / Double(max(1, group.freq)))
                }
            }
        }.frame(height: 8).clipShape(RoundedRectangle(cornerRadius: 2))
    }
}

private struct MorphologyPending: View {
    let count: Int64
    var body: some View {
        if count > 0 {
            Text("还有 \(grouped(count)) 个词的形态信息在后台补，补完之前这张表不全。")
                .font(.caption).foregroundStyle(.orange)
        }
    }
}

private func label(_ writing: Writing) -> String {
    switch writing {
    case .kanji: return "汉字"
    case .hiragana: return "平假名"
    case .katakana: return "片假名"
    case .kana: return "假名混写"
    case .other: return "其他"
    }
}

private func color(_ writing: Writing) -> Color {
    switch writing {
    case .kanji: return AnalysisScriptBar.colors[0]
    case .hiragana: return AnalysisScriptBar.colors[1]
    case .katakana: return AnalysisScriptBar.colors[2]
    case .kana: return AnalysisScriptBar.colors[3]
    case .other: return AnalysisScriptBar.colors[4]
    }
}
