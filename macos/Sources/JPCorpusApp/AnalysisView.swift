import JPCore
import SwiftUI

/// 分析：重复感知的词频、子语料对比。每个词都能双击回到检索页看具体的句子、再去听。
@MainActor
struct AnalysisView: View {
    @State private var tab = 0

    var body: some View {
        VStack(spacing: 0) {
            Picker("", selection: $tab) {
                Text("词频").tag(0)
                Text("子语料对比").tag(1)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .padding(10)
            Divider()
            if tab == 0 { FrequencyPane() } else { KeynessPane() }
        }
        .navigationTitle("分析")
    }
}

@MainActor
private struct FrequencyPane: View {
    @Environment(AppModel.self) private var app
    @State private var unit: CountUnit = .distinctLines
    @State private var pos = "NOUN"
    @State private var jpOnly = true
    @State private var result: FrequencyResult?
    @State private var selection: Set<FrequencyItem.ID> = []

    private let posChoices = ["", "NOUN", "VERB", "ADJ", "ADV", "PRON", "PROPN", "INTJ"]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 14) {
                Picker("计数", selection: $unit) {
                    Text("去掉重复行").tag(CountUnit.distinctLines)
                    Text("全部词次").tag(CountUnit.tokens)
                }
                .pickerStyle(.segmented)
                .fixedSize()
                .help("副歌会原样唱好几遍。去掉重复行 = 同一首歌里相同的行只算第一次")
                Picker("词性", selection: $pos) {
                    ForEach(posChoices, id: \.self) { p in
                        Text(p.isEmpty ? "全部" : (posLabels[p] ?? p)).tag(p)
                    }
                }
                .fixedSize()
                Toggle("仅日文", isOn: $jpOnly)
                Spacer()
            }
            .controlSize(.small)
            .padding(10)

            Table(result?.items ?? [], selection: $selection) {
                TableColumn("词元") { Text($0.lemma) }
                TableColumn("词性") { Text(posLabels[$0.pos] ?? $0.pos).foregroundStyle(.secondary) }.width(60)
                TableColumn("频次") { Text("\($0.freq)").monospacedDigit() }.width(56)
                TableColumn("每万词") { Text(String(format: "%.1f", $0.per10k)).monospacedDigit() }.width(60)
                TableColumn("全部 / 去重") { Text("\($0.rawFreq) / \($0.distinctFreq)").monospacedDigit().foregroundStyle(.secondary) }.width(90)
                TableColumn("来自重复行") { item in
                    Text(String(format: "%.0f%%", item.repeatShare * 100))
                        .monospacedDigit()
                        .foregroundStyle(item.repeatShare >= 0.4 ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
                }
                .width(80)
                TableColumn("曲数") { Text("\($0.songs)").monospacedDigit() }.width(48)
                TableColumn("写法") { Text($0.surfaces.joined(separator: "、")).foregroundStyle(.secondary).lineLimit(1) }
            }
            .contextMenu(forSelectionType: FrequencyItem.ID.self) { ids in
                if let item = result?.items.first(where: { ids.contains($0.id) }) {
                    Button("检索「\(item.lemma)」") { app.search(item.lemma) }
                }
            } primaryAction: { ids in
                if let item = result?.items.first(where: { ids.contains($0.id) }) { app.search(item.lemma) }
            }

            if let p = result?.provenance {
                ProvenanceFooter(p: p)
            }
        }
        .task(id: "\(unit.rawValue)|\(pos)|\(jpOnly)") {
            let query = FrequencyQuery(scope: AnalysisScope(unit: unit, jpOnly: jpOnly), pos: pos.isEmpty ? nil : pos, limit: 500)
            do {
                result = try await app.client?.frequency(query)
            } catch {
                app.errorMessage = error.localizedDescription
            }
        }
    }
}

@MainActor
private struct KeynessPane: View {
    @Environment(AppModel.self) private var app
    @State private var targetFrom = "2020"
    @State private var targetTo = ""
    @State private var refFrom = ""
    @State private var refTo = ""
    @State private var unit: CountUnit = .distinctLines
    @State private var result: KeynessResult?
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 16) {
                yearRange("目标", $targetFrom, $targetTo)
                yearRange("参照", $refFrom, $refTo)
                Picker("", selection: $unit) {
                    Text("去掉重复行").tag(CountUnit.distinctLines)
                    Text("全部词次").tag(CountUnit.tokens)
                }
                .pickerStyle(.segmented)
                .fixedSize()
                Button(busy ? "计算中…" : "对比") { Task { await run() } }
                    .disabled(busy)
                    .keyboardShortcut(.defaultAction)
                Spacer()
            }
            .controlSize(.small)
            Text("参照留空 = 语料库里其余的歌。按对数似然排序，两边合计少于 5 次的词不列出。双击一个词去检索。")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let result {
                HStack(alignment: .top, spacing: 16) {
                    keyList("目标里偏多", result.over)
                    keyList("目标里偏少", result.under)
                }
                ProvenanceFooter(p: result.target, label: "目标")
                ProvenanceFooter(p: result.reference, label: "参照")
            } else {
                Spacer()
            }
        }
        .padding(12)
    }

    private func yearRange(_ label: String, _ from: Binding<String>, _ to: Binding<String>) -> some View {
        HStack(spacing: 4) {
            Text(label).foregroundStyle(.secondary)
            TextField("年份", text: from).frame(width: 52)
            Text("–")
            TextField("年份", text: to).frame(width: 52)
        }
        .textFieldStyle(.roundedBorder)
    }

    private func keyList(_ title: String, _ items: [KeyItem]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.headline)
            List(items) { k in
                HStack {
                    Text(k.lemma)
                    Text(posLabels[k.pos] ?? k.pos).font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Text(String(format: "%.1f vs %.1f /万词 · LL %.1f", k.targetPer10k, k.referencePer10k, abs(k.logLikelihood)))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                .contentShape(Rectangle())
                .onTapGesture(count: 2) { app.search(k.lemma) }
            }
            .listStyle(.bordered)
        }
    }

    private func run() async {
        let target = Subcorpus(yearFrom: Int(targetFrom), yearTo: Int(targetTo))
        let reference = Subcorpus(yearFrom: Int(refFrom), yearTo: Int(refTo))
        guard !target.isAll else {
            app.errorMessage = "先给「目标」填一个年份范围"
            return
        }
        busy = true
        defer { busy = false }
        var query = KeynessQuery(target: target, reference: reference.isAll ? nil : reference)
        query.unit = unit
        do {
            result = try await app.client?.keyness(query)
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }
}

/// 结果的出处：哪些歌、多少词、怎么数、库指纹
@MainActor
struct ProvenanceFooter: View {
    let p: Provenance
    var label: String?

    var body: some View {
        Text("\(label.map { "\($0)：" } ?? "")\(p.scope) · \(p.songs) 首 / \(p.tokens) 词（\(p.unit == .distinctLines ? "重复行只算一次" : "全部词次")）· 规则 \(p.rules) · 指纹 \(String(p.snapshot.fingerprint.prefix(8)))")
            .font(.caption2)
            .foregroundStyle(.tertiary)
            .textSelection(.enabled)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
    }
}
