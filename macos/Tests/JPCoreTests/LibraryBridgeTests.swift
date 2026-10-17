import Foundation
import XCTest

@testable import JPCore

/// 首页、专辑、歌手、⌘K 搜索、来源追踪、分析存档：Rust 返回的形状能解码成 Swift 模型
final class LibraryBridgeTests: XCTestCase {
    private var dir: URL!
    private var client: CorpusClient!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("jpcore-lib-\(UUID().uuidString)")
        let db = try CorpusCore.writeFixture(in: dir)
        client = CorpusClient(core: try CorpusCore(databaseURL: db))
    }

    override func tearDownWithError() throws {
        client = nil
        try? FileManager.default.removeItem(at: dir)
    }

    func testHomeShowsWhatWasJustPlayed() async throws {
        try await client.recordPlay(PlayEvent(songId: "004", listenedSec: 40, positionSec: 40, completed: false, source: "test"))
        let home = try await client.homeSummary()
        XCTAssertEqual(home.continueTrack?.songId, "004")
        XCTAssertEqual(home.recentlyPlayed.first?.songId, "004")
        XCTAssertEqual(home.overview.tracks, 4)
        XCTAssertGreaterThanOrEqual(home.today.listenedSec, 40)
    }

    func testAlbumsPeopleAndFavorites() async throws {
        let albums = try await client.albums()
        for album in albums {
            let tracks = try await client.albumTracks(album.id)
            XCTAssertEqual(Int64(tracks.count), album.trackCount)
        }
        let people = try await client.peopleDirectory()
        let singer = try XCTUnwrap(people.first { $0.performer > 0 })
        let works = try await client.personWorks(singer.id)
        XCTAssertFalse(works.isEmpty)
        XCTAssertTrue(works.allSatisfy { !$0.track.title.isEmpty && !$0.roles.isEmpty })
        _ = try await client.collaborators(singer.id)
        _ = try await client.trackCredits(works[0].track.id)

        let was = try await client.isFavorite(song: "002")
        let now = try await client.toggleFavorite(song: "002")
        XCTAssertEqual(now, !was)
        let home = try await client.homeSummary()
        XCTAssertEqual(home.favorites.contains { $0.id == "002" }, now)
    }

    func testQuickSearchFindsEveryKind() async throws {
        let r = try await client.quickSearch("夜")
        XCTAssertFalse(r.isEmpty)
        XCTAssertFalse(r.words.isEmpty, "夜 是库里的一个词")
        XCTAssertFalse(r.lyrics.isEmpty, "也出现在歌词里")
        if case .lyric(_, _, _, let utterance, _, _) = r.lyrics[0] {
            XCTAssertGreaterThan(utterance, 0)
        } else {
            XCTFail("歌词组里应当是歌词")
        }
    }

    func testLineageQualityAndArchive() async throws {
        let lineage = try await client.songLineage("001")
        XCTAssertEqual(lineage.versions.first?.sourceType, "legacy")
        let quality = try await client.qualityOverview()
        XCTAssertEqual(quality.songs, 4)

        let query = FrequencyQuery(scope: AnalysisScope(subcorpus: Subcorpus(yearTo: 2019), unit: .distinctLines), limit: 20)
        let run = try await client.archiveFrequency(query, label: "测试")
        let cmp = try await client.analysisRerun(run.id)
        XCTAssertTrue(cmp.resultIdentical)
        XCTAssertEqual(cmp.before.id, run.id)
        let list = try await client.analysisList()
        XCTAssertEqual(list.count, 2)
        XCTAssertEqual(list.first { $0.id == run.id }?.filter.yearTo, 2019)
        XCTAssertEqual(list.first { $0.id == run.id }?.kindLabel, "词频（重复感知）")

        var keyness = KeynessQuery(target: Subcorpus(songIds: ["001"]))
        keyness.minFreq = 1
        _ = try await client.archiveKeyness(keyness, label: "对比")
        let again = try await client.analysisCompare(before: run.id, after: cmp.after.id)
        XCTAssertTrue(again.resultIdentical)
        try await client.analysisDelete(run.id)
        let after = try await client.analysisList()
        XCTAssertEqual(after.count, 2)
    }
}
