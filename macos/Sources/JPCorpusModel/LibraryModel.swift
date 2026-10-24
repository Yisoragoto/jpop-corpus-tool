import Foundation
import Observation
import JPCore

@MainActor @Observable
public final class LibraryModel {
    public var client: (any LibraryCalling)?
    public var busy = false
    public var errorMessage: String?
    public var note = ""
    public var version = 0
    public var missing: [MissingAudio] = []
    public var picks: [String: String] = [:]
    public var repairErrors: [String: String] = [:]
    public var pendingDeletion: String?
    public var correction: CorrectionView?
    public var tokens: [TokenEdit] = []
    public var selected: [Int] = []
    public var confirmRevert = false
    public var job: JobProgress<LibraryJobResult>?
    public private(set) var migrationSource = ""
    public private(set) var migrationPlan: MigratePlan?
    public var migrationOptions = MigrateOptions()
    public var found: NewAudio?
    public private(set) var checkingNewAudio = false
    public var ignores: NewSongIgnores {
        didSet { if let data = try? JSONEncoder().encode(ignores) { defaults.set(String(decoding: data, as: UTF8.self), forKey: "jp.newSongs.ignored") } }
    }
    @ObservationIgnored private let defaults: UserDefaults
    public init(client: (any LibraryCalling)? = nil, defaults: UserDefaults = .standard) {
        self.client = client; self.defaults = defaults
        ignores = NewSongs.parseIgnores(defaults.string(forKey: "jp.newSongs.ignored"))
    }
    @discardableResult public func perform(_ work: () async throws -> Void) async -> Bool {
        guard !busy else { return false }; busy = true; errorMessage = nil
        defer { busy = false }
        do { try await work(); return true } catch { errorMessage = error.localizedDescription; return false }
    }
    public func save(_ id: String, edit: SongEdit) async -> Bool {
        await perform {
            guard let client else { throw CoreError.open("请先打开语料库") }
            let report = try await client.libraryEditSong(id, edit: edit)
            note = LibraryAdmin.describeEdit(report); errorMessage = LibraryAdmin.csvProblem(report.csv); version += 1
        }
    }
    public func deleteConfirmed() async -> Bool {
        guard let id = pendingDeletion else { return false }
        return await perform {
            guard let client else { throw CoreError.open("请先打开语料库") }
            let r = try await client.libraryDeleteSong(id)
            note = LibraryAdmin.describeDelete(r); errorMessage = LibraryAdmin.csvProblem(r.csv); version += 1; pendingDeletion = nil
        }
    }
    public func loadMissing() async {
        _ = await perform { guard let client else { return }; missing = try await client.libraryMissingAudio() }
    }
    public func suggest(_ folder: String) async {
        _ = await perform {
            guard let client else { return }
            let suggestions = try await client.librarySuggestRelinks(folder)
            for s in suggestions where picks[s.songId] == nil { picks[s.songId] = s.path }
            note = suggestions.isEmpty ? "没有找到有把握配上的文件" : "配上 \(suggestions.count) 首，其余可手动选择"
        }
    }
    public func applyRelinks() async -> Bool {
        guard !picks.isEmpty else { return false }
        return await perform {
            guard let client else { return }
            let batch = try await client.libraryRelinkAudio(picks.keys.sorted().map { RelinkRequest(songId: $0, path: picks[$0]!) })
            repairErrors = Dictionary(uniqueKeysWithValues: batch.results.filter { !$0.error.isEmpty }.map { ($0.songId, $0.error) })
            let done = Set(batch.results.filter { $0.error.isEmpty }.map(\.songId))
            missing.removeAll { done.contains($0.songId) }; picks = picks.filter { !done.contains($0.key) }
            errorMessage = LibraryAdmin.csvProblem(batch.csv); version += 1
        }
    }
    public func openCorrection(_ id: Int64) async {
        correction = nil; tokens = []; selected = []; confirmRevert = false
        _ = await perform {
            guard let client else { return }; correction = try await client.tokenCorrection(id)
            guard let correction else { throw CoreError.open("这一行已经不在库里了") }; tokens = correction.tokens
        }
    }
    public var correctionDirty: Bool { correction.map { Corrections.cleanTokens(tokens) != $0.tokens } ?? false }
    public func saveCorrection() async -> Bool {
        guard let correction, correctionDirty, !Corrections.cleanTokens(tokens).isEmpty else { return false }
        return await perform {
            guard let client else { throw CoreError.open("请先打开语料库") }
            try await client.saveTokenCorrection(correction.utteranceId, tokens: Corrections.cleanTokens(tokens)); version += 1
        }
    }
    public func revertCorrection() async -> Bool {
        guard let correction, correction.corrected else { return false }
        guard confirmRevert else { confirmRevert = true; return false }
        return await perform {
            guard let client else { throw CoreError.open("请先打开语料库") }
            _ = try await client.revertTokenCorrection(correction.utteranceId); version += 1
        }
    }
    public func pollJob() async {
        do { let wasFinished = job?.finished == true; job = try await client?.libraryProgress(); if job?.finished == true && !wasFinished { version += 1 }; if let error = job?.error { errorMessage = error }; if let r = job?.result { note = r.summary } }
        catch { errorMessage = error.localizedDescription }
    }
    public func previewMigration(_ path: String) async {
        migrationSource = path; migrationPlan = nil
        _ = await perform {
            guard let client else { throw CoreError.open("请先打开语料库") }
            migrationPlan = try await client.migratePlan(path)
        }
    }
    public func migrateConfirmed() async -> Bool {
        guard migrationPlan != nil, !migrationSource.isEmpty else { return false }
        return await startJob { client in try await client.migrateRun(self.migrationSource, options: self.migrationOptions) }
    }
    public func backfillDurations() async -> Bool {
        await startJob { try await $0.backfillDurations() }
    }
    public func fillAlbumArtwork() async -> Bool {
        await startJob { try await $0.fillAlbumArtwork() }
    }
    private func startJob(_ work: (any LibraryCalling) async throws -> Void) async -> Bool {
        let started = await perform {
            guard let client else { throw CoreError.open("请先打开语料库") }
            try await work(client); job = nil
        }
        if started { await pollJob() }
        return started
    }
    public func checkNewAudio() async {
        guard !checkingNewAudio, let client else { return }
        checkingNewAudio = true
        defer { checkingNewAudio = false }
        do {
            repeat {
                let requested = ignores
                let result = try await client.libraryNewAudio(ignoredFolders: requested.folders, ignoredFiles: requested.files)
                if requested == ignores { found = result; break }
            } while !Task.isCancelled
        }
        catch { errorMessage = error.localizedDescription }
    }
    public func ignoreFiles(_ paths: [String]) { ignores = NewSongs.withIgnored(ignores, NewSongIgnores(files: paths)); found = nil }
    public func ignoreFolder(_ path: String) async { ignores = NewSongs.withIgnored(ignores, NewSongIgnores(folders: [path])); await checkNewAudio() }
    public func restoreFolder(_ path: String) async { ignores = NewSongs.withoutIgnoredFolder(ignores, path); await checkNewAudio() }
}
