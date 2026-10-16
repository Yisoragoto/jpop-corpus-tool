import JPCore
import SwiftUI

/// 搭配表：节点词左右 3 个词内（不跨行）常一起出现的词。
/// 点一个词 → KWIC 只留附近有它的行；▶ 听一句两者一起出现的歌词。
@MainActor
struct CollocationPanel: View {
    @Environment(AppModel.self) private var app
    let node: String
    let field: MatchField
    let scope: AnalysisScope
    let selected: String?
    let onPick: (String) -> Void

    @State private var measure: CollocationMeasure = .logDice
    @State private var contentOnly = true
    @State private var result: CollocationResult?
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("「\(node)」的搭配").font(.headline)
                Spacer()
                if busy { ProgressView().controlSize(.small) }
            }
            Picker("", selection: $measure) {
                Text("logDice").tag(CollocationMeasure.logDice)
                Text("MI").tag(CollocationMeasure.mi)
                Text("LL").tag(CollocationMeasure.logLikelihood)
                Text("次数").tag(CollocationMeasure.frequency)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            Toggle("只看实词（去掉助词、助动词、连词）", isOn: $contentOnly)
                .controlSize(.small)

            if let result {
                List(result.collocates) { c in
                    HStack {
                        VStack(alignment: .leading, spacing: 1) {
                            Text(c.lemma).fontWeight(selected == c.lemma ? .bold : .regular)
                            Text("\(posLabels[c.pos] ?? c.pos) · \(c.freq) 次（左 \(c.freqLeft) / 右 \(c.freqRight)）· \(c.songs) 首")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(score(c)).monospacedDigit().foregroundStyle(.secondary)
                        Button {
                            Task { await app.player.audition(c.exampleUtteranceId) }
                        } label: {
                            Image(systemName: "play.circle")
                        }
                        .buttonStyle(.borderless)
                        .help("听一句两者一起出现的歌词")
                    }
                    .contentShape(Rectangle())
                    .onTapGesture { onPick(c.lemma) }
                    .listRowBackground(selected == c.lemma ? Color.accentColor.opacity(0.15) : Color.clear)
                }
                .listStyle(.plain)
                Text("节点 \(result.nodeFreq) 次 · \(result.provenance.scope) · \(result.provenance.songs) 首 / \(result.provenance.tokens) 词（\(result.provenance.unit == .distinctLines ? "重复行只算一次" : "全部词次")）· 指纹 \(String(result.provenance.snapshot.fingerprint.prefix(8)))")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                    .textSelection(.enabled)
            } else {
                Spacer()
            }
        }
        .padding(12)
        .task(id: TaskKey(node: node, field: field, scope: scope, measure: measure, contentOnly: contentOnly)) {
            await load()
        }
    }

    private struct TaskKey: Hashable {
        let node: String
        let field: MatchField
        let scope: AnalysisScope
        let measure: CollocationMeasure
        let contentOnly: Bool
    }

    private func score(_ c: Collocate) -> String {
        switch measure {
        case .frequency: return "\(c.freq)"
        case .logLikelihood: return String(format: "%.1f", c.logLikelihood)
        default: return String(format: "%.2f", c.score(measure))
        }
    }

    private func load() async {
        guard let client = app.client else { return }
        busy = true
        defer { busy = false }
        var query = CollocationQuery(node: node, field: field, scope: scope)
        query.measure = measure
        query.contentOnly = contentOnly
        do {
            result = try await client.collocations(query)
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }
}
