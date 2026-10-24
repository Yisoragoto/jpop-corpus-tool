import Foundation
import XCTest

@testable import JPCore

/// 刮削（不联网的那部分）和振假名：Rust 返回的形状能解码
final class ScrapeBridgeTests: XCTestCase {
    private var dir: URL!
    private var client: CorpusClient!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("jpcore-scrape-\(UUID().uuidString)")
        let db = try CorpusCore.writeFixture(in: dir)
        client = CorpusClient(core: try CorpusCore(databaseURL: db))
    }

    override func tearDownWithError() throws {
        client = nil
        try? FileManager.default.removeItem(at: dir)
    }

    func testScrapeBookkeepingDecodes() async throws {
        let summary = try await client.scrapeSummary()
        XCTAssertEqual(summary.success, 0)
        let completeness = try await client.completeness()
        XCTAssertEqual(completeness.lyrics.total, 4)
        let review = try await client.scrapeReviewQueue()
        XCTAssertTrue(review.isEmpty)
        let retried = try await client.scrapeRetryFailed()
        XCTAssertEqual(retried, 0)
        let progress = try await client.scrapeProgress()
        XCTAssertFalse(progress.running)
    }

    /// 有分词词典（CI 上经 JP_SUDACHI_SOURCE 装进来）时位置落在原文里；没有时说清楚缺什么
    func testFuriganaLinesUpWithTheLyrics() async throws {
        if let source = ProcessInfo.processInfo.environment["JP_SUDACHI_SOURCE"], !source.isEmpty {
            _ = try await client.tokenizerInstall(from: source)
        }
        do {
            let furigana = try await client.lyricsFurigana("004")
            let lyrics = try await client.lyrics("004")
            XCTAssertEqual(furigana.map(\.utteranceId), lyrics.map(\.utteranceId))
            for (line, f) in zip(lyrics, furigana) {
                let length = line.text.unicodeScalars.count
                for r in f.rubies {
                    XCTAssertTrue(r.start < r.end && r.end <= length && !r.reading.isEmpty, "\(line.text) \(r)")
                }
            }
            XCTAssertTrue(furigana[0].rubies.contains { $0.reading == "よる" }, "「雨が降る夜に」的夜注よる")
            let word = try await client.lyricsFurigana("004", mode: .word)
            XCTAssertEqual(word.count, furigana.count)
        } catch {
            XCTAssertTrue(error.localizedDescription.contains("词典"), error.localizedDescription)
        }
    }
}
