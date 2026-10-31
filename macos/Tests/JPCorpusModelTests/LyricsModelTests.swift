import Foundation
import JPCore
import XCTest
@testable import JPCorpusModel

final class LyricsModelTests: XCTestCase {
    @MainActor
    func testSecretInputsAreWriteOnlyClearedOnFailureAndNeverEchoed() async throws {
        let fake = await fixture()
        let model = LyricsModel(songId: "001")
        await model.load(fake)
        model.developerToken = "secret-developer"; model.musicUserToken = "secret-user"
        await model.saveSources(fake)
        XCTAssertEqual(model.developerToken, ""); XCTAssertEqual(model.musicUserToken, "")
        XCTAssertEqual(model.errorMessage, "保存歌词来源失败；令牌输入已清空，请重试。")
        let calls = await fake.calls
        let saved = try XCTUnwrap(calls.last { $0.name == "lyricsSourcesSet" })
        let config = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(saved.arguments[0].utf8)) as? [String: Any])
        XCTAssertEqual(config["appleDeveloperToken"] as? String, "secret-developer")
        XCTAssertNil(config["appleDeveloperTokenSet"])
    }
    @MainActor
    func testPeaksBoundaryAndTimingNavigation() async throws {
        let fake = await fixture()
        await fake.respondToLyrics("audioPeaks", json: "[0,128,255]")
        let model = LyricsModel(songId: "001")
        await model.load(fake); await model.loadPeaks(fake)
        XCTAssertEqual(model.peaks, [0, 128, 255])
        model.select(line: 0, unit: 0)
        model.keyDown(at: 10); model.keyUp(at: 10.5)
        model.keyDown(at: 10.6); model.keyUp(at: 11)
        model.select(line: 0, unit: 1)
        model.nudge(.start, delta: -5)
        XCTAssertEqual(model.timing[0].units[1].start, 10.5)
        model.undo()
        XCTAssertEqual(model.cursor, .init(line: 0, unit: 1), "撤销不重置光标")
        XCTAssertEqual(model.timing[0].units[1].start, 10.6)
        model.clearLine()
        XCTAssertEqual(Karaoke.lineStatus(model.timing, 0, nil).status, "empty")
        XCTAssertEqual(model.selectedUnit, 0)
        let calls = await fake.calls
        XCTAssertTrue(calls.contains(.init(name: "audioPeaks", arguments: ["001", "100"])))
    }
    @MainActor
    func testLyricSourcesOffsetIsClampedAndChoiceReloadsLyrics() async throws {
        let fake = await fixture()
        let view = #"{"songId":"001","sources":[{"id":7,"songId":"001","kind":"sibling","provider":"001.lrc","sourceRef":"/m/001.lrc","role":"original","language":"","sourceType":"sibling","rawFormat":"lrc","hasRaw":true,"contentHash":"a","lineCount":2,"timedLineCount":2,"precision":"line","userModified":false,"missing":false,"offsetMs":0,"updatedAt":"2026-10-31T00:00:00Z"},{"id":8,"songId":"001","kind":"embedded","provider":"tag","sourceRef":"","role":"original","language":"","sourceType":"embedded","rawFormat":"text","hasRaw":true,"contentHash":"b","lineCount":2,"timedLineCount":0,"precision":"none","userModified":false,"missing":false,"offsetMs":0,"updatedAt":"2026-10-31T00:00:00Z"}],"selection":{"sourceId":8,"translationSourceId":null,"translationOff":false,"chosenBy":"legacy","offsetMs":0,"updatedAt":""},"recommended":7,"translationSourceId":null,"lookups":[]}"#
        await fake.respondToLyrics("lyricSourceView", json: view)
        await fake.respondToLyrics("lyricSourceApply", json: attached)
        let model = LyricsModel(songId: "001")
        await model.load(fake); await model.loadSources(fake)
        XCTAssertEqual(model.currentSource?.id, 8)
        XCTAssertTrue(model.sourceUntimed, "纯文本来源没有时间轴，偏移不可用")
        XCTAssertEqual(model.recommendedSource?.id, 7)
        let before = model.revision
        await model.useSource(7, client: fake)
        XCTAssertEqual(model.revision, before + 1, "换来源后歌词要重载")
        await model.setOffset(99_999, client: fake)
        let calls = await fake.calls
        XCTAssertTrue(calls.contains(.init(name: "lyricSourceApply", arguments: ["001", "7"])))
        XCTAssertTrue(calls.contains(.init(name: "lyricOffsetSet", arguments: ["001", "30000"])), "偏移超出上限要截到 ±30 秒")
        XCTAssertEqual(LyricsModel.formatOffset(-250), "-0.25 秒")
        XCTAssertEqual(LyricsModel.formatOffset(500), "+0.5 秒")
        XCTAssertEqual(SongLyricTimeline.trackMs(1_000, offsetMs: 250), 1_250)
        XCTAssertEqual(SongLyricTimeline.lyricMs(1_250, offsetMs: 250), 1_000)
    }
    private let attached = #"{"songId":"001","source":"edited","matched":"","lyricLines":1,"tokens":0,"credits":0,"correctionsRestored":0,"lrcPath":"/tmp/001.lrc"}"#
    private let idle = #"{"running":false,"finished":false,"cancelled":false,"done":0,"total":0,"stage":"","result":null,"error":null}"#
    private let running = #"{"running":true,"finished":false,"cancelled":false,"done":0,"total":2,"stage":"試験曲","result":null,"error":null}"#

    @MainActor
    private func fixture() async -> FakeCorpus {
        let fake = FakeCorpus()
        await fake.respondToLyrics("lyrics", json: #"[{"utteranceId":1,"lineIdx":0,"timeSec":1,"text":"きゃ 空","tokens":[]}]"#)
        await fake.respondToLyrics("wordTimings", json: "[]")
        await fake.respondToLyrics("songLineage", json: #"{"songId":"001","language":"ja","versions":[],"provenance":[]}"#)
        let config = String(decoding: try! JSONEncoder().encode(LyricsSources()), as: UTF8.self)
        await fake.respondToLyrics("lyricsSourcesGet", json: config)
        await fake.respondToLyrics("lyricsMissing", json: "[]")
        await fake.respondToLyrics("lyricsFillProgress", json: idle)
        return fake
    }

    @MainActor
    func testLineSaveParsesTimesAndKeepsDraftAfterFailure() async throws {
        let fake = await fixture()
        let model = LyricsModel(songId: "001")
        await model.load(fake)
        XCTAssertNil(model.errorMessage)
        model.edits[0].time = "1:23.456"
        model.edits[0].text = "あの空"
        model.note = "修正"
        await model.saveLines(fake) // 未配置写入响应：注入错误。
        XCTAssertTrue(model.dirty)
        XCTAssertEqual(model.edits[0].text, "あの空")
        XCTAssertNotNil(model.errorMessage)
        let calls = await fake.calls
        let saved = try XCTUnwrap(calls.last { $0.name == "lyricsSaveLines" })
        XCTAssertEqual(Array(saved.arguments.prefix(2)), ["001", "修正"])
        let lines = try decoded([EditedLyricLine].self, saved.arguments[2])
        XCTAssertEqual(lines[0].timeSec ?? 0, 83.46, accuracy: 0.0001)
        await fake.respondToLyrics("lyricsSaveLines", json: attached)
        await model.saveLines(fake)
        XCTAssertFalse(model.dirty)
        XCTAssertEqual(model.revision, 1)
    }

    @MainActor
    func testInvalidTimeAndPickerFailureAreVisible() async {
        let fake = await fixture()
        let model = LyricsModel(songId: "001")
        await model.load(fake)
        model.edits[0].time = "1:99"
        await model.saveLines(fake)
        XCTAssertTrue(model.errorMessage?.contains("时间格式") == true)
        let calls = await fake.calls
        XCTAssertFalse(calls.contains { $0.name == "lyricsSaveLines" })
        await model.importSelection(.failure(FakeError.injected), client: fake)
        XCTAssertTrue(model.errorMessage?.contains("没有读取权限") == true)
    }

    @MainActor
    func testImportRestoreAndSearchApplyUseTheSelectedIdentifiers() async {
        let fake = await fixture()
        for method in ["lyricsImportFile", "lyricsRestoreVersion", "lyricsTimedApply"] {
            await fake.respondToLyrics(method, json: attached)
        }
        await fake.respondToLyrics("lyricsTimedSearch", json: #"{"songId":"001","title":"試験曲","currentLines":1,"currentTimedLines":0,"candidates":[],"warnings":[]}"#)
        let model = LyricsModel(songId: "001")
        await model.load(fake)
        await model.importSelection(.success([URL(fileURLWithPath: "/tmp/test.lrc")]), client: fake)
        await model.restore(42, client: fake)
        await model.search(fake)
        XCTAssertNotNil(model.searchResult)
        await model.apply("selected", client: fake)
        XCTAssertNil(model.searchResult)
        let calls = await fake.calls
        XCTAssertTrue(calls.contains(.init(name: "lyricsImportFile", arguments: ["001", "/tmp/test.lrc"])))
        XCTAssertTrue(calls.contains(.init(name: "lyricsRestoreVersion", arguments: ["001", "42"])))
        XCTAssertTrue(calls.contains(.init(name: "lyricsTimedApply", arguments: ["001", "selected"])))
    }

    @MainActor
    func testSavingSourcesClearsCandidatesInvalidatedByTheCore() async throws {
        let fake = await fixture()
        let model = LyricsModel(songId: "001")
        await model.load(fake)
        await fake.respondToLyrics("lyricsTimedSearch", json: #"{"songId":"001","title":"試験曲","currentLines":1,"currentTimedLines":0,"candidates":[],"warnings":[]}"#)
        await model.search(fake)
        XCTAssertNotNil(model.searchResult)
        model.sources.qq = false
        let json = String(decoding: try JSONEncoder().encode(model.sources), as: UTF8.self)
        await fake.respondToLyrics("lyricsSourcesSet", json: json)
        await model.saveSources(fake)
        XCTAssertFalse(model.sources.qq)
        XCTAssertNil(model.searchResult)
        XCTAssertFalse(model.busy)
    }

    @MainActor
    func testJobsResumeCancelAndPreserveDraftOnCompletion() async {
        let fake = await fixture()
        let model = LyricsModel(songId: "001")
        await model.load(fake)
        await fake.respondToLyrics("lyricsFillProgress", json: running)
        await model.start(fake, all: false)
        XCTAssertTrue(model.running)
        await model.start(fake, all: true) // 重复启动不能抹掉运行中的任务。
        await model.cancel(fake)
        model.edits[0].text = "草稿"
        await fake.respondToLyrics("lyricsFillProgress", json: #"{"running":false,"finished":true,"cancelled":true,"done":1,"total":2,"stage":"","result":null,"error":null}"#)
        await model.poll(fake)
        XCTAssertFalse(model.running)
        XCTAssertEqual(model.edits[0].text, "草稿")
        XCTAssertTrue(model.dirty)
        let calls = await fake.calls
        XCTAssertEqual(calls.filter { $0.name == "lyricsFillStart" }.count, 1)
        XCTAssertTrue(calls.contains(.init(name: "lyricsFillStart", arguments: ["001"])))
        XCTAssertTrue(calls.contains(.init(name: "lyricsFillCancel", arguments: [])))
        let reopened = LyricsModel(songId: "001")
        await fake.respondToLyrics("lyricsFillProgress", json: running)
        await reopened.load(fake)
        XCTAssertTrue(reopened.running)
    }

    @MainActor
    func testTimingUsesScalarUnitsOnlyRecordsUserClocksAndSupportsUndo() async throws {
        let fake = await fixture()
        let model = LyricsModel(songId: "001")
        await model.load(fake)
        XCTAssertEqual(model.timing[0].units.map { LyricsModel.unitText(model.timing[0].text, unit: $0) }, ["きゃ ", "空"])
        XCTAssertTrue(model.timing[0].units.allSatisfy { $0.start == nil && $0.end == nil })
        model.keyDown(at: 1.85)
        model.keyUp(at: 2.45)
        XCTAssertEqual(model.timing[0].units[0].start ?? 0, 1.85, accuracy: 0.0001)
        model.undo()
        XCTAssertNil(model.timing[0].units[0].end)
        XCTAssertNil(model.timing[0].units[0].start, "按下和松开合为一步撤销")
        model.redo()
        XCTAssertEqual(model.timing[0].units[0].end ?? 0, 2.45, accuracy: 0.0001)
        await fake.respondToLyrics("lyricsSaveTiming", json: attached)
        await model.saveTiming(fake)
        let calls = await fake.calls
        let saved = try XCTUnwrap(calls.last { $0.name == "lyricsSaveTiming" })
        let lines = try decoded([TimedEditLine].self, saved.arguments[2])
        XCTAssertEqual(lines[0].units.count, 1, "未打点的单位不能生成时间")
        XCTAssertEqual(lines[0].text, "きゃ 空")
    }

    @MainActor
    func testTapModeUsesTheNextPressAndLastReleaseAsRealEnds() async {
        let fake = await fixture()
        let model = LyricsModel(songId: "001")
        await model.load(fake)
        model.tapMode = .tap
        model.keyDown(at: 1.85)
        model.keyUp(at: 1.95)
        XCTAssertEqual(model.selectedUnit, 1)
        XCTAssertNil(model.timing[0].units[0].end)
        model.keyDown(at: 2.35)
        XCTAssertEqual(model.timing[0].units[0].end ?? 0, 2.35, accuracy: 0.0001)
        model.keyUp(at: 2.85)
        XCTAssertEqual(model.timing[0].units[1].end ?? 0, 2.85, accuracy: 0.0001)
    }

    @MainActor
    func testTimingPreservesSourceGroupsAndCalibrationUsesScalarRange() async throws {
        let fake = await fixture()
        await fake.respondToLyrics("wordTimings", json: #"[{"utteranceId":1,"charStart":0,"charEnd":3,"startSec":1.2,"endSec":1.8,"source":"lrc"}]"#)
        let model = LyricsModel(songId: "001")
        await model.load(fake)
        XCTAssertEqual(model.timing[0].units[0].charEnd, 3)
        XCTAssertEqual(model.timing[0].units[0].end, 1.8)
        await model.calibrate(fake, position: 1.25, clear: false)
        await model.calibrate(fake, position: 0, clear: true)
        let calls = await fake.calls
        XCTAssertTrue(calls.contains(.init(name: "wordTimeSet", arguments: ["1", "0", "3", "1.25"])))
        XCTAssertTrue(calls.contains(.init(name: "wordTimeClear", arguments: ["1", "0"])))
        model.splitUnit()
        XCTAssertEqual(model.timing[0].units[0].start, 1.2, "Windows 拆分保留起点，以插值边界供继续调整")
        XCTAssertEqual(model.timing[0].units[0].end ?? 0, 1.4, accuracy: 0.0001)
    }
}
