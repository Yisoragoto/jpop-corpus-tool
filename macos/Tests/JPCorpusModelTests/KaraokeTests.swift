import JPCore
import XCTest
@testable import JPCorpusModel

/// app/src/karaoke.test.ts 全部 17 个用例，输入和期望值逐条对应。
final class KaraokeTests: XCTestCase {
    private func line(_ text: String) -> TimingDraftLine { .init(text: text, units: Karaoke.defaultUnits(text)) }
    private func texts(_ line: TimingDraftLine) -> [String] { line.units.map { Karaoke.unitText(line, $0) } }
    private func t(_ a: Int, _ b: Int, _ s: Double, _ e: Double?, _ source: String = "lrc") -> WordTiming {
        .init(utteranceId: 1, charStart: a, charEnd: b, startSec: s, endSec: e, source: source)
    }
    private func u(_ a: Int, _ b: Int, _ s: Double? = nil, _ e: Double? = nil) -> TimingDraftUnit { .init(charStart: a, charEnd: b, start: s, end: e) }
    private func at(_ l: Int, _ u: Int) -> TimingCursor { .init(line: l, unit: u) }
    private func lines() -> [TimingDraftLine] { [line("夜に"), line("あ")] }
    private func timed() -> [TimingDraftLine] { [.init(text: "夜に駆", timeSec: 10, units: [u(0, 1, 10, 10.5), u(1, 2, 10.5, 11), u(2, 3, 11.5, 12)])] }

    func testDefaultKanaAndSeparators() {
        XCTAssertEqual(texts(line("きゃりー、ぱみゅ")), ["きゃ", "り", "ー、", "ぱ", "みゅ"])
        XCTAssertEqual(texts(line("夜に 駆ける")), ["夜", "に ", "駆", "け", "る"])
    }
    func testDefaultLatinAndLeadingPunctuation() {
        XCTAssertEqual(texts(line("「Hello world 123」")), ["「Hello ", "world ", "123」"])
    }
    func testPositionsAreCodePointsNotUTF16() {
        XCTAssertEqual(line("𠮷野家").units.map { [$0.charStart, $0.charEnd] }, [[0, 1], [1, 2], [2, 3]])
    }
    func testInitialRealUnitsAndUntimedGaps() {
        XCTAssertEqual(Karaoke.initialUnits("夜に駆ける", [t(0, 2, 10, 10.5), t(4, 5, 11, 11.4)]), [u(0, 2, 10, 10.5), u(2, 3), u(3, 4), u(4, 5, 11, 11.4)])
    }
    func testManualPriorityStartOnlyAndOverlap() {
        let units = Karaoke.initialUnits("夜に", [t(0, 1, 10, 10.2), t(0, 1, 10.1, 10.3, "manual"), t(1, 2, 12, nil, "manual"), t(0, 2, 9, 9.5)])
        XCTAssertEqual(units[0], u(0, 1, 10.1, 10.3))
        XCTAssertEqual(units[1], u(1, 2))
    }
    func testHoldDownUpAndNextLine() {
        var state = Karaoke.tapDown(lines(), at(0, 0), 10, .hold)
        XCTAssertEqual(state.cursor, at(0, 0))
        var up = Karaoke.tapUp(state.lines, state.cursor, 10.4, .hold)
        XCTAssertEqual(up.cursor, at(0, 1))
        state = Karaoke.tapDown(up.lines, up.cursor, 10.6, .hold)
        up = Karaoke.tapUp(state.lines, state.cursor, 11, .hold)
        XCTAssertEqual(up.cursor, at(1, 0))
        XCTAssertEqual(up.lines[0].units.map { [$0.start, $0.end] }, [[10, 10.4], [10.6, 11]])
    }
    func testTapAdvancesAndLastReleaseEndsLine() {
        var state = Karaoke.tapDown(lines(), at(0, 0), 10, .tap)
        XCTAssertEqual(state.cursor, at(0, 1))
        let ignored = Karaoke.tapUp(state.lines, at(0, 0), 10.1, .tap)
        XCTAssertNil(ignored.lines[0].units[0].end)
        state = Karaoke.tapDown(state.lines, state.cursor, 10.5, .tap)
        XCTAssertEqual(state.lines[0].units[0].end, 10.5)
        XCTAssertEqual(state.cursor, at(0, 1))
        let up = Karaoke.tapUp(state.lines, state.cursor, 11, .tap)
        XCTAssertEqual(up.lines[0].units[1].start, 10.5)
        XCTAssertEqual(up.lines[0].units[1].end, 11)
        XCTAssertEqual(up.cursor, at(1, 0))
    }
    func testRetapClearsEarlierLaterTimes() {
        let done = Karaoke.tapUp(Karaoke.tapDown(lines(), at(0, 1), 5, .hold).lines, at(0, 1), 5.5, .hold).lines
        let again = Karaoke.tapDown(done, at(0, 0), 6, .hold).lines
        XCTAssertNil(again[0].units[1].start)
        XCTAssertNil(again[0].units[1].end)
        XCTAssertNotEqual(Karaoke.lineStatus(again, 0, nil).status, "error")
    }
    func testFastReleaseHasMinimumLength() {
        let down = Karaoke.tapDown(lines(), at(0, 0), 10, .hold)
        let up = Karaoke.tapUp(down.lines, down.cursor, 10, .hold)
        XCTAssertEqual(up.lines[0].units[0].end!, 10.03, accuracy: 0.00001)
    }
    func testLinkedAndSeparateEdgesClampToNeighbors() {
        let moved = Karaoke.nudge(timed(), at(0, 1), .start, 0.1)
        XCTAssertEqual(moved[0].units[0].end!, 10.6, accuracy: 0.00001)
        XCTAssertEqual(moved[0].units[1].start!, 10.6, accuracy: 0.00001)
        let far = Karaoke.nudge(timed(), at(0, 1), .start, -5)
        XCTAssertEqual(far[0].units[1].start!, 10.03, accuracy: 0.00001)
        let end = Karaoke.nudge(timed(), at(0, 1), .end, 2)
        XCTAssertEqual(end[0].units[1].end, 11.5)
        XCTAssertEqual(end[0].units[2].start, 11.5)
    }
    func testSplitMergeAndClear() {
        let merged = Karaoke.mergeNext(timed(), at(0, 0))
        XCTAssertEqual(merged[0].units[0], u(0, 2, 10, 11))
        let split = Karaoke.splitUnit(merged, at(0, 0))
        XCTAssertEqual(Array(split[0].units.prefix(2)), [u(0, 1, 10, 10.5), u(1, 2, 10.5, 11)])
        XCTAssertEqual(Karaoke.splitUnit(timed(), at(0, 0)), timed())
        XCTAssertEqual(Karaoke.lineStatus(Karaoke.clearLine(timed(), 0), 0, nil).status, "empty")
    }
    func testInputDoesNotMutate() {
        let before = timed(), snapshot = before
        _ = Karaoke.nudge(before, at(0, 0), .end, 0.2)
        _ = Karaoke.splitUnit(before, at(0, 0))
        _ = Karaoke.tapDown(before, at(0, 2), 3, .hold)
        XCTAssertEqual(before, snapshot)
    }
    func testDonePartialOverlapAndBackwards() {
        let lines: [TimingDraftLine] = [
            .init(text: "あい", timeSec: 1, units: [u(0, 1, 1, 1.5), u(1, 2, 1.5, 2)]),
            .init(text: "う", timeSec: 1.8, units: [u(0, 1)]),
            .init(text: "え", timeSec: 3, units: [u(0, 1, 3)])
        ]
        XCTAssertEqual(Karaoke.lineStatus(lines, 0, nil), .init("partial", "overlapNext"))
        XCTAssertEqual(Karaoke.lineStatus(lines, 0, 1.9).status, "partial")
        XCTAssertEqual(Karaoke.lineStatus(lines, 1, nil).status, "empty")
        XCTAssertEqual(Karaoke.lineStatus(lines, 2, nil).status, "partial")
        var fixed = lines; fixed[1].timeSec = 2.2
        XCTAssertEqual(Karaoke.lineStatus(fixed, 0, nil).status, "done")
        let backwards: [TimingDraftLine] = [.init(text: "あい", timeSec: 1, units: [u(0, 1, 2, 2.5), u(1, 2, 1, 1.2)])]
        XCTAssertEqual(Karaoke.lineStatus(backwards, 0, nil).status, "error")
    }
    func testSaveOnlyStampedUnits() {
        let payload = Karaoke.toSavePayload([.init(text: "あい", timeSec: 1, units: [u(0, 1, 1, 1.5), u(1, 2)])])
        XCTAssertEqual(payload, [.init(timeSec: 1, text: "あい", units: [.init(charStart: 0, charEnd: 1, startSec: 1, endSec: 1.5)])])
    }
    private var ktvLines: [KtvLine] { [.init(start: 10, end: 13), .init(start: 13.5, end: 16), .init(start: nil, end: nil), .init(start: 16.2, end: 19), .init(start: 30, end: 33)] }
    func testIntroAndFirstCountdown() {
        let intro = Karaoke.ktvLayout(ktvLines, 2)
        XCTAssertTrue(intro.intro); XCTAssertEqual(intro.slots, [0, 1]); XCTAssertEqual(intro.active, 0); XCTAssertEqual(intro.countdown, 0)
        let lead = Karaoke.ktvLayout(ktvLines, 7.5)
        XCTAssertFalse(lead.intro); XCTAssertEqual(lead.countdown, 3)
        XCTAssertEqual(Karaoke.ktvLayout(ktvLines, 9.2).countdown, 1)
    }
    func testFinishedSlotBecomesSecondFollowingLine() {
        XCTAssertEqual(Karaoke.ktvLayout(ktvLines, 11).slots, [0, 1])
        XCTAssertEqual(Karaoke.ktvLayout(ktvLines, 14).slots, [3, 1])
        XCTAssertEqual(Karaoke.ktvLayout(ktvLines, 14).active, 1)
        XCTAssertEqual(Karaoke.ktvLayout(ktvLines, 17).slots, [3, 4])
    }
    func testCountdownAfterLongGapAndEmptyAfterEnd() {
        XCTAssertEqual(Karaoke.ktvLayout(ktvLines, 13.2).countdown, 0)
        XCTAssertEqual(Karaoke.ktvLayout(ktvLines, 27.5).countdown, 3)
        let end = Karaoke.ktvLayout(ktvLines, 40)
        XCTAssertEqual(end.slots, [nil, nil]); XCTAssertNil(end.active)
    }
}
