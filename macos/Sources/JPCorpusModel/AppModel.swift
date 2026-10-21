import Foundation
import JPCore
import Observation

public enum SidebarItem: String, CaseIterable, Identifiable, Hashable {
    case search, library, analysis, importer
    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .search: return "检索"
        case .library: return "曲库"
        case .analysis: return "分析"
        case .importer: return "导入"
        }
    }

    public var symbol: String {
        switch self {
        case .search: return "text.magnifyingglass"
        case .library: return "music.note.list"
        case .analysis: return "chart.bar.xaxis"
        case .importer: return "square.and.arrow.down"
        }
    }
}

public struct SearchRequest: Equatable {
    public var keyword: String
    public var field: MatchField
    public var nonce = UUID()
}

@MainActor
@Observable
public final class AppModel {
    public private(set) var client: (any CorpusCalling)?
    public private(set) var libraryURL: URL?
    public private(set) var tracks: [Track] = []
    private var tracksById: [String: Track] = [:]
    public var section: SidebarItem = .search
    public var showLyrics = true
    public var searchRequest: SearchRequest?
    public var errorMessage: String?
    public private(set) var isDemo = false
    public var pendingCreationURL: URL?
    public let player = PlayerModel()
    /// Shared with ImportView so smoke exercises the model actually displayed in the window.
    public let importer = ImportModel()

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
    }

    public init(
        defaults: UserDefaults = .standard,
        client: (any CorpusCalling)? = nil,
        openClient: @escaping @Sendable (URL) async throws -> any CorpusCalling = { db in
            let core = try await Task.detached { try CorpusCore(databaseURL: db) }.value
            return CorpusClient(core: core)
        }
    ) {
        self.defaults = defaults
        self.client = client
        self.openClient = openClient
        audioRoots = defaults.stringArray(forKey: Keys.audioRoots) ?? []
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

    /// Selecting a non-library folder requests confirmation; only confirmCreation() creates it.
    public func selectLibrary(folder: URL) async {
        errorMessage = nil
        if Self.hasDatabase(in: folder) {
            pendingCreationURL = nil
            await openExisting(folder: folder)
        } else {
            pendingCreationURL = folder
        }
    }

    public func confirmCreation() async {
        guard let folder = pendingCreationURL else { return }
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

    public func search(_ keyword: String, field: MatchField = .lemma) {
        searchRequest = SearchRequest(keyword: keyword, field: field)
        section = .search
    }
}
