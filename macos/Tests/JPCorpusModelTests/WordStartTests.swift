import XCTest
import JPCore
@testable import JPCorpusModel

final class WordStartTests: XCTestCase {
    func testScalarSpanAndRepeatedKeyword() throws {
        let hit = try VisualFixture.hit("001",left: "𠮷夜",keyword: "夜",text: "𠮷夜夜")
        let span = WordTimingLogic.keywordSpan(hit); XCTAssertEqual(span.start,2); XCTAssertEqual(span.end,3)
    }
    func testManualBeforeSourceThenLine() throws {
        let h = try VisualFixture.hit("001",keyword: "夜",text: "夜空",time: 10,end: 20)
        let source = WordTiming(utteranceId: 1,charStart: 0,charEnd: 2,startSec: 11,endSec: 12,source: "lrc")
        let manual = WordTiming(utteranceId: 1,charStart: 0,charEnd: 1,startSec: 10.5,endSec: nil,source: "manual")
        XCTAssertEqual(WordTimingLogic.wordStart(h,[source,manual])?.sec,10.5); XCTAssertEqual(WordTimingLogic.wordStart(h,[source])?.kind,"lrc"); XCTAssertEqual(WordTimingLogic.wordStart(h,[])?.kind,"line")
    }
    func testInvalidAndUnknownUnitsFallBack() throws {
        let h = try VisualFixture.hit("001",keyword: "夜",text: "夜空",time: 10,end: 20,endKind: "nextLine")
        for unit in [WordTiming(utteranceId: 2,charStart: 0,charEnd: 1,startSec: 11,endSec: 12,source: "lrc"),WordTiming(utteranceId: 1,charStart: 0,charEnd: 3,startSec: 11,endSec: 12,source: "lrc"),WordTiming(utteranceId: 1,charStart: 0,charEnd: 1,startSec: 11,endSec: 21,source: "lrc"),WordTiming(utteranceId: 1,charStart: 0,charEnd: 1,startSec: 11,endSec: 12,source: "estimated")] { XCTAssertEqual(WordTimingLogic.wordStart(h,[unit])?.sec,10) }
    }
    func testEstimatedListenEndDoesNotConstrainSource() throws {
        let h = try VisualFixture.hit("001",keyword: "夜",text: "夜空",time: 10,end: 12,endKind: "estimated")
        let unit = WordTiming(utteranceId: 1,charStart: 0,charEnd: 1,startSec: 13,endSec: 14,source: "lrc")
        XCTAssertEqual(WordTimingLogic.wordStart(h,[unit])?.sec,13)
    }
    func testConflictingSourcesAndMissingClock() throws {
        let h = try VisualFixture.hit("001",keyword: "夜",text: "夜空",time: 10)
        let a = WordTiming(utteranceId: 1,charStart: 0,charEnd: 1,startSec: 11,endSec: nil,source: "manual"), b = WordTiming(utteranceId: 1,charStart: 0,charEnd: 1,startSec: 12,endSec: nil,source: "manual")
        XCTAssertEqual(WordTimingLogic.wordStart(h,[a,b])?.sec,10); XCTAssertNil(WordTimingLogic.wordStart(try VisualFixture.hit("001"),[]))
        XCTAssertEqual(WordTimingLogic.lyricEndBound(.nan,20),20); XCTAssertFalse(WordTimingLogic.validLyricTime(-1))
    }
}
