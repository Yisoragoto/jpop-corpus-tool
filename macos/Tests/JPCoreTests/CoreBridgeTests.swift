import Foundation
import XCTest

@testable import JPCore

/// 跨语言的形状：Rust 返回的 JSON 能不能解码成 Swift 模型。
/// 对着 jp-ffi 写出的合成语料库（四首歌）跑，任何机器都能跑。
final class CoreBridgeTests: XCTestCase {
    private var dir: URL!
    private var client: CorpusClient!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("jpcore-\(UUID().uuidString)")
        let db = try CorpusCore.writeFixture(in: dir)
        client = CorpusClient(core: try CorpusCore(databaseURL: db))
    }

    override func tearDownWithError() throws {
        client = nil
        try? FileManager.default.removeItem(at: dir)
    }

    /// 最小闭环：搜索 → KWIC → 跳到歌 → 定位到可以播放的区间
    func testSearchKwicSongAndListenWindow() async throws {
        var query = KwicQuery(keywords: ["風"], field: .lemma)
        query.dedup = true
        let hits = try await client.kwic(query)
        XCTAssertEqual(hits.count, 2)
        let hit = try XCTUnwrap(hits.first)
        XCTAssertNotNil(hit.endSec)
        XCTAssertGreaterThan(hit.endSec!, hit.timeSec!)

        let lyrics = try await client.lyrics(hit.songId)
        XCTAssertTrue(lyrics.contains { $0.utteranceId == hit.utteranceId })

        let window = try await client.listenWindow(hit.utteranceId, before: 1, after: 1)
        let w = try XCTUnwrap(window)
        XCTAssertEqual(w.songId, hit.songId)
        XCTAssertLessThanOrEqual(w.startSec, w.focusStartSec)
        XCTAssertLessThanOrEqual(w.focusEndSec, w.endSec)

        let spans = try await client.lineSpans(hit.songId)
        XCTAssertFalse(spans.isEmpty)
    }

    func testSubcorpusAndMultiWordAndNearFilters() async throws {
        var only001 = KwicQuery(keywords: ["風"], field: .lemma)
        only001.subcorpus = Subcorpus(songIds: ["001"])
        let hits = try await client.kwic(only001)
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(KwicQuery.parseKeywords("て + しまう | 夜"), ["て+しまう", "夜"])
    }

    func testAnalysisResultsDecode() async throws {
        let freq = try await client.frequency(FrequencyQuery(scope: AnalysisScope(unit: .distinctLines), limit: 5))
        XCTAssertEqual(freq.items.count, 5)
        XCTAssertEqual(freq.provenance.unit, .distinctLines)
        XCTAssertEqual(freq.provenance.snapshot.fingerprint.count, 16)

        var c = CollocationQuery(node: "夜")
        c.minFreq = 1
        c.contentOnly = false
        let colloc = try await client.collocations(c)
        XCTAssertGreaterThanOrEqual(colloc.nodeFreq, 2)

        var k = KeynessQuery(target: Subcorpus(songIds: ["001"]))
        k.minFreq = 1
        let key = try await client.keyness(k)
        XCTAssertTrue(key.over.allSatisfy { $0.logLikelihood > 0 })

        let profile = try await client.songProfile("004")
        XCTAssertGreaterThanOrEqual(profile.repeatedLines, 1)
    }

    func testLibraryCallsDecodeAndErrorsAreReadable() async throws {
        let tracks = try await client.tracks()
        XCTAssertEqual(tracks.count, 4)
        let missing = try await client.track("没有这首")
        XCTAssertNil(missing)
        _ = try await client.overview()
        _ = try await client.performers()
        let word = try await client.wordInCorpus("夜", currentSongId: nil)
        XCTAssertGreaterThanOrEqual(word.occurrences, 2)

        do {
            _ = try await client.perform { try $0.call("noSuchMethod", as: String.self) }
            XCTFail("应当报错")
        } catch let error as CoreError {
            XCTAssertTrue(error.localizedDescription.contains("noSuchMethod"))
        }
    }

    func testOpeningGarbageFailsWithAMessageInsteadOfCrashing() throws {
        let bogus = dir.appendingPathComponent("not-a-db.db")
        try Data("this is not sqlite".utf8).write(to: bogus)
        XCTAssertThrowsError(try CorpusCore(databaseURL: bogus))
    }
}
