import Charts
import JPCore
import JPCorpusModel
import SwiftUI

/// 分析页的「概览」，和 Windows 版同一套：一行关键数字，下面四块（文字构成、曲库按年份、
/// 高频实词、副歌撑大的词），再是曲库规模、语料质量入口和词云入口。都跟着上面的范围走（年份那块除外）
@MainActor
struct AnalyticsOverview: View {
    @Environment(AppModel.self) private var app
    let filter: StatsFilter
    /// 跳到别的分页（AnalysisView 的 tag）
    let go: (Int) -> Void
    @State private var years: [YearStats] = []
    @State private var overview: Overview?
    @State private var report: CorpusReport?
    @State private var words: [FrequencyItem]?
    @State private var whole: LexicalRow?

    private static let contentPos: Set<String> = ["NOUN", "VERB", "ADJ", "ADV", "PROPN"]
    private var scope: AnalysisScope {
        AnalysisScope(subcorpus: SubcorpusScope.subcorpus(filter), unit: filter.distinctLines ? .distinctLines : .tokens, jpOnly: filter.jpOnly)
    }
    /// 来自重复行的词次占比
    private var repeatShare: Double? {
        guard let words, !words.isEmpty else { return nil }
        let raw = words.reduce(Int64(0)) { $0 + $1.rawFreq }, distinct = words.reduce(Int64(0)) { $0 + $1.distinctFreq }
        return raw > 0 ? 1 - Double(distinct) / Double(raw) : nil
    }
    private var topContent: [FrequencyItem] { Array((words ?? []).filter { Self.contentPos.contains($0.pos) }.prefix(10)) }
    /// 只在一首歌里出现的词（英文副歌、口癖）重复占比天然接近 100%，至少要两首
    private var inflated: [FrequencyItem] {
        Array((words ?? [])
            .filter { $0.rawFreq >= 10 && $0.songs >= 2 && $0.pos != "AUX" && $0.pos != "ADP" }
            .sorted { $0.repeatShare != $1.repeatShare ? $0.repeatShare > $1.repeatShare : $0.rawFreq > $1.rawFreq }
            .prefix(10))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                kpis
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 380), spacing: 36, alignment: .top)], alignment: .leading, spacing: 30) {
                    OverviewBlock(title: "文字构成", hint: "整个范围里各类文字占的字数") { ScriptBlock(whole: whole) }
                    OverviewBlock(title: "曲库按年份", hint: "整个曲库，不受范围影响") { YearBars(years: years) }
                    OverviewBlock(title: "高频实词", link: "完整词表 →", onLink: { go(0) }) {
                        WordBars(items: topContent, value: { Double($0.freq) }, label: { grouped($0.freq) },
                                 detail: { $0.dispersion.map { String(format: "DP %.2f", $0.dp) } ?? "" },
                                 detailHelp: "DP：越接近 0 在各首歌里分布越均匀", warn: false) { app.search($0.lemma) }
                    }
                    OverviewBlock(title: "副歌撑大的词", hint: "频次大半来自重复唱的行", link: "两种口径对比 →", onLink: { go(4) }) {
                        WordBars(items: inflated, value: { $0.repeatShare }, label: { String(format: "%.0f%%", $0.repeatShare * 100) },
                                 detail: { "\($0.rawFreq) → \($0.distinctFreq)" },
                                 detailHelp: "全部词次 → 重复行只算一次", warn: true) { app.search($0.lemma) }
                    }
                }
                if let o = overview { library(o) }
                HStack(spacing: 12) {
                    entry(icon: "checkmark.seal", title: "语料质量", detail: "歌词来源、时间轴、分词状态，最需要看一眼的歌") { go(3) }
                    entry(icon: "doc.plaintext", title: "统计报告", detail: "整个范围的统计摘要，可以导出 TXT") { go(6) }
                }
                CloudEntry { app.libraryPath = []; app.libraryMode = .cloud; app.section = .library }
            }
            .padding(24)
            .frame(maxWidth: 1180, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .clearScrollBackground()
        .task(id: "\(app.libraryURL?.path ?? "")|\(app.library.version)") {
            guard let core = app.core else { return }
            do { years = try await core.timeline(); overview = try await core.overview() }
            catch { app.errorMessage = error.localizedDescription }
        }
        .task(id: "\(filter)|\(app.library.version)") {
            guard let core = app.core else { return }
            report = nil; words = nil; whole = nil
            let scope = scope, filter = filter
            do {
                let fresh = try await core.statsReport(filter)
                if !Task.isCancelled { report = fresh.report }
                let freq = try await core.frequency(FrequencyQuery(scope: scope, pos: nil, limit: 1_000_000))
                if !Task.isCancelled { words = freq.items }
                let lexical = try await core.lexicalProfile(LexicalQuery(scope: scope))
                if !Task.isCancelled { whole = lexical.rows.first }
            } catch {
                if !Task.isCancelled { app.errorMessage = error.localizedDescription }
            }
        }
    }

    private var kpis: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 130), spacing: 10)], spacing: 10) {
            Kpi(value: report.map { grouped($0.songCount) } ?? "…", label: "曲目")
            Kpi(value: report.map { grouped($0.utteranceCount) } ?? "…", label: "歌词行")
            Kpi(value: report.map { grouped($0.tokenCount) } ?? "…", label: "词次", hint: "不含标点")
            Kpi(value: report.map { grouped($0.typeCount) } ?? "…", label: "词种", hint: "不同的词元")
            Kpi(value: repeatShare.map { String(format: "%.1f%%", $0 * 100) } ?? "…", label: "来自重复行", hint: "同一首歌里重复唱的行占的词次")
            Kpi(value: whole == nil ? "…" : whole?.mattr.map { String(format: "%.3f", $0) } ?? "—", label: "MATTR", hint: "不受长度影响的词汇丰富度（窗口 50）")
        }
    }

    private func library(_ o: Overview) -> some View {
        OverviewBlock(title: "曲库", hint: "整个曲库") {
            HStack(spacing: 0) {
                ForEach(Array([
                    (grouped(o.tracks), "首"), (grouped(o.albums), "张专辑"), (grouped(o.performers), "位演唱"),
                    (grouped(o.composers), "位作曲"), (grouped(o.lyricists), "位作词"), (formatListening(o.totalDurationSec), "总时长"),
                ].enumerated()), id: \.offset) { _, item in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(item.0).font(.title3.weight(.semibold)).monospacedDigit()
                        Text(item.1).font(.caption).foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
    }

    private func entry(icon: String, title: String, detail: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: icon).font(.title2).foregroundStyle(.tint).frame(width: 28)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.headline)
                    Text(detail).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right").foregroundStyle(.tertiary)
            }
            .padding(14)
            .frame(maxWidth: .infinity)
            .contentShape(Rectangle())
            .glassCard(cornerRadius: 10)
        }
        .buttonStyle(.plain)
    }
}

/// 一个数字 + 一行说明（Windows 的 .kpi）
private struct Kpi: View {
    let value: String
    let label: String
    var hint: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(value).font(.system(size: 24, weight: .semibold)).monospacedDigit().lineLimit(1).minimumScaleFactor(0.7)
            Text(label).font(.caption).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 16).padding(.vertical, 14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassCard(cornerRadius: 8)
        .help(hint ?? "")
    }
}

/// 概览里的一块：标题、灰色提示、右边一个跳转链接，下面是内容。不画框（区块不是方框）
private struct OverviewBlock<Content: View>: View {
    let title: String
    var hint: String?
    var link: String?
    var onLink: (() -> Void)?
    @ViewBuilder let content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(title).font(.headline)
                if let hint { Text(hint).font(.caption).foregroundStyle(.secondary) }
                Spacer(minLength: 0)
                if let link, let onLink { Button(link, action: onLink).buttonStyle(.link).font(.caption) }
            }
            content
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }
}

/// 五类文字的占比：一根分段条 + 下面的数字
private struct ScriptBlock: View {
    let whole: LexicalRow?
    static let kinds: [(String, String, Color)] = [
        ("kanji", "汉字", Color(red: 0x4c / 255, green: 0xc2 / 255, blue: 1)),
        ("hiragana", "平假名", Color(red: 0x9b / 255, green: 0xe1 / 255, blue: 0x8e / 255)),
        ("katakana", "片假名", Color(red: 1, green: 0xb8 / 255, blue: 0x6b / 255)),
        ("latin", "拉丁字母", Color(red: 0xc7 / 255, green: 0x9b / 255, blue: 1)),
        ("other", "其他", Color(red: 0x8a / 255, green: 0x8a / 255, blue: 0x96 / 255)),
    ]
    private func share(_ s: ScriptShares, _ key: String) -> Double {
        switch key {
        case "kanji": return s.kanji
        case "hiragana": return s.hiragana
        case "katakana": return s.katakana
        case "latin": return s.latin
        default: return s.other
        }
    }
    var body: some View {
        if let scripts = whole?.scripts {
            VStack(alignment: .leading, spacing: 14) {
                GeometryReader { geo in
                    HStack(spacing: 0) {
                        ForEach(Self.kinds.indices, id: \.self) { i in
                            let kind = Self.kinds[i]
                            Rectangle().fill(kind.2).frame(width: geo.size.width * share(scripts, kind.0))
                        }
                    }
                }
                .frame(height: 14)
                .clipShape(RoundedRectangle(cornerRadius: 4))
                .help(Self.kinds.map { "\($0.1) \(String(format: "%.1f%%", share(scripts, $0.0) * 100))" }.joined(separator: " · "))
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 110), spacing: 14)], alignment: .leading, spacing: 8) {
                    ForEach(Self.kinds.indices, id: \.self) { i in
                            let kind = Self.kinds[i]
                        HStack(spacing: 6) {
                            RoundedRectangle(cornerRadius: 2).fill(kind.2).frame(width: 10, height: 10)
                            Text(kind.1).foregroundStyle(.secondary)
                            Spacer(minLength: 4)
                            Text(String(format: "%.1f%%", share(scripts, kind.0) * 100)).fontWeight(.semibold).monospacedDigit()
                        }
                        .font(.callout)
                    }
                }
            }
        } else {
            Text("计算中…").font(.caption).foregroundStyle(.secondary)
        }
    }
}

/// 每年几首（细柱）
private struct YearBars: View {
    let years: [YearStats]
    var body: some View {
        if years.isEmpty {
            Text("没有带年份的曲目").font(.caption).foregroundStyle(.secondary)
        } else {
            Chart(years) { y in
                BarMark(x: .value("年份", String(y.year.suffix(2))), y: .value("歌曲", y.trackCount))
                    .foregroundStyle(Color.accentColor.opacity(0.85))
                    .cornerRadius(2)
            }
            .chartYAxis(.hidden)
            .chartXAxis {
                AxisMarks { _ in AxisValueLabel().font(.caption2) }
            }
            .frame(height: 120)
        }
    }
}

/// 名次 · 词 · 词性 · 条 · 数字 · 补充（Windows 的 .ov-list）
private struct WordBars: View {
    let items: [FrequencyItem]
    let value: (FrequencyItem) -> Double
    let label: (FrequencyItem) -> String
    let detail: (FrequencyItem) -> String
    let detailHelp: String
    let warn: Bool
    let open: (FrequencyItem) -> Void
    var body: some View {
        let top = items.map(value).max() ?? 1
        let scale = warn ? 1 : max(top, 1)
        if items.isEmpty {
            Text("计算中…").font(.caption).foregroundStyle(.secondary)
        } else {
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 0) {
                ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                    GridRow {
                        Text("\(index + 1)").font(.caption2).foregroundStyle(.secondary).monospacedDigit()
                            .gridColumnAlignment(.trailing)
                        Button(item.lemma) { open(item) }.buttonStyle(.plain).lineLimit(1).help("检索「\(item.lemma)」")
                        Text(posLabels[item.pos] ?? item.pos).font(.caption).foregroundStyle(.secondary)
                        Capsule().fill(.quaternary)
                            .overlay(alignment: .leading) {
                                GeometryReader { geo in
                                    Capsule().fill(warn ? Color.orange : Color.accentColor).opacity(0.85)
                                        .frame(width: geo.size.width * min(1, value(item) / scale))
                                }
                            }
                            .frame(minWidth: 60, maxWidth: .infinity)
                            .frame(height: 6)
                        Text(label(item)).monospacedDigit().gridColumnAlignment(.trailing)
                        Text(detail(item)).font(.caption).foregroundStyle(.secondary).monospacedDigit().help(detailHelp)
                            .gridColumnAlignment(.trailing)
                    }
                    .frame(height: 30)
                }
            }
            .font(.callout)
        }
    }
}

/// 词云入口：几个大小不一的字当插画
private struct CloudEntry: View {
    let open: () -> Void
    var body: some View {
        Button(action: open) {
            HStack(spacing: 18) {
                ZStack {
                    Text("夢").font(.system(size: 30, weight: .bold)).foregroundStyle(Color(red: 159 / 255, green: 216 / 255, blue: 1)).offset(x: -16, y: -4)
                    Text("君").font(.system(size: 18, weight: .semibold)).foregroundStyle(.white.opacity(0.85)).offset(x: 18, y: -12)
                    Text("世界").font(.system(size: 14)).foregroundStyle(.white.opacity(0.6)).offset(x: 14, y: 14)
                }
                .frame(width: 96, height: 64)
                .background(
                    LinearGradient(colors: [Color(red: 0.16, green: 0.2, blue: 0.32), Color(red: 0.24, green: 0.16, blue: 0.34)], startPoint: .topLeading, endPoint: .bottomTrailing),
                    in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                VStack(alignment: .leading, spacing: 3) {
                    Text("词云在「资料库」里").font(.headline)
                    Text("可以选词性（默认名词）、换字体，点词看出现在哪些歌、点歌从那一句开始放，还能全屏").font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
                Text("打开 →").foregroundStyle(.tint)
            }
            .padding(12)
            .contentShape(Rectangle())
            .glassCard(cornerRadius: 12)
        }
        .buttonStyle(.plain)
    }
}

@MainActor
struct StatsReportPane: View {
    @Environment(AppModel.self) private var app
    let filter: StatsFilter
    @State private var result: StatsReport?
    @State private var exportNote = ""
    @State private var loading = false
    var body: some View {
        ScrollView {
            if let result {
                VStack(alignment: .leading,spacing: 16) {
                    Text("语料统计报告").font(.title2.bold())
                    HStack {
                        Button { Task { await export(result.text) } } label: { Label("导出 TXT", systemImage: "square.and.arrow.up") }.buttonStyle(.borderless).disabled(loading)
                        Text(exportNote).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                    Text(result.text).font(.system(.body,design: .monospaced)).textSelection(.enabled)
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 100))],alignment: .leading) { ForEach(result.report.topWords) { w in Button("\(w.value) · \(w.count)") { app.searchRequest = SearchRequest(keyword: w.value,field: .lemma,scope: SubcorpusScope.subcorpus(filter)); app.section = .search }.buttonStyle(.borderless) } }
                }.padding(24).frame(maxWidth: .infinity,alignment: .leading)
            } else { ProgressView().padding(40) }
        }.task(id: "\(filter)|\(app.library.version)") {
            exportNote = ""
            loading = true
            do { let fresh = try await app.core?.statsReport(filter); if !Task.isCancelled { result = fresh; loading = false } }
            catch { if !Task.isCancelled { loading = false; app.errorMessage = error.localizedDescription } }
        }
    }
    private func export(_ text: String) async {
        guard let client = app.core else { return }
        do { if let path = try await SaveExport.text(text, name: "corpus_report.txt", title: "保存统计报告", client: client) { exportNote = "已保存：\(path)" } }
        catch { app.errorMessage = error.localizedDescription }
    }
}
