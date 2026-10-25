import Foundation
import JPCore

/// corpusQuality.ts 的显示逻辑；条件归一化见 SubcorpusScope。
public enum CorpusQuality {
    public static let issueLabels = [
        "empty": "一行歌词都没有", "replacement_char": "有乱码", "not_japanese": "看起来不是日文歌词",
        "too_few_lines": "行数太少", "ends_early": "提前结束，可能被截断", "untimed": "没有时间轴",
        "mixed_timing": "部分行没有时间", "non_monotonic": "时间倒退", "beyond_duration": "时间超过歌曲长度",
        "duplicate_timestamp": "和上一行同一个时间", "stacked_duplicate": "重复贴了一遍",
        "long_gap": "长空档（间奏或漏行）", "leftover_markup": "混进了时间标记或署名", "very_long_line": "一行特别长"
    ]
    public static func issueLabel(_ code: String) -> String { issueLabels[code] ?? code }
    public static func completenessLabel(_ value: Double?) -> String {
        guard let value else { return "未校验" }
        if value >= 0.95 { return "完整" }; if value >= 0.8 { return "基本完整" }
        return value >= 0.5 ? "可能有缺" : "很可能有问题"
    }
    public static func percent(_ value: Double?, digits: Int = 0) -> String {
        guard let value else { return "—" }
        let digits = min(100,max(0,digits)), scale = pow(10,Double(digits))
        let rounded = (value*100*scale).rounded(.toNearestOrAwayFromZero)/scale
        return String(format: "%.*f%%",locale: Locale(identifier: "en_US_POSIX"),digits,rounded)
    }
    public static func tokenizerLabel(_ t: LyricTokenizerRun) -> String {
        "\(t.engine) \(t.engineVersion) · \(t.dictDescription.isEmpty ? "未知词典" : t.dictDescription) · Mode \(t.splitMode)"
    }
    public static func metricLabel(_ name: String) -> String {
        let parts = name.split(separator: ".",omittingEmptySubsequences: false).map(String.init)
        let sides = ["provenance":"", "target":"目标", "reference":"参照"], fields = ["songs":"曲数", "tokens":"词次"]
        if parts.count > 1, let side = sides[parts[0]], let field = fields[parts[1]] { return side+field }
        let labels = ["songCount":"曲数", "utteranceCount":"歌词行数", "tokenCount":"词次", "typeCount":"词种",
                      "ttr":"TTR", "sttr":"STTR", "sttrChunk":"STTR 段长", "hapaxCount":"Hapax 数", "hapaxRatio":"Hapax 比率",
                      "avgTypesPerSong":"平均每曲词汇量", "avgTokensPerLine":"平均每行 token"]
        return labels[name] ?? (name.hasPrefix("coverage@") ? "Top \(name.dropFirst(9)) 覆盖率" : name)
    }
}
