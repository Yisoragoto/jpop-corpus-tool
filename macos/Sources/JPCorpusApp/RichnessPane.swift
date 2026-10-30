import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct RichnessPane: View {
    @Environment(AppModel.self) private var app
    let scope: AnalysisScope
    @State private var group: LexicalQuery.Group = .performer
    @State private var result: LexicalResult?
    @State private var busy = false
    @State private var sort: RichnessColumn = .tokens
    @State private var ascending = false

    private var query: LexicalQuery {
        LexicalQuery(scope: scope, group: group, window: 50, minTokens: group == .whole ? 1 : 100, limit: 1000)
    }
    private var rows: [LexicalRow] {
        (result?.rows ?? []).sorted { a, b in
            let av = sort.value(a), bv = sort.value(b)
            if av != bv { return ascending ? av < bv : av > bv }
            if a.tokens != b.tokens { return a.tokens > b.tokens }
            return a.key < b.key
        }
    }
    private var nameLabel: String {
        switch group {
        case .whole: return "范围"
        case .performer: return "演唱者"
        case .song: return "歌"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            heading
            Text("TTR 会随文本变长一路下降，不同长度的文本没法比；MATTR（50 词窗口的平均 TTR）和 MTLD 基本不受长度影响，越大越丰富；Yule's K 越大越重复。都按词元算。词次不到 100 的演唱者、歌不列出。")
                .font(.callout).foregroundStyle(.secondary)
            if scope.unit == .tokens {
                Text("现在按全部词次算，副歌唱几遍算几遍；勾上筛选里的「副歌只算一遍」更接近「写了什么」。")
                    .font(.caption).foregroundStyle(.secondary)
            }
            AnalysisScriptLegend()
            table
            if let result {
                Text("\(result.rows.count) 组 · 点表头换排序").font(.caption).foregroundStyle(.secondary)
                ProvenanceFooter(p: result.provenance)
            }
        }
        .padding(20)
        .task(id: AnalysisExtrasLoadKey(query: query, library: app.libraryURL, version: app.library.version)) { await load() }
    }

    private var heading: some View {
        HStack(spacing: 14) {
            Text("词汇丰富度与文字").font(.title2.bold())
            if busy { ProgressView().controlSize(.small) }
            Spacer()
            Picker("分组", selection: $group) {
                Text("整体").tag(LexicalQuery.Group.whole)
                Text("按演唱者").tag(LexicalQuery.Group.performer)
                Text("按歌").tag(LexicalQuery.Group.song)
            }.pickerStyle(.segmented).frame(width: 260)
            AnalysisExtrasExportButton(name: "lexical_\(group.rawValue).csv", disabled: busy || rows.isEmpty, content: csv)
        }.controlSize(.small)
    }

    private var table: some View {
        ScrollView([.horizontal, .vertical]) {
            LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
                Section {
                    ForEach(rows, id: \.key) { row in
                        RichnessTableRow(row: row)
                        Divider()
                    }
                } header: {
                    tableHeader.padding(.vertical, 10).background(.regularMaterial)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .overlay {
            if result != nil && rows.isEmpty && !busy {
                Text(group == .whole ? "没有数据" : "没有词次够 100 的组").foregroundStyle(.secondary)
            }
        }
    }

    private var tableHeader: some View {
        HStack(spacing: 14) {
            Text(nameLabel).frame(width: 200, alignment: .leading)
            ForEach(RichnessColumn.allCases, id: \.self) { column in
                Button {
                    if sort == column { ascending.toggle() }
                    else { sort = column; ascending = column == .yuleK }
                } label: {
                    Text(column.title + (sort == column ? (ascending ? " ↑" : " ↓") : ""))
                        .frame(width: column.width, alignment: .trailing)
                }.buttonStyle(.plain).help(column.hint)
            }
            Text("文字构成").frame(width: 150, alignment: .leading)
        }.font(.caption).foregroundStyle(.secondary).padding(.horizontal, 8)
    }

    private func csv() -> String {
        let headers = ["name", "songs", "tokens", "types", "TTR", "MATTR50", "MTLD", "YuleK", "hapaxRatio",
                       "kanji", "hiragana", "katakana", "latin", "other"]
        let values = rows.map { row -> [String] in
            let metrics = [row.label.isEmpty ? "整个范围" : row.label, String(row.songs), String(row.tokens), String(row.types),
                           String(row.ttr), row.mattr.map { String($0) } ?? "", row.mtld.map { String($0) } ?? "",
                           String(row.yuleK), String(row.hapaxRatio)]
            let scripts = [row.scripts.kanji, row.scripts.hiragana, row.scripts.katakana, row.scripts.latin, row.scripts.other]
            return metrics + scripts.map { String($0) }
        }
        return AnalysisExtrasCSV.make(headers, rows: values)
    }

    private func load() async {
        guard let core = app.core else { return }
        busy = true; result = nil
        do {
            let fresh = try await core.lexicalProfile(query)
            guard !Task.isCancelled else { return }
            result = fresh; busy = false
        } catch {
            guard !Task.isCancelled else { return }
            busy = false; app.errorMessage = error.localizedDescription
        }
    }
}

private struct RichnessTableRow: View {
    let row: LexicalRow
    var body: some View {
        HStack(spacing: 14) {
            Text(row.label.isEmpty ? "整个范围" : row.label)
                .lineLimit(1).frame(width: 200, alignment: .leading).help(row.label)
            ForEach(RichnessColumn.allCases, id: \.self) { column in
                Text(column.formatted(row)).monospacedDigit().frame(width: column.width, alignment: .trailing)
            }
            AnalysisScriptBar(shares: row.scripts).frame(width: 150)
        }
        .font(.callout).textSelection(.enabled).padding(.horizontal, 8).padding(.vertical, 10)
    }
}

private enum RichnessColumn: String, CaseIterable {
    case songs, tokens, ttr, mattr, mtld, yuleK, hapaxRatio, kanji, katakana, latin

    var title: String {
        switch self {
        case .songs: return "曲数"
        case .tokens: return "词次"
        case .ttr: return "TTR"
        case .mattr: return "MATTR"
        case .mtld: return "MTLD"
        case .yuleK: return "Yule K"
        case .hapaxRatio: return "单现词"
        case .kanji: return "汉字"
        case .katakana: return "片假名"
        case .latin: return "拉丁"
        }
    }
    var width: CGFloat {
        switch self {
        case .tokens: return 80
        case .mattr, .mtld, .yuleK, .hapaxRatio: return 72
        default: return 58
        }
    }
    var hint: String {
        switch self {
        case .ttr: return "词种 / 词次，会受文本长度影响"
        case .mattr: return "50 词窗口的移动平均 TTR；词次不足窗口时不计算"
        case .mtld: return "TTR 降到 0.72 之前平均能走多少词，正反两遍取平均"
        case .yuleK: return "越大越重复"
        case .hapaxRatio: return "只出现一次的词种占全部词种的比例"
        default: return title
        }
    }
    func value(_ row: LexicalRow) -> Double {
        switch self {
        case .songs: return Double(row.songs)
        case .tokens: return Double(row.tokens)
        case .ttr: return row.ttr
        case .mattr: return row.mattr ?? -Double.infinity
        case .mtld: return row.mtld ?? -Double.infinity
        case .yuleK: return row.yuleK
        case .hapaxRatio: return row.hapaxRatio
        case .kanji: return row.scripts.kanji
        case .katakana: return row.scripts.katakana
        case .latin: return row.scripts.latin
        }
    }
    func formatted(_ row: LexicalRow) -> String {
        switch self {
        case .songs: return String(row.songs)
        case .tokens: return grouped(row.tokens)
        case .ttr: return String(format: "%.3f", row.ttr)
        case .mattr: return row.mattr.map { String(format: "%.3f", $0) } ?? "—"
        case .mtld: return row.mtld.map { String(format: "%.1f", $0) } ?? "—"
        case .yuleK: return String(format: "%.1f", row.yuleK)
        case .hapaxRatio, .kanji, .katakana, .latin: return String(format: "%.1f%%", value(row) * 100)
        }
    }
}
