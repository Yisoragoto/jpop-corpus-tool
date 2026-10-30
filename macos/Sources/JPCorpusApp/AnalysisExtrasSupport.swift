import Foundation
import JPCore
import JPCorpusModel
import SwiftUI

struct AnalysisExtrasLoadKey<Query: Hashable>: Hashable {
    let query: Query
    let library: URL?
    let version: Int
}

enum AnalysisExtrasCSV {
    static func make(_ headers: [String], rows: [[String]]) -> String {
        "\u{feff}" + ([headers] + rows).map { ExportText.csvRow($0.map(ExportText.CsvValue.text)) }.joined()
    }
}

@MainActor
struct AnalysisExtrasExportButton: View {
    @Environment(AppModel.self) private var app
    let name: String
    let disabled: Bool
    let content: () -> String
    @State private var note = ""
    @State private var busy = false

    var body: some View {
        HStack(spacing: 8) {
            if !note.isEmpty { Text(note).font(.caption).foregroundStyle(.secondary).lineLimit(1).help(note) }
            Button("导出 CSV") { Task { await save() } }.disabled(disabled || busy)
        }
    }

    private func save() async {
        guard let core = app.core else { return }
        busy = true
        defer { busy = false }
        do {
            if let path = try await SaveExport.text(content(), name: name, title: "保存 CSV", client: core) {
                note = "已保存：\(path)"
            }
        } catch { app.errorMessage = error.localizedDescription }
    }
}

/// 各分析使用相同的文字颜色，便于横向比较。
struct AnalysisScriptBar: View {
    let shares: ScriptShares
    static let labels = ["汉字", "平假名", "片假名", "拉丁字母", "其他"]
    static let colors: [Color] = [
        Color(red: 0.30, green: 0.76, blue: 1), Color(red: 0.61, green: 0.88, blue: 0.56),
        Color(red: 1, green: 0.72, blue: 0.42), Color(red: 0.78, green: 0.61, blue: 1), .gray,
    ]
    private var values: [Double] { [shares.kanji, shares.hiragana, shares.katakana, shares.latin, shares.other] }
    private var hint: String {
        Self.labels.indices.map { "\(Self.labels[$0]) \(String(format: "%.1f%%", values[$0] * 100))" }.joined(separator: " · ")
    }

    var body: some View {
        GeometryReader { geo in
            HStack(spacing: 0) {
                ForEach(Self.labels.indices, id: \.self) { i in
                    Rectangle().fill(Self.colors[i]).frame(width: geo.size.width * min(1, max(0, values[i])))
                }
            }
        }
        .frame(height: 12)
        .clipShape(RoundedRectangle(cornerRadius: 3))
        .help(hint)
        .accessibilityLabel(hint)
    }
}

struct AnalysisScriptLegend: View {
    var body: some View {
        HStack(spacing: 16) {
            ForEach(AnalysisScriptBar.labels.indices, id: \.self) { i in
                HStack(spacing: 5) {
                    RoundedRectangle(cornerRadius: 2).fill(AnalysisScriptBar.colors[i]).frame(width: 9, height: 9)
                    Text(AnalysisScriptBar.labels[i])
                }
            }
        }.font(.caption).foregroundStyle(.secondary)
    }
}
