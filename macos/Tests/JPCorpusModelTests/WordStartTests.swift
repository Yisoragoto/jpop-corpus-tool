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
    func testCalibrationStaysInsideTheLine() throws {
        let h = try VisualFixture.hit("001",keyword: "夜",text: "夜空",time: 10,end: 14,endKind: "nextLine")
        func value(_ r: Result<Double,WordTimingLogic.CalibrationProblem>) -> Double? { if case .success(let v) = r { return v }; return nil }
        XCTAssertEqual(WordTimingLogic.calibration(h,playing: false,position: 11,duration: 99),.failure(.notPlaying),"暂停时不记")
        XCTAssertEqual(value(WordTimingLogic.calibration(h,playing: true,position: 11.15,duration: 99)) ?? -1,11,accuracy: 1e-9)
        XCTAssertEqual(WordTimingLogic.calibration(h,playing: true,position: 9,duration: 99),.failure(.beforeLine))
        XCTAssertEqual(value(WordTimingLogic.calibration(h,playing: true,position: 10,duration: 99)),10,"按快了一点记成行首")
        XCTAssertEqual(WordTimingLogic.calibration(h,playing: true,position: 14.2,duration: 99),.failure(.afterLine))
        // 估计的行尾不卡人，曲目时长卡
        let est = try VisualFixture.hit("001",keyword: "夜",text: "夜空",time: 10,end: 12,endKind: "estimated")
        XCTAssertEqual(value(WordTimingLogic.calibration(est,playing: true,position: 13,duration: 99)) ?? -1,12.85,accuracy: 1e-9)
        XCTAssertEqual(WordTimingLogic.calibration(est,playing: true,position: 13,duration: 12.5),.failure(.afterLine))
        // 微调不能推出这一行
        XCTAssertEqual(WordTimingLogic.nudged(h,from: 10.05,by: -0.1,duration: nil),.failure(.beforeLine))
        XCTAssertEqual(value(WordTimingLogic.nudged(h,from: 10.05,by: 0.1,duration: nil)) ?? -1,10.15,accuracy: 1e-9)
    }
    func testStartLabelsTellWordFromLineStart() {
        XCTAssertEqual(WordTimingLogic.startLabel(WordStart(sec: 1,kind: "manual")),"已校准")
        XCTAssertEqual(WordTimingLogic.startLabel(WordStart(sec: 1,kind: "lrc")),"逐字定位")
        XCTAssertEqual(WordTimingLogic.startLabel(WordStart(sec: 1,kind: "line")),"从行首播放")
        XCTAssertEqual(WordTimingLogic.startLabel(nil),"没有时间戳")
    }
    func testConflictingSourcesAndMissingClock() throws {
        let h = try VisualFixture.hit("001",keyword: "夜",text: "夜空",time: 10)
        let a = WordTiming(utteranceId: 1,charStart: 0,charEnd: 1,startSec: 11,endSec: nil,source: "manual"), b = WordTiming(utteranceId: 1,charStart: 0,charEnd: 1,startSec: 12,endSec: nil,source: "manual")
        XCTAssertEqual(WordTimingLogic.wordStart(h,[a,b])?.sec,10); XCTAssertNil(WordTimingLogic.wordStart(try VisualFixture.hit("001"),[]))
        XCTAssertEqual(WordTimingLogic.lyricEndBound(.nan,20),20); XCTAssertFalse(WordTimingLogic.validLyricTime(-1))
    }
}
