import Foundation
import JPCorpusModel
import JPCore
import SwiftUI

enum KwicSearchSpec: Hashable, Sendable {
    case kwic(KwicQuery)
    case cql(CqlQuery)

    func breakdown(by: BreakdownGroup) -> BreakdownQuery {
        switch self {
        case .kwic(let query): return BreakdownQuery(by: by, kwic: query)
        case .cql(let query): return BreakdownQuery(by: by, cql: query)
        }
    }

    var network: NetworkQuery? {
        guard case .kwic(let query) = self,
              query.keywords.count == 1, let word = query.keywords.first,
              !word.contains("+") else { return nil }
        let scope = AnalysisScope(subcorpus: query.subcorpus,
                                  unit: query.dedup ? .distinctLines : .tokens, jpOnly: query.jpOnly)
        var collocation = CollocationQuery(node: word, field: query.field, scope: scope)
        collocation.limit = 20
        return NetworkQuery(collocation: collocation)
    }
}

enum KwicResultMode: String, CaseIterable {
    case list = "列表"
    case context = "语境分解"
    case breakdown = "年份·歌手"
    case network = "网络"
}

struct CqlExamplesView: View {
    let onPick: (String) -> Void
    @State private var expanded = true

    private struct Example: Identifiable {
        let query: String
        let note: String
        var id: String { query }
    }

    private let examples: [Example] = [
        Example(query: #"[lemma="君"] [pos="ADP"]"#, note: "「君」后面紧跟一个助词"),
        Example(query: #"[pos="VERB" & cform="命令形.*"]"#, note: "所有命令形的动词"),
        Example(query: #"[lemma="て"] []{0,2} [lemma="しまう"]"#, note: "て……しまう，中间最多隔两个词"),
        Example(query: #"[reading="きみ"]"#, note: "读音是 きみ 的词，各种写法都算"),
        Example(query: #"[norm="会う"] [lemma="たい"]"#, note: "「会いたい」「逢いたい」…"),
        Example(query: #"[pos="ADJ"] [pos="NOUN"]"#, note: "形容词 + 名词"),
        Example(query: #""夢" []? [pos="NOUN"]"#, note: "「夢」后面隔零到一个词跟名词"),
    ]

    // 不用 DisclosureGroup、长说明不让它自由换行：macOS 上 DisclosureGroup 里的多行 Text
    // 放在检索页的竖排里，排版会来回量个不停，整个窗口卡死、一片空白（切到 CQL 就会碰到）
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button { expanded.toggle() } label: {
                Label("CQL 示例", systemImage: expanded ? "chevron.down" : "chevron.right")
            }
            .buttonStyle(.borderless)
            if expanded {
                Text("CQL 按词的属性找词串：方括号里写一个词的条件，几个方括号连起来是相邻的几个词。值是正则表达式，要整个匹配。")
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                ForEach(examples) { example in
                    Button { onPick(example.query) } label: {
                        HStack(spacing: 12) {
                            Text(example.query).font(.system(.caption, design: .monospaced)).lineLimit(1)
                            Text(example.note).foregroundStyle(.secondary).lineLimit(1)
                            Spacer(minLength: 0)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                Text("不跨行；标点也是词；每个起点取最长的匹配，匹配之间不重叠。")
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .font(.caption)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

@MainActor
struct KwicBreakdownView: View {
    @Environment(AppModel.self) private var app
    let search: KwicSearchSpec
    @State private var by: BreakdownGroup = .year
    @State private var result: BreakdownResult?
    @State private var errorMessage = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text("频率分解").font(.headline)
                    Picker("分组", selection: $by) {
                        Text("按年份").tag(BreakdownGroup.year)
                        Text("按演唱者").tag(BreakdownGroup.performer)
                    }
                    .pickerStyle(.segmented).fixedSize()
                    Spacer()
                }
                if !errorMessage.isEmpty {
                    Text(errorMessage).foregroundStyle(.red).textSelection(.enabled)
                } else if let result {
                    breakdownRows(result)
                } else {
                    ProgressView("计算中…").controlSize(.small)
                }
            }
            .padding(20)
        }
        .task(id: search.breakdown(by: by)) { await load() }
    }

    private func breakdownRows(_ result: BreakdownResult) -> some View {
        let maximum = max(0.0001, max(result.per10k, result.rows.map(\.per10k).max() ?? 0))
        return VStack(alignment: .leading, spacing: 10) {
            Text("\(result.hits) 次 · 整体每万词 \(result.per10k, specifier: "%.2f")（不去重，分母不含标点）")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Text(by == .year ? "年份" : "演唱者").frame(width: 110, alignment: .leading)
                Text("每万词").frame(maxWidth: .infinity, alignment: .trailing)
                Text("次数").frame(width: 56, alignment: .trailing)
                Text("命中歌 / 歌数").frame(width: 95, alignment: .trailing)
            }
            .font(.caption).foregroundStyle(.secondary)
            LazyVStack(spacing: 10) {
                ForEach(result.rows) { row in
                    BreakdownFrequencyRow(row: row, maximum: maximum, mean: result.per10k)
                }
            }
            Text("条形是每万词，竖线是整体水平。右边两列：次数、有这个词的歌 / 这一组的歌数。")
            Text("* 这一组不到 1,000 个词，每万词很不稳，一两次出现就能翻倍。")
        }
        .font(.caption)
    }

    private func load() async {
        guard !Task.isCancelled else { return }
        result = nil
        errorMessage = ""
        guard let client = app.core else { return }
        let query = search.breakdown(by: by)
        do {
            let value = try await client.kwicBreakdown(query)
            guard !Task.isCancelled else { return }
            result = value
        } catch {
            guard !Task.isCancelled else { return }
            errorMessage = error.localizedDescription
        }
    }
}

private struct BreakdownFrequencyRow: View {
    let row: BreakdownRow
    let maximum: Double
    let mean: Double

    private var barColor: Color {
        if row.ratio >= 2 { return .orange }
        if row.ratio > 0 && row.ratio <= 0.5 { return .secondary }
        return .accentColor
    }

    var body: some View {
        HStack {
            Text((row.label.isEmpty ? "年份不明" : row.label) + (row.tokens < 1000 ? " *" : ""))
                .lineLimit(1).frame(width: 110, alignment: .leading)
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule().fill(barColor.opacity(0.7))
                        .frame(width: max(0, geometry.size.width * row.per10k / maximum), height: 8)
                    Rectangle().fill(Color.primary.opacity(0.5)).frame(width: 1, height: 14)
                        .offset(x: max(0, min(geometry.size.width - 1, geometry.size.width * mean / maximum)))
                }
                .frame(height: geometry.size.height)
            }
            .frame(height: 16)
            Text(String(format: "%.1f", row.per10k)).frame(width: 55, alignment: .trailing)
            Text("\(row.hits)").frame(width: 56, alignment: .trailing)
            Text("\(row.hitSongs)/\(row.songs)").frame(width: 95, alignment: .trailing)
        }
        .monospacedDigit()
        .help("\(row.hits) 次 · \(row.hitSongs) / \(row.songs) 首歌有 · 这一组 \(row.tokens) 词 · 是整体的 \(String(format: "%.2f", row.ratio)) 倍")
    }
}

/// 保留完整语境，避免把日语按字符误当作分词。
@MainActor
struct KwicContextBreakdownView: View {
    let hits: [KwicHit]
    @State private var left: [ContextCount] = []
    @State private var right: [ContextCount] = []

    private struct ContextCount: Identifiable, Sendable {
        let context: String
        let count: Int
        var id: String { context }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("语境分解").font(.headline)
                Text("按当前列表的完整左／右语境分组；每条命中计一次，沿用去重与跨行设置。最多统计 5,000 条命中。")
                    .font(.caption).foregroundStyle(.secondary)
                HStack(alignment: .top, spacing: 28) {
                    contextColumn("左语境（关键词之前）", groups: left)
                    contextColumn("右语境（关键词之后）", groups: right)
                }
            }
            .padding(20)
        }
        .task(id: hits) {
            let input = hits
            let grouped = await Task.detached(priority: .userInitiated) {
                return (Self.group(input.map(\.left)), Self.group(input.map(\.right)))
            }.value
            guard !Task.isCancelled else { return }
            left = grouped.0
            right = grouped.1
        }
    }

    private func contextColumn(_ title: String, groups: [ContextCount]) -> some View {
        LazyVStack(alignment: .leading, spacing: 10) {
            Text(title).font(.subheadline).fontWeight(.semibold)
            ForEach(groups) { group in
                HStack(alignment: .top) {
                    Text(group.context.isEmpty ? "（无语境）" : group.context).textSelection(.enabled)
                    Spacer(minLength: 8)
                    Text("\(group.count)").monospacedDigit().foregroundStyle(.secondary)
                }
                .font(.caption)
            }
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    nonisolated private static func group(_ contexts: [String]) -> [ContextCount] {
        let counts = contexts.reduce(into: [String: Int]()) { $0[$1, default: 0] += 1 }
        return counts.map { ContextCount(context: $0.key, count: $0.value) }
            .sorted { $0.count == $1.count ? $0.context < $1.context : $0.count > $1.count }
    }
}
