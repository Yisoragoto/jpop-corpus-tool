import JPCore
import Observation
import SwiftUI
import UniformTypeIdentifiers

/// 导入页的状态：分词词典在不在、扫出来的计划、正在跑的导入。
///
/// 扫描、计划、写库、分词都在 Rust（jp-import / jp-tokenizer，和 Windows 版同一份代码）；
/// 这里只选文件、展示计划、隔一会儿问一次进度。
@MainActor
@Observable
final class ImportModel {
    var tokenizer: TokenizerStatus?
    var untokenized = 0
    var dictionaryJob: JobProgress<DictionaryReady>?
    var dictionaryMessage: String?

    var sources: [URL] = []
    var scanning = false
    var plan: ScanResult?
    var tokenize = true

    var importJob: JobProgress<ImportResult>?
    var lastResult: ImportResult?
    var errorMessage: String?

    var importing: Bool { importJob?.running == true }
    var downloading: Bool { dictionaryJob?.running == true }

    func refresh(_ client: CorpusClient) async {
        do {
            tokenizer = try await client.tokenizerStatus()
            untokenized = try await client.untokenizedCount()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    // MARK: 扫描

    func scan(_ urls: [URL], client: CorpusClient) async {
        guard !urls.isEmpty else { return }
        sources = urls
        scanning = true
        defer { scanning = false }
        lastResult = nil
        let paths = urls.map(\.path)
        do {
            let access = urls.map { $0.startAccessingSecurityScopedResource() }
            defer { for (url, ok) in zip(urls, access) where ok { url.stopAccessingSecurityScopedResource() } }
            let result = paths.count == 1 ? try await client.scanFolder(paths[0]) : try await client.scanFiles(paths)
            plan = result
            tokenize = result.tokenizerReady
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    // MARK: 导入

    func runImport(app: AppModel) async {
        guard let client = app.client else { return }
        lastResult = nil
        do {
            try await client.runImport(tokenize: tokenize)
            while true {
                let progress = try await client.importProgress()
                importJob = progress
                if progress.finished { break }
                try await Task.sleep(for: .milliseconds(250))
            }
            if let error = importJob?.error {
                errorMessage = error
            }
            lastResult = importJob?.result
            plan = nil
            await app.reloadTracks()
            await refresh(client)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func cancel(_ client: CorpusClient) {
        Task { try? await client.cancelImport() }
    }

    // MARK: 分词词典

    func downloadDictionary(app: AppModel) async {
        guard let client = app.client else { return }
        dictionaryMessage = nil
        do {
            try await client.tokenizerDownload()
            while true {
                let progress = try await client.tokenizerProgress()
                dictionaryJob = progress
                if progress.finished { break }
                try await Task.sleep(for: .milliseconds(300))
            }
            if let error = dictionaryJob?.error {
                errorMessage = error
            } else if let ready = dictionaryJob?.result {
                dictionaryMessage = Self.describe(ready)
            }
            await refresh(client)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func installDictionary(from url: URL, client: CorpusClient) async {
        dictionaryMessage = "正在复制词典（约 207 MB）…"
        let ok = url.startAccessingSecurityScopedResource()
        defer { if ok { url.stopAccessingSecurityScopedResource() } }
        do {
            let ready = try await client.tokenizerInstall(from: url.path)
            dictionaryMessage = Self.describe(ready)
        } catch {
            dictionaryMessage = nil
            errorMessage = error.localizedDescription
        }
        await refresh(client)
    }

    func tokenizeMissing(_ client: CorpusClient) async {
        do {
            let done = try await client.tokenizeMissing()
            dictionaryMessage = "补上了 \(done.songs) 首歌的分词（\(done.tokens) 个词）"
        } catch {
            errorMessage = error.localizedDescription
        }
        await refresh(client)
    }

    private static func describe(_ ready: DictionaryReady) -> String {
        var text = "词典装好了：\(ready.dir)"
        if ready.tokenized.songs > 0 {
            text += "；顺手补上了 \(ready.tokenized.songs) 首歌的分词"
        }
        if !ready.tokenizeError.isEmpty {
            text += "；补分词失败：\(ready.tokenizeError)"
        }
        return text
    }
}

/// 导入页：选音频 / 歌词所在的文件夹 → 看计划 → 导入（带进度、可以停）
@MainActor
struct ImportView: View {
    @Environment(AppModel.self) private var app
    @State private var model = ImportModel()
    @State private var pickingFolders = false
    @State private var pickingFiles = false
    @State private var pickingDictionary = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                TokenizerCard(model: model, pickingDictionary: $pickingDictionary)
                sourceBar
                if let job = model.importJob, job.running {
                    progress(job)
                } else if let result = model.lastResult {
                    ImportResultView(result: result)
                }
            }
            .padding(16)
            Divider()
            if let plan = model.plan {
                PlanTable(plan: plan)
            } else {
                ContentUnavailableView(
                    "选择音乐文件夹",
                    systemImage: "square.and.arrow.down.on.square",
                    description: Text("音频（FLAC、MP3、M4A、WAV…）旁边放同名的 .lrc，歌词会一起导进来；\n音频里内嵌的歌词也认。先看计划，确认了才写库。")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        // 顶部对齐：不加的话整列在窗口里垂直居中，词典提示和按钮掉到中间，上面空一大块
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .navigationTitle("导入")
        .navigationSubtitle(app.libraryURL?.path ?? "")
        .task { if let client = app.client { await model.refresh(client) } }
        .fileImporter(isPresented: $pickingFolders, allowedContentTypes: [.folder], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result, let client = app.client {
                Task { await model.scan(urls, client: client) }
            }
        }
        .fileImporter(isPresented: $pickingFiles, allowedContentTypes: [.audio], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result, let client = app.client {
                Task { await model.scan(urls, client: client) }
            }
        }
        .fileImporter(isPresented: $pickingDictionary, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result, let client = app.client {
                Task { await model.installDictionary(from: url, client: client) }
            }
        }
        .alert("出错了", isPresented: Binding(get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })) {
            Button("好") { model.errorMessage = nil }
        } message: {
            Text(model.errorMessage ?? "")
        }
    }

    private var sourceBar: some View {
        HStack(spacing: 10) {
            Button("选择文件夹…") { pickingFolders = true }
                .buttonStyle(.borderedProminent)
            Button("选择文件…") { pickingFiles = true }
            if model.scanning {
                ProgressView().controlSize(.small)
                Text("正在扫描…").foregroundStyle(.secondary)
            } else if !model.sources.isEmpty {
                Text(model.sources.map(\.lastPathComponent).joined(separator: "、"))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if let plan = model.plan {
                Toggle("同时分词", isOn: Binding(get: { model.tokenize }, set: { model.tokenize = $0 }))
                    .disabled(!plan.tokenizerReady)
                    .help(plan.tokenizerReady ? "分词后才能检索、做分析" : "没有分词词典：先在上面下载一份，或者先导入、之后再补分词")
                Button("导入 \(plan.summary.new) 首") {
                    Task { await model.runImport(app: app) }
                }
                .buttonStyle(.borderedProminent)
                .disabled(plan.summary.new == 0 || model.importing || model.scanning)
            }
        }
    }

    private func progress(_ job: JobProgress<ImportResult>) -> some View {
        HStack(spacing: 12) {
            ProgressView(value: job.fraction ?? 0)
                .frame(maxWidth: 360)
            Text("\(job.done) / \(job.total)").monospacedDigit().foregroundStyle(.secondary)
            Text(job.stage).lineLimit(1).foregroundStyle(.secondary)
            Spacer()
            Button("停止") { if let client = app.client { model.cancel(client) } }
                .help("停在当前这首之后。已经导进去的不会撤回")
        }
    }
}

/// 分词词典那一行：没有就提供下载 / 复制，有就说在哪
@MainActor
private struct TokenizerCard: View {
    @Environment(AppModel.self) private var app
    let model: ImportModel
    @Binding var pickingDictionary: Bool

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 6) {
                if let status = model.tokenizer {
                    if status.ready {
                        Label("分词词典已就绪", systemImage: "checkmark.seal")
                            .foregroundStyle(.green)
                        Text(status.dictPath ?? "").font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                        if model.untokenized > 0 {
                            HStack {
                                Text("有 \(model.untokenized) 首歌还没有分词（查不到、也不进统计）")
                                Button("补上分词") {
                                    if let client = app.client { Task { await model.tokenizeMissing(client) } }
                                }
                            }
                        }
                    } else {
                        Label("还没有分词词典（Sudachi）", systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.orange)
                        Text("没有它也能导入，但歌词不会分词：检索、词频、搭配都查不到这些歌。词典约 43 MB，下载后解开 207 MB，放在 \(status.bundledDir)。")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                        HStack {
                            Button("下载词典（43 MB）") { Task { await model.downloadDictionary(app: app) } }
                                .disabled(model.downloading)
                            Button("从文件夹复制…") { pickingDictionary = true }
                                .disabled(model.downloading)
                                .help("已经有一份的话（另一个语料库目录的 sudachi/，或 0.1.x 的项目目录），直接复制，不用下载")
                        }
                    }
                } else {
                    ProgressView().controlSize(.small)
                }
                if let job = model.dictionaryJob, job.running {
                    HStack {
                        ProgressView(value: job.fraction ?? 0).frame(maxWidth: 320)
                        Text(Self.stage(job)).font(.caption).monospacedDigit().foregroundStyle(.secondary)
                    }
                }
                if let message = model.dictionaryMessage {
                    Text(message).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private static func stage(_ job: JobProgress<DictionaryReady>) -> String {
        switch job.stage {
        case "downloading":
            let mb = { (b: Int64) in String(format: "%.1f", Double(b) / 1_048_576) }
            return "下载中 \(mb(job.done)) / \(mb(job.total)) MB"
        case "extracting": return "解压中…"
        case "tokenizing": return "给已有的歌补分词…"
        default: return job.stage
        }
    }
}

/// 计划：每个文件会怎么处置
@MainActor
private struct PlanTable: View {
    let plan: ScanResult

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(summary)
                .font(.callout)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
            Table(plan.items) {
                TableColumn("处置") { item in
                    Label(Self.label(item.action), systemImage: item.action.writes ? "plus.circle" : "minus.circle")
                        .foregroundStyle(item.action.writes ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
                        .help(item.action.reason ?? item.action.existingPath ?? "")
                }
                .width(min: 90, ideal: 110)
                TableColumn("曲名", value: \.title)
                TableColumn("歌手") { item in
                    HStack(spacing: 4) {
                        Text(item.artist)
                        if item.artistSource == "folder" {
                            Image(systemName: "questionmark.folder")
                                .foregroundStyle(.orange)
                                .help("歌手是按文件夹名猜的，导入后可以再改")
                        }
                    }
                }
                TableColumn("专辑", value: \.album)
                TableColumn("歌词") { item in
                    Image(systemName: item.hasLyrics ? "text.quote" : "minus")
                        .foregroundStyle(item.hasLyrics ? AnyShapeStyle(.primary) : AnyShapeStyle(.tertiary))
                }
                .width(40)
                TableColumn("文件") { item in
                    HStack(spacing: 4) {
                        if let warning = item.warning {
                            Image(systemName: "exclamationmark.triangle").foregroundStyle(.orange).help(warning)
                        }
                        Text(item.fileName).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    }
                }
            }
            .clearScrollBackground()
            .alternatingRowBackgrounds(.disabled)
        }
    }

    private var summary: String {
        let s = plan.summary
        var parts = ["共 \(s.total) 个音频", "新歌 \(s.new) 首"]
        if s.alreadyImported > 0 { parts.append("已在库中 \(s.alreadyImported)") }
        if s.possibleDuplicates > 0 { parts.append("可能重复 \(s.possibleDuplicates)（不导）") }
        if s.duplicatesInBatch > 0 { parts.append("本批重复 \(s.duplicatesInBatch)") }
        if s.skipped > 0 { parts.append("导不了 \(s.skipped)") }
        let withLyrics = plan.items.filter { $0.action.writes && $0.hasLyrics }.count
        parts.append("其中有歌词 \(withLyrics)")
        if !plan.ignored.isEmpty { parts.append("不是音频、没算进去：\(plan.ignored.joined(separator: "、"))") }
        return parts.joined(separator: " · ")
    }

    static func label(_ action: PlanAction) -> String {
        switch action.kind {
        case .new: return "新建"
        case .alreadyImported: return "已在库中"
        case .possibleDuplicate: return "可能重复"
        case .duplicateInBatch: return "本批重复"
        case .skipped: return "跳过"
        }
    }
}

/// 一次导入的结果
@MainActor
private struct ImportResultView: View {
    let result: ImportResult

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Label(
                (result.cancelled ? "已停止：" : "导入完成：")
                    + "\(result.imported) 首、\(result.lyricLines) 行歌词、\(result.tokens) 个词"
                    + (result.failed > 0 ? "；失败 \(result.failed) 首" : ""),
                systemImage: result.failed > 0 ? "exclamationmark.circle" : "checkmark.circle"
            )
            if result.imported > 0 && result.tokens == 0 {
                Text("这批没有分词：装好分词词典后点「补上分词」，检索和分析才看得到它们。")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
            ForEach(result.tracks.filter { $0.outcome.kind == "failed" }, id: \.path) { track in
                Text("\(track.title)：\(track.outcome.error ?? "")")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
        }
    }
}
