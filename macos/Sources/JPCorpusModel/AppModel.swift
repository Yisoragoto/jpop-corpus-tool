import Foundation
import JPCore
import Observation

public enum SidebarItem: String, CaseIterable, Identifiable, Hashable {
    case home, search, library, analysis, dictionaries, scrape, importer
    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .home: return "首页"
        case .search: return "检索"
        case .library: return "曲库"
        case .analysis: return "分析"
        case .dictionaries: return "词典"
        case .scrape: return "刮削"
        case .importer: return "导入"
        }
    }

    public var symbol: String {
        switch self {
        case .home: return "house"
        case .search: return "text.magnifyingglass"
        case .library: return "music.note.list"
        case .analysis: return "chart.bar.xaxis"
        case .dictionaries: return "character.book.closed"
        case .scrape: return "wand.and.stars"
        case .importer: return "square.and.arrow.down"
        }
    }
}

/// 曲库页里能推进去的详情页。首页、全库搜索、歌词面板也往这里推
public enum LibraryRoute: Hashable {
    case song(String)
    case album(Album)
    case person(Int64)
}

/// 别处要求分析页只看一部分歌（歌手页的「分析这个人的歌」）
public struct ScopeRequest: Equatable {
    public var name: String
    public var performerIds: [Int64]
    public var nonce = UUID()

    public init(name: String, performerIds: [Int64]) {
        self.name = name
        self.performerIds = performerIds
    }
}

/// 别处要求检索页查一个词（分析页、歌词面板）。nonce 让同一个词点两次也会再查
public struct SearchRequest: Equatable {
    public var keyword: String
    public var field: MatchField
    public var nonce = UUID()

    public init(keyword: String, field: MatchField) {
        self.keyword = keyword
        self.field = field
    }
}

@MainActor
@Observable
public final class AppModel {
    public private(set) var client: (any CorpusCalling)?
    public private(set) var libraryURL: URL?
    public private(set) var tracks: [Track] = []
    private var tracksById: [String: Track] = [:]
    public var section: SidebarItem = .home
    /// 曲库页的导航栈
    public var libraryPath: [LibraryRoute] = []
    /// 分析页的范围（nil = 整个语料库）
    public var analysisScope: ScopeRequest?
    /// ⌘K 全库搜索
    public var showQuickSearch = false
    /// 全屏歌词
    public var showStage = false
    public var stageKaraoke = false
    public var lyricsDisplay = LyricsDisplay() {
        didSet { if let data = try? JSONEncoder().encode(lyricsDisplay) { defaults.set(data, forKey: "lyrics-display") } }
    }
    public var importedLyricFonts: [String] = [] {
        didSet { defaults.set(importedLyricFonts, forKey: "lyrics-imported-fonts") }
    }

    /// 振假名开关和注法。存在偏好里，歌词面板和全屏歌词共用
    public var showFurigana: Bool {
        didSet { defaults.set(showFurigana, forKey: Keys.furigana) }
    }
    public var furiganaMode: FuriganaMode {
        didSet { defaults.set(furiganaMode.rawValue, forKey: Keys.furiganaMode) }
    }
    /// 当前这首每行的注音（utteranceId → 注音段）。开关关着、或者没有分词词典时是空的
    public private(set) var furigana: [Int64: [Ruby]] = [:]
    /// 振假名取不到的原因（多半是没有分词词典）
    public private(set) var furiganaError: String?

    /// 词典页的「试查」预先填进去的字（冒烟截图用）
    public var lookupSeed = ""
    /// 词典库被别处改过（冒烟导入），词典页据此重读
    public var dictionaryVersion = 0
    /// 打开 ⌘K 时预先填进去的字（冒烟截图用）
    public var quickSearchSeed = ""
    /// 收藏、播放记录变了就加一，首页据此重读
    public var homeVersion = 0
    public var showLyrics = true
    public var searchRequest: SearchRequest?
    public var errorMessage: String?
    public private(set) var isDemo = false
    public var pendingCreationURL: URL?
    public let player: PlayerModel
    public var stageCoverBlur: Bool {
        didSet { defaults.set(stageCoverBlur, forKey: "player-stage-cover-blur") }
    }
    public var stageBlurRadius: Double {
        didSet { defaults.set(stageBlurRadius, forKey: "player-stage-blur-radius") }
    }
    public var karaokeLyrics: Bool {
        didSet { defaults.set(karaokeLyrics, forKey: "player-karaoke-lyrics") }
    }
    /// Shared with ImportView so smoke exercises the model actually displayed in the window.
    public private(set) var importer = ImportModel()

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let openClient: @Sendable (URL) async throws -> any CorpusCalling

    public var audioRoots: [String] {
        didSet {
            defaults.set(audioRoots, forKey: Keys.audioRoots)
            let roots = audioRoots
            let current = client
            Task {
                do { try await current?.setAudioRoots(roots) }
                catch { errorMessage = error.localizedDescription }
            }
        }
    }

    private enum Keys {
        static let library = "libraryFolder"
        static let audioRoots = "audioRoots"
        static let furigana = "furigana"
        static let furiganaMode = "furiganaMode"
    }

    /// 具体的 Rust 客户端。首页、词典、刮削、分析存档这些只有真库才有的功能走它；
    /// 能替换成测试替身的那部分走 `client`（`CorpusCalling`）
    public var core: CorpusClient? { client as? CorpusClient }

    public init(
        defaults: UserDefaults = .standard,
        client: (any CorpusCalling)? = nil,
        playbackEngine: (any PlaybackEngine)? = nil,
        openClient: @escaping @Sendable (URL) async throws -> any CorpusCalling = { db in
            let core = try await Task.detached { try CorpusCore(databaseURL: db) }.value
            return CorpusClient(core: core)
        }
    ) {
        self.defaults = defaults
        self.client = client
        self.openClient = openClient
        player = PlayerModel(engine: playbackEngine, defaults: defaults)
        stageCoverBlur = defaults.object(forKey: "player-stage-cover-blur") as? Bool ?? true
        let blur = defaults.object(forKey: "player-stage-blur-radius") as? NSNumber
        let radius = blur?.doubleValue ?? 64
        stageBlurRadius = radius.isFinite ? min(120, max(20, radius)) : 64
        karaokeLyrics = defaults.object(forKey: "player-karaoke-lyrics") as? Bool ?? true
        audioRoots = defaults.stringArray(forKey: Keys.audioRoots) ?? []
        showFurigana = defaults.bool(forKey: Keys.furigana)
        furiganaMode = FuriganaMode(rawValue: defaults.string(forKey: Keys.furiganaMode) ?? "") ?? .kanji
        if let data = defaults.data(forKey: "lyrics-display"), var display = try? JSONDecoder().decode(LyricsDisplay.self, from: data) {
            display.normalize(); lyricsDisplay = display
        }
        importedLyricFonts = defaults.stringArray(forKey: "lyrics-imported-fonts") ?? []
        player.app = self
    }

    public static var defaultLibraryFolder: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("JPOP Corpus Tool", isDirectory: true)
    }

    public func restore() async {
        if let saved = defaults.string(forKey: Keys.library), Self.hasDatabase(in: URL(fileURLWithPath: saved)) {
            await openExisting(folder: URL(fileURLWithPath: saved, isDirectory: true))
        } else if Self.hasDatabase(in: Self.defaultLibraryFolder) {
            await openExisting(folder: Self.defaultLibraryFolder)
        }
    }

    private static func hasDatabase(in folder: URL) -> Bool {
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(atPath: folder.appendingPathComponent("corpus.db").path, isDirectory: &isDirectory)
            && !isDirectory.boolValue
    }

    /// Selecting a non-library folder requests confirmation; only the explicit create action creates it.
    public func selectLibrary(folder: URL) async {
        errorMessage = nil
        if Self.hasDatabase(in: folder) {
            pendingCreationURL = nil
            await openExisting(folder: folder)
        } else {
            pendingCreationURL = folder
        }
    }

    /// The alert action passes its captured URL: dismissal may already have cleared the pending prompt.
    public func confirmCreation(in folder: URL) async {
        pendingCreationURL = nil
        await createOrOpen(folder: folder)
        if errorMessage == nil { section = .importer }
    }

    public func cancelCreation() { pendingCreationURL = nil }

    public func openExisting(folder: URL) async {
        guard Self.hasDatabase(in: folder) else {
            errorMessage = "这个文件夹没有 corpus.db：请选择已有的语料库，或确认新建一个库。"
            return
        }
        await createOrOpen(folder: folder)
    }

    /// Explicit create-or-open semantics, used for the default folder and confirmed custom folders.
    public func createOrOpen(folder: URL) async {
        errorMessage = nil
        let access = folder.startAccessingSecurityScopedResource()
        defer { if access { folder.stopAccessingSecurityScopedResource() } }
        do {
            let next = try await openClient(folder.appendingPathComponent("corpus.db"))
            try await next.setAudioRoots(audioRoots)
            let fetched = try await next.tracks()
            client = next
            libraryURL = folder
            importer = ImportModel()
            updateTracks(fetched)
            isDemo = false
            defaults.set(folder.path, forKey: Keys.library)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    public func openDemo() async {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("JPCorpusDemo-\(UUID().uuidString)", isDirectory: true)
        do {
            let db = try await Task.detached { try CorpusCore.writeFixture(in: dir) }.value
            await openExisting(folder: db.deletingLastPathComponent())
            if errorMessage == nil {
                isDemo = true
                defaults.removeObject(forKey: Keys.library)
            }
        } catch { errorMessage = error.localizedDescription }
    }

    public func createLibrary() async {
        await createOrOpen(folder: Self.defaultLibraryFolder)
        if errorMessage == nil { section = .importer }
    }

    public func reloadTracks() async {
        guard let client else { return }
        do { updateTracks(try await client.tracks()) }
        catch { errorMessage = error.localizedDescription }
    }

    private func updateTracks(_ tracks: [Track]) {
        self.tracks = tracks
        tracksById = Dictionary(tracks.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
    }

    public func track(_ id: String) async throws -> Track? {
        if let known = tracksById[id] { return known }
        return try await client?.track(id)
    }

    /// 读当前这首的振假名。换歌、开关、换注法时调（ContentView 里按这三样做 task 的 id）
    public func loadFurigana() async {
        furigana = [:]
        furiganaError = nil
        guard showFurigana, let songId = player.track?.id, let core else { return }
        do {
            let lines = try await core.lyricsFurigana(songId, mode: furiganaMode)
            furigana = Dictionary(lines.map { ($0.utteranceId, $0.rubies) }, uniquingKeysWith: { a, _ in a })
        } catch {
            furiganaError = error.localizedDescription
        }
    }

    /// 去曲库页的某个详情页（从别处来时先清掉原来的栈）
    public func show(_ route: LibraryRoute) {
        if section != .library {
            libraryPath = []
            section = .library
        }
        if libraryPath.last != route { libraryPath.append(route) }
    }

    /// 按 id 放一首歌（首页、搜索结果只有 id）
    public func play(songId: String, at: Double? = nil, source: String = "library") async {
        do {
            guard let track = try await track(songId) else { return }
            showLyrics = true
            await player.open(track, at: at, autoplay: true, source: source)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    /// 跳到检索页查一个词
    public func search(_ keyword: String, field: MatchField = .lemma) {
        searchRequest = SearchRequest(keyword: keyword, field: field)
        section = .search
    }
}
