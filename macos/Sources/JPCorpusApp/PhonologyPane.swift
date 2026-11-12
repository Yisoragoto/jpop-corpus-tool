import JPCore
import JPCorpusModel
import SwiftUI

/// 分析的「音系」：元音分布、单元音词和单元音行、押韵检索、相邻行押韵。和 Windows 的
/// PhonologyCard、RhymeCard 一样，读音按发音拆拍的规则和押韵的定义都在 Rust（jp-corpus 的 phonology.rs）。
@MainActor
struct PhonologyPane: View {
    let scope: AnalysisScope
    /// 点「常见韵脚」时交给押韵检索
    @State private var rhymeRequest: RhymeRequest?

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 28) {
                    VowelSection(scope: scope).padding(18).glassCard()
                    UnivocalicSection(scope: scope).padding(18).glassCard()
                    RhymeSearchSection(scope: scope, request: rhymeRequest).padding(18).glassCard().id("rhyme-search")
                    RhymeStatsSection(scope: scope) { request in
                        rhymeRequest = request
                        // 检索在长表上方，多半已经滚出视野
                        withAnimation { proxy.scrollTo("rhyme-search", anchor: .top) }
                    }.padding(18).glassCard()
                }.padding(20)
            }.clearScrollBackground()
        }
    }
}

/// 从「常见韵脚」带过去的检索条件；serial 让同一个韵脚点第二次也算新请求
struct RhymeRequest: Equatable {
    var pattern: String
    var position: RhymePosition
    var strict: Bool
    var serial: Int
}

private let rowLabels: [Vowel: String] = [.a: "ア段", .i: "イ段", .u: "ウ段", .e: "エ段", .o: "オ段"]
private let rowKana: [Vowel: String] = [.a: "ア", .i: "イ", .u: "ウ", .e: "エ", .o: "オ"]
private func pct(_ share: Double) -> String { String(format: "%.1f%%", share * 100) }

/// 解析后的模式 aiu → a·i·u；n、q 写成 ン、ッ；两种读法都算的 (ou) 写成 o/u
func showRhymePattern(_ pattern: String) -> String {
    func one(_ c: Character) -> String {
        switch c {
        case "n": return "ン"
        case "q": return "ッ"
        case ".": return "＿"
        default: return String(c)
        }
    }
    var parts: [String] = []
    var chars = pattern.makeIterator()
    while let c = chars.next() {
        if c == "(" {
            var inner: [String] = []
            while let d = chars.next(), d != ")" { inner.append(one(d)) }
            parts.append(inner.joined(separator: "/"))
        } else {
            parts.append(one(c))
        }
    }
    return parts.joined(separator: "·")
}

/// 按 Unicode 标量下标（Rust 的 char）切出押韵的那一段，强调色加粗
func rhymeHighlighted(_ text: String, start: Int?, end: Int?) -> AttributedString {
    let scalars = Array(text.unicodeScalars)
    guard let start, let end, start <= end, end <= scalars.count else { return AttributedString(text) }
    func piece(_ slice: ArraySlice<Unicode.Scalar>) -> String {
        var view = String.UnicodeScalarView()
        view.append(contentsOf: slice)
        return String(view)
    }
    var out = AttributedString(piece(scalars[..<start]))
    var mark = AttributedString(piece(scalars[start..<end]))
    mark.foregroundColor = .accentColor
    mark.inlinePresentationIntent = .stronglyEmphasized
    out.append(mark)
    out.append(AttributedString(piece(scalars[end...])))
    return out
}

// ────────────────────────────── 元音分布 ──────────────────────────────

@MainActor
private struct VowelSection: View {
    @Environment(AppModel.self) private var app
    let scope: AnalysisScope
    @State private var result: VowelResult?
    @State private var busy = false
    private var query: VowelQuery { VowelQuery(scope: scope) }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
                Text("元音分布").font(.app(.title2).bold())
                if busy { ProgressView().controlSize(.small) }
                Spacer()
                if let result { Text("\(grouped(result.vowelMorae)) 拍").font(.app(.caption)).foregroundStyle(.secondary) }
            }
            Text("歌词按发音拆成拍（モーラ），数每拍的元音。助词は读ワ、エイ / オウ 读长音（エーエン、トーキョー），长音拍算前一拍的元音；促音、拨音没有元音，单独列。")
                .font(.app(.callout)).foregroundStyle(.secondary)
            if busy && result == nil { Text("计算中…").font(.app(.caption)).foregroundStyle(.secondary) }
            if let result, result.vowelMorae > 0 {
                bar(result).opacity(busy ? 0.55 : 1)
                VStack(alignment: .leading, spacing: 4) {
                    Text("促音 \(grouped(result.sokuon)) 拍 · 拨音 \(grouped(result.hatsuon)) 拍 · 长音 \(grouped(result.long)) 拍")
                    Text("拆了 \(grouped(result.words)) 个词")
                    if result.skipped > 0 {
                        Text("\(grouped(result.skipped)) 个没有假名读音（多是英文，如 \(result.skippedExamples.prefix(3).map(\.word).joined(separator: "、"))），没算")
                            .help(result.skippedExamples.map { "\($0.word) ×\($0.count)" }.joined(separator: "\n"))
                    }
                }.font(.app(.caption)).foregroundStyle(.secondary)
            }
            if let result { ProvenanceFooter(p: result.provenance) }
        }
        .task(id: AnalysisExtrasLoadKey(query: query, library: app.libraryURL, version: app.library.version)) { await load() }
    }

    private func bar(_ result: VowelResult) -> some View {
        GeometryReader { geo in
            HStack(spacing: 0) {
                ForEach(result.vowels.indices, id: \.self) { i in
                    let v = result.vowels[i]
                    ZStack {
                        Rectangle().fill(AnalysisScriptBar.colors[i % AnalysisScriptBar.colors.count])
                        Text(verbatim: "\(rowKana[v.vowel] ?? "") \(pct(v.share))")
                            .font(.app(.caption).bold()).foregroundStyle(.black.opacity(0.75)).lineLimit(1)
                    }
                    .frame(width: geo.size.width * v.share)
                    .help(Text(verbatim: "\(L10n.t(rowLabels[v.vowel] ?? "")) \(pct(v.share))"))
                }
            }
        }.frame(height: 22).clipShape(RoundedRectangle(cornerRadius: 4))
    }

    private func load() async {
        guard let core = app.core else { return }
        busy = true
        do {
            let fresh = try await core.vowels(query)
            guard !Task.isCancelled else { return }
            result = fresh; busy = false
        } catch {
            guard !Task.isCancelled else { return }
            busy = false; app.errorMessage = error.localizedDescription
        }
    }
}

// ────────────────────────────── 单元音词 ──────────────────────────────

@MainActor
private struct UnivocalicSection: View {
    @Environment(AppModel.self) private var app
    let scope: AnalysisScope
    @State private var minMorae = 2
    @State private var vowel: Vowel = .a
    @State private var showAll = false
    @State private var result: VowelResult?
    @State private var busy = false
    private static let pageRows = 50
    private var query: VowelQuery { VowelQuery(scope: scope, minWordMorae: minMorae) }
    private var group: UnivocalicGroup? { result?.univocalic.first { $0.vowel == vowel } }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            heading
            Text("整个词只有一个元音（あなた、永遠 エーエン）。促音、拨音不影响，拍数下限也只数带元音的拍。按段数一数，就能看出只用一个段写歌词时手里有多少词。点词看它出现在哪些歌里。")
                .font(.app(.callout)).foregroundStyle(.secondary)
            if busy && result == nil { Text("计算中…").font(.app(.caption)).foregroundStyle(.secondary) }
            if let result {
                Picker("元音段", selection: $vowel) {
                    ForEach(result.univocalic) { g in
                        Text(verbatim: "\(L10n.t(rowLabels[g.vowel] ?? "")) \(grouped(g.distinctWords))").tag(g.vowel)
                    }
                }.pickerStyle(.segmented).labelsHidden().frame(maxWidth: 460)
            }
            if let group {
                Text("\(grouped(group.distinctWords)) 个词，一共出现 \(grouped(group.tokens)) 次；整行只有这个元音的行 \(grouped(group.lineCount)) 行")
                    .font(.app(.caption)).foregroundStyle(.secondary)
                if group.words.isEmpty { Text("没有").foregroundStyle(.secondary) } else { words(group).opacity(busy ? 0.55 : 1) }
                if !group.lines.isEmpty { lines(group) }
            }
        }
        .onChange(of: vowel) { showAll = false }
        .onChange(of: query) { showAll = false }
        .task(id: AnalysisExtrasLoadKey(query: query, library: app.libraryURL, version: app.library.version)) { await load() }
    }

    private var heading: some View {
        HStack(spacing: 12) {
            Text("单元音词").font(.app(.title2).bold())
            if busy { ProgressView().controlSize(.small) }
            Spacer()
            Picker("至少", selection: $minMorae) {
                ForEach([2, 3, 4], id: \.self) { Text("\($0) 拍").tag($0) }
            }.frame(width: 120)
            AnalysisExtrasExportButton(name: "univocalic.csv", disabled: busy || result == nil, content: csv)
        }.controlSize(.small)
    }

    private func words(_ group: UnivocalicGroup) -> some View {
        let rows = showAll ? group.words : Array(group.words.prefix(Self.pageRows))
        return VStack(alignment: .leading, spacing: 10) {
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 8) {
                GridRow {
                    Text("发音").frame(width: 120, alignment: .leading)
                    Text("写法").frame(minWidth: 160, alignment: .leading)
                    Text("拍").frame(width: 30, alignment: .trailing)
                    Text("次数").frame(width: 55, alignment: .trailing)
                    Text("出现曲数").frame(width: 65, alignment: .trailing)
                }.font(.app(.caption)).foregroundStyle(.secondary)
                ForEach(rows) { w in
                    GridRow {
                        Text(verbatim: w.kana)
                        Button { search(w.lemma) } label: { Text(verbatim: w.surfaces.map(\.word).joined(separator: " / ")) }
                            .buttonStyle(.link)
                            .help("「\(w.lemma)」出现在哪些歌里")
                        Text(String(w.morae)).monospacedDigit().gridColumnAlignment(.trailing)
                        Text(grouped(w.freq)).monospacedDigit().gridColumnAlignment(.trailing)
                        Text(grouped(w.songs)).monospacedDigit().gridColumnAlignment(.trailing)
                    }
                }
            }.font(.app(.callout))
            if group.words.count > Self.pageRows {
                Button(showAll ? "收起" : "显示全部 \(grouped(Int64(group.words.count))) 个") { showAll.toggle() }
                    .buttonStyle(.link).font(.app(.caption))
            }
        }
    }

    private func lines(_ group: UnivocalicGroup) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("单元音行").font(.app(.headline))
            ForEach(group.lines) { line in
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text(verbatim: line.text)
                    Text(verbatim: line.kana).font(.app(.caption)).foregroundStyle(.secondary)
                }
            }
        }.font(.app(.callout))
    }

    private func search(_ lemma: String) {
        app.searchRequest = SearchRequest(keyword: lemma, field: .lemma, scope: scope.subcorpus)
        app.section = .search
    }

    private func csv() -> String {
        let rows = (result?.univocalic ?? []).flatMap { g in
            g.words.map { w in
                [g.vowel.rawValue, w.kana, w.lemma, w.surfaces.map { "\($0.word):\($0.count)" }.joined(separator: " "),
                 String(w.morae), String(w.freq), String(w.songs)]
            }
        }
        return AnalysisExtrasCSV.make(["vowel", "kana", "lemma", "surfaces", "morae", "freq", "songs"], rows: rows)
    }

    private func load() async {
        guard let core = app.core else { return }
        busy = true
        do {
            let fresh = try await core.vowels(query)
            guard !Task.isCancelled else { return }
            result = fresh; busy = false
        } catch {
            guard !Task.isCancelled else { return }
            busy = false; app.errorMessage = error.localizedDescription
        }
    }
}

// ────────────────────────────── 押韵检索 ──────────────────────────────

@MainActor
private struct RhymeSearchSection: View {
    @Environment(AppModel.self) private var app
    let scope: AnalysisScope
    let request: RhymeRequest?
    @State private var draft = ""
    @State private var pattern = ""
    @State private var position: RhymePosition = .end
    @State private var strict = false
    @State private var result: RhymeResult?
    @State private var busy = false
    private var query: RhymeQuery { RhymeQuery(scope: scope, pattern: pattern, position: position, strict: strict) }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
                Text("押韵检索").font(.app(.title2).bold())
                if busy { ProgressView().controlSize(.small) }
                Spacer()
                if let result, !result.pattern.isEmpty {
                    Text("\(grouped(result.lines)) 行 · \(grouped(result.songs)) 首").font(.app(.caption)).foregroundStyle(.secondary)
                }
            }
            Text("按元音找行：输入元音（a i u e o，n 是ン、q 是ッ，. 是任意一个音）或假名，假名按发音转（えいえん 就是 e-e-e）。默认只比元音，ン、ッ 跳过；英文词是断点。")
                .font(.app(.callout)).foregroundStyle(.secondary)
            controls
            if busy && result == nil { Text("计算中…").font(.app(.caption)).foregroundStyle(.secondary) }
            if let result {
                if result.pattern.isEmpty { Text("输入里没有能比的元音").font(.app(.caption)).foregroundStyle(.secondary) }
                else { hits(result).opacity(busy ? 0.55 : 1) }
            }
        }
        .onChange(of: request) { _, request in
            guard let request else { return }
            draft = request.pattern; pattern = request.pattern; position = request.position; strict = request.strict
        }
        .task(id: AnalysisExtrasLoadKey(query: query, library: app.libraryURL, version: app.library.version)) { await load() }
    }

    private var controls: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 14) { field; options }
            VStack(alignment: .leading, spacing: 10) { field; options }
        }.controlSize(.small)
    }

    private var field: some View {
        TextField("如 aiu、ないよ、kimi（回车）", text: $draft)
            .textFieldStyle(.roundedBorder).frame(minWidth: 200, maxWidth: 280)
            .onSubmit { pattern = draft }
            // 删光时旧结果马上清掉
            .onChange(of: draft) { _, value in if value.isEmpty { pattern = "" } }
    }

    private var options: some View {
        HStack(spacing: 14) {
            Picker("押韵位置", selection: $position) {
                Text("行尾（脚韵）").tag(RhymePosition.end)
                Text("行首（头韵）").tag(RhymePosition.start)
                Text("任意位置").tag(RhymePosition.any)
            }.pickerStyle(.segmented).labelsHidden().fixedSize()
            Toggle("ン、ッ 也要对上", isOn: $strict).fixedSize()
        }
    }

    private func hits(_ result: RhymeResult) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("模式：\(showRhymePattern(result.pattern))").font(.app(.caption)).foregroundStyle(.secondary)
            if !result.carriers.isEmpty {
                // 只是列出来，不能点：描边、不填底色
                FlowLayout(spacing: 6) {
                    ForEach(result.carriers.prefix(24)) { c in
                        HStack(spacing: 4) {
                            Text(verbatim: c.text)
                            Text(grouped(c.lines)).foregroundStyle(.secondary)
                        }
                        .font(.app(.caption))
                        .padding(.horizontal, 8).padding(.vertical, 3)
                        .overlay(Capsule().strokeBorder(.separator))
                        .help("\(grouped(c.lines)) 行，\(grouped(c.songs)) 首")
                    }
                }
            }
            if result.hits.isEmpty { Text("没有").foregroundStyle(.secondary) }
            ForEach(result.hits) { h in
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Button { play(h) } label: { Text(rhymeHighlighted(h.text, start: h.charStart, end: h.charEnd)) }
                        .buttonStyle(.plain)
                        .help("打开这首歌，滚到这一行")
                    Text(verbatim: h.kana).font(.app(.caption)).foregroundStyle(.secondary)
                    Spacer(minLength: 12)
                    Text(verbatim: h.artist.isEmpty ? h.title : "\(h.title) — \(h.artist)")
                        .font(.app(.caption)).foregroundStyle(.secondary).lineLimit(1)
                }.font(.app(.callout))
            }
            if result.lines > Int64(result.hits.count) {
                Text("只列前 \(grouped(Int64(result.hits.count))) 行").font(.app(.caption)).foregroundStyle(.secondary)
            }
            ProvenanceFooter(p: result.provenance)
        }
    }

    private func play(_ hit: RhymeHit) {
        Task { await app.player.playFrom(songId: hit.songId, utteranceId: hit.utteranceId, timeSec: hit.timeSec) }
    }

    private func load() async {
        guard !pattern.trimmingCharacters(in: .whitespaces).isEmpty else { result = nil; busy = false; return }
        guard let core = app.core else { return }
        busy = true
        do {
            let fresh = try await core.rhyme(query)
            guard !Task.isCancelled else { return }
            result = fresh; busy = false
        } catch {
            guard !Task.isCancelled else { return }
            busy = false; app.errorMessage = error.localizedDescription
        }
    }
}

// ────────────────────────────── 相邻行押韵 ──────────────────────────────

@MainActor
private struct RhymeStatsSection: View {
    @Environment(AppModel.self) private var app
    let scope: AnalysisScope
    let onSearch: (RhymeRequest) -> Void
    @State private var position: RhymePosition = .end
    @State private var tail = 3
    @State private var strict = false
    @State private var serial = 0
    @State private var result: RhymeStatsResult?
    @State private var busy = false
    private var query: RhymeStatsQuery { RhymeStatsQuery(scope: scope, position: position, strict: strict, tail: tail) }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            heading
            Text("同一首歌里挨着的两行，结尾（或开头）几个元音一样就算押韵；两行文本相同的不算。和同一首歌里随便挑两行比：倍数大于 1，说明挨着的行押韵比碰巧多。")
                .font(.app(.callout)).foregroundStyle(.secondary)
            if busy && result == nil { Text("计算中…").font(.app(.caption)).foregroundStyle(.secondary) }
            if let result {
                Group {
                    lengths(result)
                    if let at = result.lengths.first(where: { $0.length == tail }), at.adjacentRhymed > 0 {
                        Text("押 \(tail) 个音的 \(grouped(at.adjacentRhymed)) 对里，\(grouped(result.sameWord)) 对是同一个词收尾（如都以「ないで」结尾），算重复不算押韵的话要扣掉。")
                            .font(.app(.caption)).foregroundStyle(.secondary)
                    }
                    if !result.tails.isEmpty { tails(result) }
                    if !result.songs.isEmpty { songs(result) }
                }.opacity(busy ? 0.55 : 1)
                ProvenanceFooter(p: result.provenance)
            }
        }
        .task(id: AnalysisExtrasLoadKey(query: query, library: app.libraryURL, version: app.library.version)) { await load() }
    }

    private var heading: some View {
        HStack(spacing: 12) {
            Text("相邻行押韵").font(.app(.title2).bold())
            if busy { ProgressView().controlSize(.small) }
            Spacer()
            Picker("押韵位置", selection: $position) {
                Text("行尾").tag(RhymePosition.end)
                Text("行首").tag(RhymePosition.start)
            }.pickerStyle(.segmented).labelsHidden().fixedSize()
            Picker("韵脚", selection: $tail) {
                ForEach([2, 3, 4], id: \.self) { Text("\($0) 个音").tag($0) }
            }.frame(width: 130)
            Toggle("含ン、ッ", isOn: $strict).fixedSize()
        }.controlSize(.small)
    }

    private func lengths(_ result: RhymeStatsResult) -> some View {
        Grid(alignment: .trailing, horizontalSpacing: 22, verticalSpacing: 6) {
            GridRow {
                Text("音数"); Text("相邻行"); Text("押韵"); Text("随便两行"); Text("倍数")
            }.font(.app(.caption)).foregroundStyle(.secondary)
            ForEach(result.lengths) { l in
                GridRow {
                    Text(String(l.length))
                    Text(grouped(l.adjacent))
                    Text(verbatim: "\(pct(l.adjacentRate)) (\(grouped(l.adjacentRhymed)))")
                    Text(pct(l.baselineRate))
                    Text(verbatim: l.lift.map { String(format: "×%.2f", $0) } ?? "—")
                }
                .fontWeight(l.length == tail ? .semibold : .regular)
                .foregroundStyle(l.length == tail ? Color.accentColor : Color.primary)
            }
        }.font(.app(.callout)).monospacedDigit()
    }

    private func tails(_ result: RhymeStatsResult) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("常见韵脚").font(.app(.headline))
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 6) {
                GridRow {
                    Text("韵脚"); Text("对数"); Text("出现曲数"); Text("同词"); Text("例子")
                }.font(.app(.caption)).foregroundStyle(.secondary)
                ForEach(result.tails.prefix(20)) { t in
                    GridRow {
                        Button {
                            serial += 1
                            onSearch(RhymeRequest(pattern: t.tail, position: position == .start ? .start : .end, strict: strict, serial: serial))
                        } label: { Text(verbatim: showRhymePattern(t.tail)) }.buttonStyle(.link).help("检索押这个韵的行")
                        Text(grouped(t.pairs)).monospacedDigit().gridColumnAlignment(.trailing)
                        Text(grouped(t.songs)).monospacedDigit().gridColumnAlignment(.trailing)
                        Text(grouped(t.sameWord)).monospacedDigit().gridColumnAlignment(.trailing)
                        Button { open(t.example.songId) } label: {
                            Text(verbatim: "\(t.example.first) / \(t.example.second)").lineLimit(1)
                        }.buttonStyle(.link)
                    }
                }
            }.font(.app(.callout))
        }
    }

    private func songs(_ result: RhymeStatsResult) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("押韵最密的歌").font(.app(.headline))
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 6) {
                GridRow {
                    Text("歌曲"); Text("相邻行"); Text("押韵"); Text("比例")
                }.font(.app(.caption)).foregroundStyle(.secondary)
                ForEach(result.songs.prefix(20)) { s in
                    GridRow {
                        HStack(spacing: 8) {
                            Button { open(s.songId) } label: { Text(verbatim: s.title) }.buttonStyle(.link)
                            Text(verbatim: s.artist).font(.app(.caption)).foregroundStyle(.secondary)
                        }
                        Text(grouped(s.pairs)).monospacedDigit().gridColumnAlignment(.trailing)
                        Text(grouped(s.rhymed)).monospacedDigit().gridColumnAlignment(.trailing)
                        Text(pct(s.rate)).monospacedDigit().gridColumnAlignment(.trailing)
                    }
                }
            }.font(.app(.callout))
        }
    }

    private func open(_ songId: String) {
        Task { await app.play(songId: songId, source: "analysis") }
    }

    private func load() async {
        guard let core = app.core else { return }
        busy = true
        do {
            let fresh = try await core.rhymeStats(query)
            guard !Task.isCancelled else { return }
            result = fresh; busy = false
        } catch {
            guard !Task.isCancelled else { return }
            busy = false; app.errorMessage = error.localizedDescription
        }
    }
}
