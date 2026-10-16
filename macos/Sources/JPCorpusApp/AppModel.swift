import Foundation
import JPCore
import Observation

enum SidebarItem: String, CaseIterable, Identifiable, Hashable {
    case search, library, analysis, importer
    var id: String { rawValue }

    var title: String {
        switch self {
        case .search: return "检索"
        case .library: return "曲库"
        case .analysis: return "分析"
        case .importer: return "导入"
        }
    }

    var symbol: String {
        switch self {
        case .search: return "text.magnifyingglass"
        case .library: return "music.note.list"
        case .analysis: return "chart.bar.xaxis"
        case .importer: return "square.and.arrow.down"
        }
    }
}

/// 别处要求检索页查一个词（分析页、歌词面板）。nonce 让同一个词点两次也会再查
struct SearchRequest: Equatable {
    var keyword: String
    var field: MatchField
    var nonce = UUID()
}

/// 整个应用的状态：打开的是哪个库、曲目表、当前在哪一页。
@MainActor
@Observable
final class AppModel {
    private(set) var client: CorpusClient?
    private(set) var libraryURL: URL?
    private(set) var tracks: [Track] = []
    private var tracksById: [String: Track] = [:]
    var section: SidebarItem = .search
    var showLyrics = true
    var searchRequest: SearchRequest?
    var errorMessage: String?
    var isDemo = false

    let player = PlayerModel()

    /// 额外的音频目录。库从 Windows 拷来时，库里记的是 `E:\...` 这种路径，要按文件名到这些目录里找
    var audioRoots: [String] = UserDefaults.standard.stringArray(forKey: Keys.audioRoots) ?? [] {
        didSet {
            UserDefaults.standard.set(audioRoots, forKey: Keys.audioRoots)
            Task { try? await client?.setAudioRoots(audioRoots) }
        }
    }

    private enum Keys {
        static let library = "libraryFolder"
        static let audioRoots = "audioRoots"
    }

    init() {
        player.app = self
    }

    /// 默认语料库目录，和 Windows 版的 `%LOCALAPPDATA%\JPOP Corpus Tool` 对应
    static var defaultLibraryFolder: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("JPOP Corpus Tool", isDirectory: true)
    }

    /// 启动时打开上次的库；没有就看默认位置有没有
    func restore() async {
        if let saved = UserDefaults.standard.string(forKey: Keys.library) {
            let url = URL(fileURLWithPath: saved, isDirectory: true)
            if FileManager.default.fileExists(atPath: url.appendingPathComponent("corpus.db").path) {
                await open(folder: url)
                return
            }
        }
        let fallback = Self.defaultLibraryFolder
        if FileManager.default.fileExists(atPath: fallback.appendingPathComponent("corpus.db").path) {
            await open(folder: fallback)
        }
    }

    /// 打开一个语料库目录（里面有 corpus.db）。Windows 版的库目录直接拷过来就能用
    func open(folder: URL) async {
        let db = folder.appendingPathComponent("corpus.db")
        do {
            let core = try await Task.detached { try CorpusCore(databaseURL: db) }.value
            let client = CorpusClient(core: core)
            try await client.setAudioRoots(audioRoots)
            let tracks = try await client.tracks()
            self.client = client
            self.libraryURL = folder
            self.tracks = tracks
            self.tracksById = Dictionary(tracks.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
            isDemo = false
            UserDefaults.standard.set(folder.path, forKey: Keys.library)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    /// 没有库时先看看效果：四首歌的合成语料库（没有音频，检索和分析都能用）
    func openDemo() async {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("JPCorpusDemo", isDirectory: true)
        do {
            try? FileManager.default.removeItem(at: dir)
            let db = try CorpusCore.writeFixture(in: dir)
            await open(folder: db.deletingLastPathComponent())
            isDemo = true
            UserDefaults.standard.removeObject(forKey: Keys.library)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    /// 在默认位置新建（或打开已有的）语料库，然后去导入页。
    /// 打开一个不存在的 corpus.db 就是建库：Rust 那边建目录、建表、打版本戳，和 Windows 版第一次启动一样
    func createLibrary() async {
        await open(folder: Self.defaultLibraryFolder)
        if client != nil { section = .importer }
    }

    /// 导入之后重新读曲目表
    func reloadTracks() async {
        guard let client else { return }
        do {
            let tracks = try await client.tracks()
            self.tracks = tracks
            self.tracksById = Dictionary(tracks.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func track(_ id: String) async throws -> Track? {
        if let known = tracksById[id] { return known }
        return try await client?.track(id)
    }

    /// 跳到检索页查一个词
    func search(_ keyword: String, field: MatchField = .lemma) {
        searchRequest = SearchRequest(keyword: keyword, field: field)
        section = .search
    }
}
