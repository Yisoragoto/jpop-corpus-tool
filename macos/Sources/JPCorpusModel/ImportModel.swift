import Foundation
import JPCore
import Observation

/// 导入页的状态：分词词典在不在、扫出来的计划、正在跑的导入。
///
/// 扫描、计划、写库、分词都在 Rust（jp-import / jp-tokenizer，和 Windows 版同一份代码）；
/// 这里只选文件、展示计划、隔一会儿问一次进度。
@MainActor
@Observable
public final class ImportModel {
    public private(set) var tokenizer: TokenizerStatus?
    public private(set) var untokenized = 0
    public private(set) var dictionaryJob: JobProgress<DictionaryReady>?
    public private(set) var dictionaryMessage: String?

    public private(set) var sources: [URL] = []
    public private(set) var scanning = false
    public private(set) var plan: ScanResult?
    public var tokenize = true

    public private(set) var importJob: JobProgress<ImportResult>?
    public private(set) var lastResult: ImportResult?
    public var errorMessage: String?

    public var importing: Bool { importJob?.running == true }
    public var downloading: Bool { dictionaryJob?.running == true }

    public func refresh(_ client: any CorpusCalling) async {
        do {
            tokenizer = try await client.tokenizerStatus()
            untokenized = try await client.untokenizedCount()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    public static let noLibrary = "还没有打开语料库：先在欢迎页新建或选择一个库"

    public init() {}

    /// The view and smoke runner use this same completion route.
    public func handleSelection(_ result: Result<[URL], Error>, kind: ImportPickKind?, client: (any CorpusCalling)?) async {
        switch result {
        case .failure(let error):
            errorMessage = error.localizedDescription
        case .success(let urls):
            guard let client else {
                errorMessage = Self.noLibrary
                return
            }
            guard let kind else {
                errorMessage = "文件选择的用途已丢失，请重新选择"
                return
            }
            switch kind {
            case .dictionary:
                guard let first = urls.first else { return }
                await installDictionary(from: first, client: client)
            case .folders, .files:
                await scan(urls, client: client)
            }
        }
    }

    // MARK: 扫描

    public func scan(_ urls: [URL], client: any CorpusCalling) async {
        guard !urls.isEmpty else { return }
        sources = urls
        scanning = true
        defer { scanning = false }
        lastResult = nil
        plan = nil
        errorMessage = nil
        let paths = urls.map(\.path)
        do {
            let access = urls.map { $0.startAccessingSecurityScopedResource() }
            defer { for (url, ok) in zip(urls, access) where ok { url.stopAccessingSecurityScopedResource() } }
            let result = try await client.scanFiles(paths)
            plan = result
            tokenize = result.tokenizerReady
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    // MARK: 导入

    public func runImport(app: AppModel) async {
        guard let client = app.client else {
            errorMessage = Self.noLibrary
            return
        }
        lastResult = nil
        do {
            _ = try await client.runImport(tokenize: tokenize)
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
            if let problem = LibraryAdmin.csvProblem(lastResult?.csv) { errorMessage = problem }
            plan = nil
            await app.reloadTracks()
            app.library.version += 1
            app.homeVersion += 1
            await app.library.checkNewAudio()
            await refresh(client)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    public func cancel(_ client: any CorpusCalling) {
        Task {
            do { try await client.cancelImport() }
            catch { errorMessage = error.localizedDescription }
        }
    }

    // MARK: 分词词典

    public func downloadDictionary(app: AppModel) async {
        guard let client = app.client else {
            errorMessage = Self.noLibrary
            return
        }
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

    public func installDictionary(from url: URL, client: any CorpusCalling) async {
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

    public func tokenizeMissing(_ client: any CorpusCalling) async {
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
        let note = InterfaceLogic.backfillNote(songs: ready.tokenized.songs,tokens: ready.tokenized.tokens,error: ready.tokenizeError)
        if !note.isEmpty { text += "；" + note }
        return text
    }
}
