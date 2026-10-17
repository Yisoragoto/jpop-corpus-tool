import Foundation
import XCTest

@testable import JPCore

/// 词典管理和制卡：Rust 返回的形状能解码；Anki 没开时（CI 上就是）各个调用说人话而不是崩
final class MiningBridgeTests: XCTestCase {
    private var dir: URL!
    private var client: CorpusClient!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("jpcore-mine-\(UUID().uuidString)")
        let db = try CorpusCore.writeFixture(in: dir)
        client = CorpusClient(core: try CorpusCore(databaseURL: db))
    }

    override func tearDownWithError() throws {
        client = nil
        try? FileManager.default.removeItem(at: dir)
    }

    /// 一本只有一个词条的 Yomitan 词典（解开的目录）
    private func yomitan() throws -> URL {
        let d = dir.appendingPathComponent("yomitan", isDirectory: true)
        try FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        try #"{"title":"テスト辞書","revision":"1","format":3}"#.write(to: d.appendingPathComponent("index.json"), atomically: true, encoding: .utf8)
        try #"[["夜","よる","n","",0,["日が沈んでから昇るまでの間"],1,""]]"#.write(to: d.appendingPathComponent("term_bank_1.json"), atomically: true, encoding: .utf8)
        return d
    }

    private func waitForImport() async throws -> JobProgress<DictImportBatch> {
        for _ in 0..<300 {
            let p = try await client.dictImportProgress()
            if p.finished { return p }
            try await Task.sleep(nanoseconds: 50_000_000)
        }
        XCTFail("词典导入没跑完")
        throw CancellationError()
    }

    func testDictionaryImportOrderToggleAndDelete() async throws {
        try await client.dictImport([try yomitan().path])
        let done = try await waitForImport()
        XCTAssertNil(done.error)
        XCTAssertEqual(done.result?.results.first?.summary?.terms, 1)

        let list = try await client.dictionaries()
        XCTAssertEqual(list.map(\.title), ["テスト辞書"])
        XCTAssertEqual(list[0].counts?.terms, 1)
        XCTAssertTrue(list[0].summary.contains("1 条"))
        let found = try await client.lookup("夜に")
        XCTAssertEqual(found.entries.first?.headwords.first?.term, "夜")

        try await client.dictSetEnabled(list[0].id, false)
        let disabled = try await client.lookup("夜に")
        XCTAssertTrue(disabled.entries.isEmpty)
        try await client.dictSetEnabled(list[0].id, true)
        try await client.dictSetOrder([list[0].id])
        try await client.dictDelete(list[0].id)
        let after = try await client.dictionaries()
        XCTAssertTrue(after.isEmpty)
    }

    func testAnkiCallsDecodeAndExplainWhenAnkiIsNotRunning() async throws {
        let status = try await client.ankiStatus()
        if !status.connected {
            XCTAssertFalse(status.message.isEmpty, "连不上要给指引")
            let check = try await client.ankiMineCheck(model: lyricsNoteType, expressions: ["夜"])
            XCTAssertFalse(check.connected)
            XCTAssertEqual(check.notes.count, 1)
        }
        // 没有词典时制卡：报一句人话，不碰 Anki
        let request = MineRequest(lookupText: "夜に", term: "夜", reading: "よる", songId: "004", utteranceId: nil, tokenIndex: nil,
                                  deck: "JPOP", artistSubdeck: false, model: lyricsNoteType, mainDictionary: nil, kind: .wordAndSentence)
        do {
            _ = try await client.ankiMine(request)
            XCTFail("没有词典应当报错")
        } catch {
            XCTAssertTrue(error.localizedDescription.contains("词典"), error.localizedDescription)
        }
    }
}
