@testable import JPCorpusModel
import XCTest

/// 和 Windows LoopAdjustBar 的 formatLoopTime / formatDelta / stepLabel 同样的写法
final class LoopFormatTests: XCTestCase {
    func testTimeRoundsToTenthsAndPadsSeconds() {
        XCTAssertEqual(LoopFormat.time(0), "0:00.0")
        XCTAssertEqual(LoopFormat.time(12.34), "0:12.3")
        XCTAssertEqual(LoopFormat.time(59.96), "1:00.0", "进位到下一分钟")
        XCTAssertEqual(LoopFormat.time(125.06), "2:05.1")
        XCTAssertEqual(LoopFormat.time(-3), "0:00.0")
    }

    func testDeltaUsesTrueMinusAndIsEmptyWhenUnchanged() {
        XCTAssertEqual(LoopFormat.delta(0), "")
        XCTAssertEqual(LoopFormat.delta(300), "+0.3")
        XCTAssertEqual(LoopFormat.delta(-200), "\u{2212}0.2")
        XCTAssertEqual(LoopFormat.step(-500), "\u{2212}0.5")
        XCTAssertEqual(LoopFormat.step(100), "+0.1")
    }
}
