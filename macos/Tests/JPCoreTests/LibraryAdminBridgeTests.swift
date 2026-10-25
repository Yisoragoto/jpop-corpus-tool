import Foundation
import XCTest
@testable import JPCore

/// 真正调用 libjp_ffi：字段解码、事务与后台作业读回。
final class LibraryAdminBridgeTests: XCTestCase {
    private var dir: URL!
    private var client: CorpusClient!
    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("jpcore-admin-\(UUID().uuidString)")
        client = CorpusClient(core: try CorpusCore(databaseURL: CorpusCore.writeFixture(in: dir)))
    }
    override func tearDownWithError() throws { client = nil; try? FileManager.default.removeItem(at: dir) }

    func testEditReadbackAndDeleteKeepAudio() async throws {
        let tracks = try await client.tracks()
        let original = try XCTUnwrap(tracks.first)
        var edit = SongEdit(original); edit.title = "管理测试标题"; edit.artist = "甲/乙"; edit.album = "新专辑"
        let report = try await client.libraryEditSong(original.id, edit: edit)
        XCTAssertTrue(report.changed.contains("title")); XCTAssertEqual(report.performers, 2)
        let readback = try await client.track(original.id)
        XCTAssertEqual(readback?.title, edit.title); XCTAssertEqual(readback?.artist, "甲/乙")
        let removed = try await client.libraryDeleteSong(original.id)
        XCTAssertEqual(removed.title, edit.title); XCTAssertEqual(removed.audioPath, original.audioPath)
        let deleted = try await client.track(original.id)
        XCTAssertNil(deleted)
    }
    func testMemberInputOutcomesAndAllQueriesDecode() async throws {
        let people = try await client.peopleByRole("performer", limit: 5000)
        let band = try XCTUnwrap(people.first)
        let inserted = try await client.membershipPut(.init(bandId: band.id, memberName: "测试成员", part: "吉他", joined: "2019"))
        XCTAssertEqual(inserted, .inserted)
        let members = try await client.bandMembers(band.id)
        let member = try XCTUnwrap(members.first)
        XCTAssertEqual(member.source, "manual"); XCTAssertEqual(member.joined, "2019")
        let confirmed = try await client.membershipConfirm(bandId: band.id, memberId: member.memberId)
        XCTAssertEqual(confirmed, .keptExisting)
        let bands = try await client.bandsOf(member.memberId)
        XCTAssertEqual(bands.first?.bandId, band.id)
        _ = try await client.memberships(); _ = try await client.membershipSuggestions()
        _ = try await client.artistRoster(); _ = try await client.worksByPerson(band.id)
        let directory = try await client.peopleDirectory()
        XCTAssertFalse(directory.isEmpty)
        let works = try await client.personWorks(band.id)
        XCTAssertTrue(works.allSatisfy { !$0.performerIds.isEmpty })
        try await client.membershipReject(bandId: band.id, memberId: member.memberId)
        let remaining = try await client.bandMembers(band.id)
        XCTAssertTrue(remaining.isEmpty)
    }
    func testCorrectionRoundtripAndLanguage() async throws {
        let lines = try await client.lyrics("001")
        let id = try XCTUnwrap(lines.first?.utteranceId)
        let before = try await client.tokenCorrection(id)
        try await client.saveTokenCorrection(id, tokens: [TokenEdit("夜空", lemma: "夜空", pos: "NOUN")])
        let corrected = try await client.tokenCorrection(id)
        XCTAssertEqual(corrected?.tokens, [TokenEdit("夜空")]); XCTAssertEqual(corrected?.corrected, true)
        let reverted = try await client.revertTokenCorrection(id)
        XCTAssertTrue(reverted)
        let after = try await client.tokenCorrection(id)
        XCTAssertEqual(after?.tokens, before?.tokens)
        try await client.songSetLanguage("001", language: "ja")
        let lineage = try await client.songLineage("001")
        XCTAssertEqual(lineage.language, "ja")
        let forms = try await client.lemmaForms([])
        XCTAssertTrue(forms.isEmpty)
    }
    func testRepairDiscoveryAndListeningQueries() async throws {
        let missing = try await client.libraryMissingAudio()
        XCTAssertFalse(missing.isEmpty)
        let result = try await client.libraryRelinkAudio([RelinkRequest(songId: "001", path: dir.appendingPathComponent("absent.wav").path)])
        XCTAssertFalse(try XCTUnwrap(result.results.first).error.isEmpty)
        _ = try await client.librarySuggestRelinks(dir.path)
        let found = try await client.libraryNewAudio(ignoredFolders: [], ignoredFiles: [])
        XCTAssertEqual(found.scan.summary.new, 0)
        try await client.recordPlay(PlayEvent(songId: "001", listenedSec: 40, positionSec: 40, completed: false, source: "test"))
        let recent = try await client.recentlyPlayed(limit: 1)
        let most = try await client.mostPlayed(minListenedSec: 30, limit: 1)
        XCTAssertEqual(recent.first?.songId, "001"); XCTAssertEqual(most.first?.songId, "001")
    }
    func testArtistPhotoSetAndResetReadBackManualFlag() async throws {
        let roster = try await client.artistRoster()
        let name = try XCTUnwrap(roster.first?.name)
        let photo = dir.appendingPathComponent("photo.png")
        try Data([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0]).write(to: photo)
        let manual = try await client.artistImageSet(name, path: photo.path)
        XCTAssertTrue(FileManager.default.fileExists(atPath: manual))
        let people = try await client.peopleDirectory()
        XCTAssertEqual(people.first { $0.name == name }?.imageManual, true)
        _ = try await client.artistImageReset(name)
        let reset = try await client.peopleDirectory()
        XCTAssertEqual(reset.first { $0.name == name }?.imageManual, false)
        XCTAssertFalse(FileManager.default.fileExists(atPath: manual))
    }
    private func completedJob() async throws -> LibraryJobResult {
        for _ in 0..<400 {
            let job = try await client.libraryProgress()
            if job.finished { XCTAssertNil(job.error); return try XCTUnwrap(job.result) }
            try await Task.sleep(for: .milliseconds(25))
        }
        throw CoreError.call(method: "libraryProgress", message: "测试作业超时")
    }
    func testMaintenanceAndMigrationJobsDecodeFlattenedResults() async throws {
        try await client.backfillDurations()
        guard case .durations(let r) = try await completedJob() else { return XCTFail("预期时长报告") }
        XCTAssertGreaterThanOrEqual(r.scanned, 0)
        try await client.fillAlbumArtwork()
        guard case .artwork(let count) = try await completedJob() else { return XCTFail("预期封面数量") }
        XCTAssertGreaterThanOrEqual(count, 0)
        let source = dir.appendingPathComponent("source")
        _ = try CorpusCore.writeFixture(in: source)
        let plan = try await client.migratePlan(source.path)
        XCTAssertEqual(plan.sourceSongs, 4)
        try await client.migrateRun(source.path, options: MigrateOptions())
        guard case .migration(let outcome) = try await completedJob() else { return XCTFail("预期迁移报告") }
        XCTAssertEqual(outcome.skipped, 4); XCTAssertEqual(outcome.songs, 0)
        XCTAssertTrue(outcome.idMap.isEmpty)
    }
}
