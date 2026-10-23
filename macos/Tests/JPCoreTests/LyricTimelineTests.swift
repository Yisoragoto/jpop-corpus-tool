import Foundation
import XCTest

@testable import JPCore

/// Invented phrases with the same JSON shape as the Rust lyricTimeline method.
final class LyricTimelineTests: XCTestCase {
    private func line(_ time: Double? = 1) -> LyricLine {
        LyricLine(utteranceId: 91, lineIdx: 0, timeSec: time, text: "𠮷😀か\u{3099}光", tokens: [])
    }

    private func fixture() throws -> LineTimeline {
        let json = #"{"utteranceId":91,"mode":"karaoke","units":[{"charStart":0,"charEnd":2,"startSec":1,"endSec":1.4,"source":"lrc"},{"charStart":2,"charEnd":5,"startSec":2,"endSec":2.4,"source":"manual"}]}"#
        return try JSONDecoder().decode(LineTimeline.self, from: Data(json.utf8))
    }

    func testUnicodeScalarRangesPreserveWholeActualWords() throws {
        let timeline = try fixture()
        let units = try XCTUnwrap(timeline.validatedUnits(for: line(), endSec: 4))
        XCTAssertEqual(units.count, 2)
        XCTAssertEqual(units[0].text(in: line().text), "𠮷😀")
        XCTAssertEqual(units[1].text(in: line().text), "か\u{3099}光")
        XCTAssertEqual(timeline.displayMode(for: line()), .karaoke)
        XCTAssertEqual(timeline.displayMode(for: line(nil)), .karaoke)
    }

    func testProgressUsesOnlyTheActualIntervalAndLeavesRestsIntact() throws {
        let timeline = try fixture()
        let first = timeline.units[0]
        let second = timeline.units[1]
        XCTAssertEqual(first.progress(at: 0.9), 0)
        XCTAssertEqual(first.progress(at: 1.2), 0.5, accuracy: 0.000001)
        XCTAssertEqual(first.progress(at: 1.8), 1)
        XCTAssertEqual(second.progress(at: 1.8), 0)
        XCTAssertEqual(second.progress(at: 2.2), 0.5, accuracy: 0.000001)
        XCTAssertEqual(second.progress(at: 2.4), 1)
        XCTAssertEqual(first.progress(at: .nan), 0)
        XCTAssertEqual(first.progress(at: .infinity), 0)
    }

    func testInvalidCoverageTimesSourcesAndIdentityDegradeTheWholeLine() throws {
        let good = try fixture()
        var cases: [LineTimeline] = []
        var bad = good; bad.units.removeLast(); cases.append(bad)
        bad = good; bad.units[1].charStart = 3; cases.append(bad)
        bad = good; bad.units[1].charEnd = 6; cases.append(bad)
        bad = good; bad.units[0].charStart = -1; cases.append(bad)
        bad = good; bad.units[1].startSec = .nan; cases.append(bad)
        bad = good; bad.units[1].endSec = .infinity; cases.append(bad)
        bad = good; bad.units[1].endSec = 1.9; cases.append(bad)
        bad = good; bad.units[1].startSec = 1.3; cases.append(bad)
        bad = good; bad.units[1].source = "estimate"; cases.append(bad)
        bad = good; bad.utteranceId = 92; cases.append(bad)
        for timeline in cases {
            XCTAssertNil(timeline.validatedUnits(for: line()))
            XCTAssertEqual(timeline.displayMode(for: line()), .lineSynced)
        }
        XCTAssertEqual(good.displayMode(for: line(), endSec: 2.3), .lineSynced)
        XCTAssertEqual(good.displayMode(for: line(.nan)), .plain)
        XCTAssertEqual(good.displayMode(for: line(-1)), .plain)
    }

    func testFallbackJSONNeverProducesKaraokeOrSeekUnits() throws {
        for mode in ["lineSynced", "plain"] {
            let json = "{\"utteranceId\":91,\"mode\":\"\(mode)\",\"units\":[]}"
            let timeline = try JSONDecoder().decode(LineTimeline.self, from: Data(json.utf8))
            XCTAssertNil(timeline.validatedUnits(for: line()))
            XCTAssertEqual(timeline.displayMode(for: line()), .lineSynced)
            XCTAssertEqual(timeline.displayMode(for: line(nil)), .plain)
        }
    }

    func testSharedCrossClientRuleFixturesDecodeAndRenderAtTheirActualPrecision() throws {
        struct Case: Decodable {
            var name: String
            var line: LyricLine
            var endSec: Double?
            var expected: LineTimeline
        }
        let url = try XCTUnwrap(Bundle.module.url(forResource: "LyricTimeline", withExtension: "json", subdirectory: "Fixtures"))
        let cases = try JSONDecoder().decode([Case].self, from: Data(contentsOf: url))
        XCTAssertGreaterThanOrEqual(cases.count, 14)
        for fixture in cases {
            let timeline = fixture.expected
            XCTAssertEqual(timeline.displayMode(for: fixture.line, endSec: fixture.endSec), timeline.mode, fixture.name)
            if timeline.mode == .karaoke {
                let units = try XCTUnwrap(timeline.validatedUnits(for: fixture.line, endSec: fixture.endSec), fixture.name)
                XCTAssertEqual(units.compactMap { $0.text(in: fixture.line.text) }.joined(), fixture.line.text, fixture.name)
                for unit in units {
                    XCTAssertEqual(unit.progress(at: unit.startSec), 0, fixture.name)
                    XCTAssertEqual(unit.progress(at: unit.endSec), 1, fixture.name)
                }
            } else {
                XCTAssertNil(timeline.validatedUnits(for: fixture.line), fixture.name)
                XCTAssertTrue(timeline.units.isEmpty, fixture.name)
            }
        }
    }

    func testBridgeDecodesAllFixtureLinesAndFallsBackWithoutSourceTimings() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("jp-timeline-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let db = try CorpusCore.writeFixture(in: dir)
        let core = try CorpusCore(databaseURL: db)
        let client = CorpusClient(core: core)
        let lines = try await client.lyrics("001")
        let timelines = try await client.lyricTimeline("001")
        XCTAssertEqual(Set(lines.map(\.utteranceId)), Set(timelines.map(\.utteranceId)))
        XCTAssertTrue(timelines.allSatisfy { $0.mode == .lineSynced && $0.units.isEmpty })
        let direct = try core.lyricTimeline("001")
        XCTAssertEqual(direct, timelines)
        let missing = try await client.lyricTimeline("missing")
        XCTAssertTrue(missing.isEmpty)
    }
}
