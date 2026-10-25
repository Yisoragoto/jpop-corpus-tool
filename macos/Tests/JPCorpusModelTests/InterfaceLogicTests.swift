import XCTest
import Foundation
import JPCore
@testable import JPCorpusModel

/// relativeTime.test.ts 2、listen.test.ts 2、backfill.test.ts 3。
final class InterfaceLogicTests: XCTestCase {
    private let now = ISO8601DateFormatter().date(from: "2026-10-15T12:00:00Z")!
    func testRelativeTimes() {
        for (input,expected) in [("2026-10-15T11:59:30Z","刚刚"),("2026-10-15T11:30:00Z","30 分钟前"),("2026-10-15T02:00:00Z","10 小时前"),("2026-10-13T15:44:00Z","1 天前"),("2026-06-02T16:31:24Z","4 个月前")] { XCTAssertEqual(InterfaceLogic.formatRelative(input,now: now),expected) }
    }
    func testBadDatesEmpty() { XCTAssertEqual(InterfaceLogic.formatRelative("",now: now),""); XCTAssertEqual(InterfaceLogic.formatRelative("昨天",now: now),"") }
    private func span(_ song: String = "001",_ start: Double?,_ end: Double?,playing: Bool = true) -> Bool {
        InterfaceLogic.isPlayingSpan(currentSong: "001",playing: playing,position: 12,songId: song,startSec: start,endSec: end)
    }
    func testPlayingOnlyInSameSongAndHalfOpenSpan() {
        XCTAssertTrue(span("001",10,14)); XCTAssertFalse(span("002",10,14)); XCTAssertFalse(span("001",10,14,playing: false)); XCTAssertFalse(span("001",13,20)); XCTAssertFalse(span("001",5,12))
    }
    func testLeadAndUntimed() { XCTAssertTrue(span("001",12.4,15)); XCTAssertFalse(span("001",nil,nil)) }
    func testNoBackfillNoNote() { XCTAssertEqual(InterfaceLogic.backfillNote(songs: 0,tokens: 0,error: ""),"") }
    func testBackfillReportsSongs() { let s = InterfaceLogic.backfillNote(songs: 16,tokens: 4200,error: ""); XCTAssertTrue(s.contains("16 首")); XCTAssertTrue(s.contains("振假名")) }
    func testBackfillErrorPointsToRetry() { let s = InterfaceLogic.backfillNote(songs: 0,tokens: 0,error: "分词失败：…"); XCTAssertTrue(s.contains("分词失败：…")); XCTAssertTrue(s.contains("补齐缺失分词")) }
}
