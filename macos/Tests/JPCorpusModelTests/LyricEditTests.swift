import JPCore
import XCTest
@testable import JPCorpusModel

/// app/src/lyricEdit.test.ts 全部 6 个用例。
final class LyricEditTests: XCTestCase {
    func testFormatsLikeLRC() {
        XCTAssertEqual(LyricEdit.formatStamp(3.1), "0:03.10")
        XCTAssertEqual(LyricEdit.formatStamp(61.255), "1:01.26")
        XCTAssertEqual(LyricEdit.formatStamp(nil), "")
    }
    func testReadsUsualSpellingsAndRefusesRest() {
        XCTAssertEqual(LyricEdit.parseStamp("1:02.5"), .value(62.5))
        XCTAssertEqual(LyricEdit.parseStamp("[01:02.50]"), .value(62.5))
        XCTAssertEqual(LyricEdit.parseStamp("01:02:50"), .value(62.5))
        XCTAssertEqual(LyricEdit.parseStamp("62.25"), .value(62.25))
        XCTAssertEqual(LyricEdit.parseStamp("  "), .empty)
        XCTAssertEqual(LyricEdit.parseStamp("1:75"), .invalid)
        XCTAssertEqual(LyricEdit.parseStamp("abc"), .invalid)
    }
    func testRoundTrips() {
        for t in [0.0, 3.1, 59.99, 61.25, 3599.5] { XCTAssertEqual(LyricEdit.parseStamp(LyricEdit.formatStamp(t)), .value(t)) }
    }
    private var before: [EditedLyricLine] { [.init(timeSec: 1, text: "夜に駆けるる"), .init(timeSec: 1, text: "夜に駆けるる"), .init(timeSec: 2, text: "さよなら")] }
    func testUnchangedIgnoresBlankRows() {
        XCTAssertEqual(LyricEdit.summarizeEdit(before, before + [.init(timeSec: nil, text: " ")]), "没有改动")
    }
    func testCountsRewrittenAndRemovedLines() {
        XCTAssertEqual(LyricEdit.summarizeEdit(before, [.init(timeSec: 1, text: "夜に駆ける"), .init(timeSec: 2, text: "さよなら")]), "新增或改写 1 行，去掉或改写前 2 行（3 → 2 行）")
    }
    func testRetimedLines() {
        var after = before; after[2].timeSec = 2.5
        XCTAssertEqual(LyricEdit.summarizeEdit(before, after), "调了 1 行的时间（3 → 3 行）")
    }
}
