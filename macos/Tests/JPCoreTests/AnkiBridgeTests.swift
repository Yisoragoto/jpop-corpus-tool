import Foundation
import XCTest
@testable import JPCore

final class AnkiBridgeTests: XCTestCase {
    private var directory: URL!
    private var client: CorpusClient!
    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("anki-bridge-\(UUID().uuidString)")
        client = CorpusClient(core: try CorpusCore(databaseURL: CorpusCore.writeFixture(in: directory)))
    }
    override func tearDownWithError() throws { client = nil; try? FileManager.default.removeItem(at: directory) }
    func testOfflineWordsPreviewAndEmptyCardDecodeEveryField() async throws {
        let words = try await client.ankiWords(pos: ["NOUN"], skipStudied: false)
        let word = try XCTUnwrap(words.first); XCTAssertGreaterThan(word.count, 0); XCTAssertGreaterThan(word.songCount, 0)
        let card = try await client.ankiPreview(lemma: word.lemma, pos: word.pos, maxExamples: 0, maxDicts: 0)
        XCTAssertEqual(card.expression, word.lemma); XCTAssertFalse(card.noExamples)
        XCTAssertEqual(card.examples?.count, 1); XCTAssertFalse(card.sentence.isEmpty)
        let empty = try await client.ankiPreview(lemma: "不存在的词", pos: "NOUN", maxExamples: 8, maxDicts: nil)
        XCTAssertTrue(empty.noExamples); XCTAssertTrue(empty.noDefinitions); XCTAssertNil(empty.examples)
    }
    func testIdleCancelAndInvalidStartsAreOffline() async throws {
        let before = try await client.ankiProgress(); XCTAssertFalse(before.running); XCTAssertFalse(before.finished); XCTAssertNil(before.result)
        try await client.ankiCancel(); let running = try await client.ankiIsRunning(); XCTAssertFalse(running)
        var request = AnkiBatchRequest(); request.deck = "D"
        do { _ = try await client.ankiExportStart(request); XCTFail("空词表应拒绝") } catch { XCTAssertTrue(error.localizedDescription.contains("没有选中")) }
        do { _ = try await client.ankiUpdateStart(request); XCTFail("空词表应拒绝") } catch { XCTAssertTrue(error.localizedDescription.contains("没有选中")) }
        do { try await client.ankiRefreshStart(deck: " ", scope: .deck); XCTFail("空牌组应拒绝") } catch { XCTAssertTrue(error.localizedDescription.contains("牌组")) }
    }
    func testReportTextAndImageWriteReadbackAndValidation() async throws {
        let report = try await client.statsReport(StatsFilter())
        let text = directory.appendingPathComponent("report.txt")
        try await client.exportText(path: text.path, content: report.text)
        XCTAssertFalse(report.text.isEmpty); XCTAssertEqual(try String(contentsOf: text, encoding: .utf8), report.text)
        let csv = "\u{feff}a,b\r\n\"歌,詞\",60.0\r\n"
        let csvURL = directory.appendingPathComponent("kwic.csv")
        try await client.exportText(path: csvURL.path, content: csv)
        XCTAssertEqual(try Data(contentsOf: csvURL), Data(csv.utf8))
        let bytes: [UInt8] = [137,80,78,71,13,10,26,10]
        let image = directory.appendingPathComponent("cloud.png")
        try await client.exportPng(path: image.path, bytes: bytes)
        XCTAssertEqual(try Data(contentsOf: image), Data(bytes))
        do { try await client.exportPng(path: image.path, bytes: [0]); XCTFail("非 PNG 应拒绝") } catch { XCTAssertTrue(error.localizedDescription.contains("PNG")) }
        do { try await client.exportText(path: directory.appendingPathComponent("bad.exe").path, content: "x"); XCTFail("扩展名应拒绝") } catch { XCTAssertTrue(error.localizedDescription.contains("只能导出")) }
    }
}
