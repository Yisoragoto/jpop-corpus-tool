import Charts
import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct AnalyticsOverview: View {
    @Environment(AppModel.self) private var app
    @State private var years: [YearStats] = []
    @State private var overview: Overview?
    var body: some View {
        ScrollView {
            VStack(alignment: .leading,spacing: 24) {
                if let o = overview {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 110))],alignment: .leading,spacing: 18) {
                        StatBlock(value: grouped(o.tracks),label: "曲目"); StatBlock(value: grouped(o.albums),label: "专辑")
                        StatBlock(value: grouped(o.people),label: "人物"); StatBlock(value: grouped(o.performers),label: "演唱")
                        StatBlock(value: grouped(o.composers),label: "作曲"); StatBlock(value: grouped(o.lyricists),label: "作词")
                        StatBlock(value: grouped(o.arrangers),label: "编曲"); StatBlock(value: grouped(o.lyricLines),label: "歌词行")
                        StatBlock(value: grouped(o.tokens),label: "词次"); StatBlock(value: grouped(o.vocabulary),label: "词汇量")
                        StatBlock(value: formatListening(o.totalDurationSec),label: "总时长")
                    }
                }
                Text("发行年份").font(.title2.bold())
                Chart(years) { y in BarMark(x: .value("年份",y.year),y: .value("歌曲",y.trackCount)).foregroundStyle(Color.accentColor.gradient) }.frame(height: 220)
                Text("词汇随年份变化").font(.title2.bold())
                Chart(years) { y in LineMark(x: .value("年份",y.year),y: .value("词种",y.vocabulary)).interpolationMethod(.linear) }.frame(height: 180)
                ForEach(years) { y in HStack { Text(y.year); Spacer(); Text("\(y.trackCount) 首 · \(y.albumCount) 张专辑 · \(y.vocabulary) 词种").foregroundStyle(.secondary) } }
                Button("打开词云") { app.libraryPath = []; app.libraryMode = .cloud; app.section = .library }
            }.padding(24)
        }.task(id: "\(app.libraryURL?.path ?? "")|\(app.library.version)") {
            do { years = try await app.core?.timeline() ?? []; overview = try await app.core?.overview() }
            catch { app.errorMessage = error.localizedDescription }
        }
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
