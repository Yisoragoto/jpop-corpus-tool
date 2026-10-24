import Foundation
import XCTest
@testable import JPCore

/// 真静态库：编辑、历史、打轴与离线队列的 JSON 形状。
final class LyricsBridgeTests: XCTestCase {
    private var dir: URL!
    private var client: CorpusClient!
    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("jpcore-lyrics-\(UUID().uuidString)")
        let db = try CorpusCore.writeFixture(in: dir)
        client = CorpusClient(core: try CorpusCore(databaseURL: db))
    }
    override func tearDownWithError() throws {
        client = nil
        try? FileManager.default.removeItem(at: dir)
    }

    func testEditRestoreImportAndTimingRoundTrip() async throws {
        let first = [EditedLyricLine(timeSec: 1, text: "あの空"), EditedLyricLine(timeSec: 4, text: "君の声")]
        _ = try await client.lyricsSaveLines("001", lines: first, note: "初版")
        let lineage = try await client.songLineage("001")
        let old = try XCTUnwrap(lineage.versions.first { $0.isCurrent })
        let raw = try await client.lyricVersionRaw(old.id)
        XCTAssertTrue(raw?.contains("あの空") == true)
        _ = try await client.lyricsSaveLines("001", lines: [EditedLyricLine(timeSec: 2, text: "星の空")], note: "改稿")
        let changed = try await client.lyrics("001")
        XCTAssertEqual(changed[0].text, "星の空")
        _ = try await client.lyricsRestoreVersion("001", versionId: old.id)
        let restored = try await client.lyrics("001")
        XCTAssertEqual(restored.map(\.text), first.map(\.text))
        let row = try XCTUnwrap(restored.first)
        try await client.wordTimeSet(utteranceId: row.utteranceId, charStart: 0, charEnd: 1, startSec: 1.25)
        let marks = try await client.wordTimings([row.utteranceId])
        XCTAssertTrue(marks.contains { $0.source == "manual" && $0.startSec == 1.25 })
        try await client.wordTimeClear(utteranceId: row.utteranceId, charStart: 0)
        let cleared = try await client.wordTimings([row.utteranceId])
        XCTAssertFalse(cleared.contains { $0.source == "manual" })
        let timed = first.map { line in
            TimedEditLine(timeSec: line.timeSec, text: line.text, units: [TimedEditUnit(charStart: 0, charEnd: 3, startSec: line.timeSec ?? 0, endSec: (line.timeSec ?? 0) + 1)])
        }
        _ = try await client.lyricsSaveTiming("001", lines: timed, note: "打轴")
        let timeline = try await client.lyricTimeline("001")
        XCTAssertEqual(timeline.first?.mode, .karaoke)
        let file = dir.appendingPathComponent("chosen.lrc")
        try "[00:03.000]夜の声\n".write(to: file, atomically: true, encoding: .utf8)
        _ = try await client.lyricsImportFile("001", path: file.path)
        let imported = try await client.lyrics("001")
        XCTAssertEqual(imported[0].text, "夜の声")
        let validated = try await client.lyricsRevalidate("001")
        XCTAssertEqual(validated, 1)
    }

    func testOfflineSourcesSearchAndJobProgressDecode() async throws {
        var settings = try await client.lyricsSourcesGet()
        settings.netease = false; settings.qq = false; settings.kugou = false
        settings.amll = false; settings.lrclib = false; settings.apple = false
        let saved = try await client.lyricsSourcesSet(settings)
        XCTAssertFalse(saved.qq)
        let candidates = try await client.lyricsTimedSearch("001")
        XCTAssertEqual(candidates.songId, "001")
        XCTAssertTrue(candidates.candidates.isEmpty)
        _ = try await client.lyricsMissing()
        _ = try await client.lyricsTimedTargets()
        try await client.lyricsFillStart(songIds: [])
        var progress = try await client.lyricsFillProgress()
        for _ in 0..<200 {
            if progress.finished { break }
            try await Task.sleep(for: .milliseconds(10))
            progress = try await client.lyricsFillProgress()
        }
        XCTAssertTrue(progress.finished)
        XCTAssertFalse(progress.running)
        XCTAssertNil(progress.error)
        XCTAssertEqual(progress.result?.upgraded, 0)
        let running = try await client.lyricsFillRunning()
        XCTAssertFalse(running)
        try await client.lyricsUpgradeStart()
        var upgrade = try await client.lyricsUpgradeProgress()
        for _ in 0..<200 {
            if upgrade.finished { break }
            try await Task.sleep(for: .milliseconds(10))
            upgrade = try await client.lyricsUpgradeProgress()
        }
        XCTAssertTrue(upgrade.finished)
        let status = try await client.lyricsUpgradeStatus()
        XCTAssertFalse(status.running)
        try await client.lyricsFillCancel()
        try await client.lyricsUpgradeCancel()
    }
}
