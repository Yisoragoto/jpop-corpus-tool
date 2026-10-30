import Foundation
import JPCore
import Observation

public enum SidebarItem: String, CaseIterable, Identifiable, Hashable {
    case home, search, library, analysis, importer, scrape, dictionaries, anki, settings
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
        case .settings: return "设置"
        case .anki: return "Anki"
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
        case .settings: return "gearshape"
        case .anki: return "rectangle.on.rectangle"
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
    public var scope: Subcorpus?
    public var pos: String?
    public var nonce = UUID()

    public init(keyword: String, field: MatchField, scope: Subcorpus? = nil, pos: String? = nil) {
        self.keyword = keyword
        self.field = field
        self.scope = scope; self.pos = pos
    }
}

@MainActor
@Observable
public final class AppModel {
    public private(set) var client: (any CorpusCalling)?
    public private(set) var libraryURL: URL?
    /// 库文件夹里的数据库文件名。一般是 corpus.db；直接选了别的名字的 .db（比如拷过来时改了名）就用那个
    public private(set) var databaseName = AppModel.defaultDatabaseName
    nonisolated public static let defaultDatabaseName = "corpus.db"
    public private(set) var tracks: [Track] = []
    private var tracksById: [String: Track] = [:]
    public var section: SidebarItem = .home
    /// 曲库页的导航栈
    public var libraryPath: [LibraryRoute] = []
    /// 分析页的范围（nil = 整个语料库）
    public var analysisScope: ScopeRequest?
    /// ⌘K 全库搜索
    public var showQuickSearch = false
    public var paletteCommands = false
    public let scopes: SubcorpusStore
    public let visualizations = VisualizationModel()
    public let anki = AnkiModel()
    public let updates: UpdateModel
    public var libraryMode: LibraryMode = .songs { didSet { defaults.set(libraryMode.rawValue,forKey: "libraryMode") } }
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
    public var notice: String?
    public private(set) var isDemo = false
    public var pendingCreationURL: URL?
    public let player: PlayerModel
    public var stageCoverBlur: Bool {
        didSet { defaults.set(stageCoverBlur, forKey: "player-stage-cover-blur") }
    }
    public var stageBlurRadius: Double {
        didSet { defaults.set(stageBlurRadius, forKey: "player-stage-blur-radius") }
    }
    public var autoCheckNewSongs: Bool { didSet { defaults.set(autoCheckNewSongs, forKey: "autoCheckNewSongs") } }
    public var dailyGoalMinutes: Double { didSet { defaults.set(dailyGoalMinutes, forKey: "dailyGoalMinutes") } }
    public var karaokeLyrics: Bool {
        didSet { defaults.set(karaokeLyrics, forKey: "player-karaoke-lyrics") }
    }
    /// Shared with ImportView so smoke exercises the model actually displayed in the window.
    public private(set) var importer = ImportModel()
    public private(set) var library: LibraryModel

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
        static let database = "libraryDatabase"
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
        updates = UpdateModel(defaults: defaults, client: client as? any UpdateCalling)
        scopes = SubcorpusStore(defaults: defaults)
        libraryMode = LibraryMode(rawValue: defaults.string(forKey: "libraryMode") ?? "") ?? .songs
        self.client = client
        library = LibraryModel(client: client as? any LibraryCalling, defaults: defaults)
        self.openClient = openClient
        player = PlayerModel(engine: playbackEngine, defaults: defaults)
        stageCoverBlur = defaults.object(forKey: "player-stage-cover-blur") as? Bool ?? true
        let blur = defaults.object(forKey: "player-stage-blur-radius") as? NSNumber
        let radius = blur?.doubleValue ?? 64
        stageBlurRadius = radius.isFinite ? min(120, max(20, radius)) : 64
        autoCheckNewSongs = defaults.object(forKey: "autoCheckNewSongs") as? Bool ?? true
        let goal = defaults.object(forKey: "dailyGoalMinutes") as? Double ?? 30
        dailyGoalMinutes = goal.isFinite ? min(180, max(5, goal.rounded())) : 30
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
        let savedName = defaults.string(forKey: Keys.database) ?? Self.defaultDatabaseName
        if let saved = defaults.string(forKey: Keys.library), Self.hasDatabase(in: URL(fileURLWithPath: saved), named: savedName) {
            await openExisting(folder: URL(fileURLWithPath: saved, isDirectory: true), database: savedName)
        } else if Self.hasDatabase(in: Self.defaultLibraryFolder) {
            await openExisting(folder: Self.defaultLibraryFolder)
        }
    }

    private static func hasDatabase(in folder: URL, named name: String = "corpus.db") -> Bool {
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(atPath: folder.appendingPathComponent(name).path, isDirectory: &isDirectory)
            && !isDirectory.boolValue
    }

    /// Selecting a non-library folder requests confirmation; only the explicit create action creates it.
    /// 选库时可以选文件夹，也可以直接选里面的 `corpus.db`（打开的仍是它所在的整个文件夹：
    /// 音频、raw/、词典都按这个目录找）
    public func selectLibrary(picked url: URL) async {
        do {
            let location = try Self.libraryLocation(forPicked: url)
            if location.database == Self.defaultDatabaseName {
                await selectLibrary(folder: location.folder)
            } else {
                // 直接选的数据库文件：它就在那儿，不用问要不要新建
                errorMessage = nil
                pendingCreationURL = nil
                await openExisting(folder: location.folder, database: location.database)
            }
        } catch { errorMessage = error.localizedDescription }
    }

    public struct LibraryPickError: LocalizedError, Equatable {
        public let errorDescription: String?
    }

    /// 选中的东西对应哪个库：选文件夹就是里面的 corpus.db；选数据库文件就用它，不管叫什么名字。
    /// 音频、raw/、词典都按数据库所在的文件夹找
    public static func libraryLocation(forPicked url: URL) throws -> (folder: URL, database: String) {
        var isDirectory: ObjCBool = false
        if FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory), isDirectory.boolValue {
            return (url, defaultDatabaseName)
        }
        let ext = url.pathExtension.lowercased()
        if ["db", "sqlite", "sqlite3", "db3"].contains(ext) {
            // corpus.db-wal / -shm 这类附属文件不是库本身
            return (url.deletingLastPathComponent(), url.lastPathComponent)
        }
        throw LibraryPickError(errorDescription: "「\(url.lastPathComponent)」不是语料库：请选语料库文件夹，或其中的数据库文件（.db）")
    }

    public static func libraryFolder(forPicked url: URL) throws -> URL {
        try libraryLocation(forPicked: url).folder
    }

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

    public func openExisting(folder: URL, database: String = "corpus.db") async {
        guard Self.hasDatabase(in: folder, named: database) else {
            errorMessage = "这个文件夹没有 \(database)：请选择已有的语料库，或确认新建一个库。"
            return
        }
        await createOrOpen(folder: folder, database: database)
    }

    /// 当前库里还在跑的长任务（导入、下载分词词典、导入词典、刮削、补歌词、曲库维护、Anki 制卡）。
    /// 这些任务绑在旧库上，换库后界面就看不到它们的进度、也停不下来，完成后的刷新还会落到新库上，
    /// 所以有任务在跑时不让换库
    public func runningJobs() async -> [String] {
        guard let client else { return [] }
        var names: [String] = []
        if importer.scanning { names.append("扫描导入文件") }
        // 不写成 `a || (try? await …)`：|| 的右边是不支持 async 的自动闭包
        let importRunning = (try? await client.importProgress())?.running == true
        if importer.importing || importRunning { names.append("导入") }
        let tokenizerRunning = (try? await client.tokenizerProgress())?.running == true
        if importer.downloading || tokenizerRunning { names.append("下载分词词典") }
        if let core {
            if (try? await core.dictImportProgress())?.running == true { names.append("导入词典") }
            if (try? await core.scrapeProgress())?.running == true { names.append("刮削") }
            if (try? await core.lyricsFillProgress())?.running == true { names.append("补逐字歌词") }
            if (try? await core.libraryProgress())?.running == true { names.append("曲库维护") }
            if (try? await core.ankiProgress())?.running == true { names.append("Anki 制卡") }
        }
        return names
    }

    /// Explicit create-or-open semantics, used for the default folder and confirmed custom folders.
    public func createOrOpen(folder: URL, database: String = "corpus.db") async {
        errorMessage = nil
        if let blocked = await switchBlocked() {
            errorMessage = blocked
            return
        }
        let access = folder.startAccessingSecurityScopedResource()
        defer { if access { folder.stopAccessingSecurityScopedResource() } }
        do {
            let next = try await openClient(folder.appendingPathComponent(database))
            try await next.setAudioRoots(audioRoots)
            let fetched = try await next.tracks()
            // 打开新库要几秒：这期间开始的任务也要拦
            if let blocked = await switchBlocked() {
                errorMessage = blocked
                return
            }
            if client != nil { await detachLibrary() }
            client = next
            updates.client = next as? any UpdateCalling
            library = LibraryModel(client: next as? any LibraryCalling, defaults: defaults)
            libraryURL = folder
            databaseName = database
            importer = ImportModel()
            updateTracks(fetched)
            isDemo = false
            defaults.set(folder.path, forKey: Keys.library)
            defaults.set(database, forKey: Keys.database)
            if autoCheckNewSongs && library.client != nil { Task { await library.checkNewAudio() } }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func switchBlocked() async -> String? {
        let jobs = await runningJobs()
        guard !jobs.isEmpty else { return nil }
        return "「\(jobs.joined(separator: "、"))」还在进行，它属于当前这个库。等它完成或先停止，再切换语料库。"
    }

    /// 换库前把旧库留在界面上的东西清掉：播放器（历史先记回旧库）、详情页的导航栈、
    /// 检索 / 分析的请求、振假名。它们里面的歌曲 id、行 id 都是旧库的
    private func detachLibrary() async {
        await player.detachLibrary()
        showStage = false
        libraryPath = []
        analysisScope = nil
        searchRequest = nil
        furigana = [:]
        furiganaError = nil
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
        player.refreshMetadata(tracks)
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
        let mode = furiganaMode
        do {
            let lines = try await core.lyricsFurigana(songId, mode: mode)
            // 快速换歌时旧请求可能后到：别拿上一首的注音盖掉这一首的
            guard showFurigana, player.track?.id == songId, furiganaMode == mode, self.core === core else { return }
            furigana = Dictionary(lines.map { ($0.utteranceId, $0.rubies) }, uniquingKeysWith: { a, _ in a })
        } catch {
            guard player.track?.id == songId, self.core === core else { return }
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

    /// 按 id 放一首歌（首页、搜索结果只有 id）。
    /// `queue` 是界面上这一组歌（专辑、歌手作品、曲库列表、检索结果），放完这首按顺序接着放
    public func play(songId: String, at: Double? = nil, source: String = "library", queue: [Track]? = nil) async {
        do {
            // 不写成 `listed ?? await track(id)`：?? 的右边是不支持 async 的自动闭包
            var found = queue?.first(where: { $0.id == songId })
            if found == nil { found = try await self.track(songId) }
            guard let track = found else { return }
            showLyrics = true
            await player.open(track, at: at, autoplay: true, source: source, queue: queue)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    /// 检索结果里出现的歌，按第一次出现的顺序排成队列（曲库里找不到的跳过）
    public func queue(songIds: [String]) -> [Track] {
        var seen = Set<String>()
        return songIds.compactMap { id in seen.insert(id).inserted ? tracksById[id] : nil }
    }

    /// 跳到检索页查一个词
    public func search(_ keyword: String, field: MatchField = .lemma) {
        searchRequest = SearchRequest(keyword: keyword, field: field)
        section = .search
    }
}
