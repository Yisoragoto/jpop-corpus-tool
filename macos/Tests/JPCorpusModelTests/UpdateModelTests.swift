import Foundation
import JPCore
import XCTest
@testable import JPCorpusModel

private func decodeUpdate<T: Decodable>(_ text: String) throws -> T { try JSONDecoder().decode(T.self, from: Data(text.utf8)) }
private let preferencesJSON = #"{"channel":"stable","current":"0.3.3","buildId":"local-test","skippedStable":"","skippedDebug":""}"#
private let statusJSON = #"{"channel":"stable","buildId":"local-test","skipped":false,"message":"","current":"0.3.3","updateAvailable":true,"latest":{"buildId":null,"version":"0.3.4","tag":"v0.3.4","name":"新版","notes":"改动","publishedAt":"2026-10-25","url":"https://github.com/Yisoragoto/jpop-corpus-tool/releases/tag/v0.3.4","prerelease":false,"installer":{"name":"windows-setup.exe"}}}"#
private actor FakeUpdates: UpdateCalling {
    var prefs: UpdatePreferences
    var result: UpdateStatus
    var checks = 0
    var failed = false
    init(skipped: Bool = false, debug: Bool = false, failed: Bool = false) throws {
        prefs = try decodeUpdate(preferencesJSON); result = try decodeUpdate(statusJSON)
        result.skipped = skipped; self.failed = failed
        if debug { prefs.channel = .debug; result.channel = .debug; result.latest?.buildId = "new-debug" }
    }
    func updatePreferences() async throws -> UpdatePreferences { prefs }
    func updateCheck() async throws -> UpdateStatus { checks += 1; if failed { throw CoreError.open("离线") }; return result }
    func updateSetChannel(_ channel: UpdateChannel) async throws -> UpdatePreferences { prefs.channel = channel; return prefs }
    func updateSkip(_ channel: UpdateChannel, id: String) async throws -> UpdatePreferences {
        if channel == .stable { prefs.skippedStable = id } else { prefs.skippedDebug = id }; return prefs
    }
    func updateChangelog() async throws -> [UpdateRelease] { if failed { throw CoreError.open("离线") }; return [result.latest!] }
    func checkCount() -> Int { checks }
    func setFailed(_ value: Bool) { failed = value }
}

@MainActor
final class UpdateModelTests: XCTestCase {
    private func defaults() -> UserDefaults { UserDefaults(suiteName: "update-tests-\(UUID().uuidString)")! }
    func testStartupChecksOnceAndOffersSkip() async throws {
        let client = try FakeUpdates(), model = UpdateModel(defaults: defaults(), client: client)
        await model.startup(); await model.startup()
        let count = await client.checkCount()
        XCTAssertEqual(count, 1); XCTAssertTrue(model.showDialog); XCTAssertTrue(model.offerSkip); XCTAssertFalse(model.busy)
    }
    func testFailedCheckKeepsEarlierResultButMarksItStale() async throws {
        let client = try FakeUpdates(), model = UpdateModel(defaults: defaults(), client: client)
        await model.check()
        XCTAssertFalse(model.statusIsStale)
        let first = try XCTUnwrap(model.statusCheckedAt)
        await client.setFailed(true); await model.check()
        XCTAssertTrue(model.statusIsStale, "这次失败了，手上的结果是上一次的")
        XCTAssertEqual(model.statusCheckedAt, first)
        XCTAssertNotNil(model.lastCheckError)
        await client.setFailed(false); await model.check()
        XCTAssertFalse(model.statusIsStale)
    }
    func testSkippedVersionDoesNotPromptButManualCheckStillShowsIt() async throws {
        let model = UpdateModel(defaults: defaults(), client: try FakeUpdates(skipped: true))
        await model.startup(); XCTAssertFalse(model.showDialog)
        await model.check(); XCTAssertTrue(model.status?.updateAvailable == true)
    }
    func testAutomaticPreferencePersistsAndDisablesNetwork() async throws {
        let prefs = defaults(), client = try FakeUpdates()
        UpdateModel(defaults: prefs).autoCheck = false
        let model = UpdateModel(defaults: prefs, client: client); await model.startup()
        let count = await client.checkCount()
        XCTAssertEqual(count, 0); XCTAssertNotNil(model.preferences); XCTAssertFalse(model.autoCheck)
    }
    func testDebugSkipUsesBuildIdentityAndClearKeepsChannel() async throws {
        let model = UpdateModel(defaults: defaults(), client: try FakeUpdates(debug: true))
        await model.check(); await model.skip()
        XCTAssertEqual(model.preferences?.skippedDebug, "new-debug"); XCTAssertEqual(model.preferences?.skippedStable, "")
        await model.clearSkipped(); XCTAssertEqual(model.preferences?.skippedDebug, ""); XCTAssertEqual(model.preferences?.channel, .debug)
    }
    func testStableSkipUsesVersion() async throws {
        let model = UpdateModel(defaults: defaults(), client: try FakeUpdates())
        await model.check(); await model.skip(); XCTAssertEqual(model.preferences?.skippedStable, "0.3.4")
    }
    func testSwitchChannelInvalidatesStatus() async throws {
        let model = UpdateModel(defaults: defaults(), client: try FakeUpdates())
        await model.check(); await model.setChannel(.debug)
        XCTAssertNil(model.status); XCTAssertEqual(model.preferences?.channel, .debug); XCTAssertFalse(model.busy)
    }
    func testOfflineErrorsReleaseBusyAndAutomaticFailureIsQuiet() async throws {
        let store = defaults()
        let model = UpdateModel(defaults: store, client: try FakeUpdates(failed: true))
        await model.startup(); XCTAssertNil(model.errorMessage); XCTAssertFalse(model.busy)
        XCTAssertNotNil(model.lastCheckedAt, "自动检查失败不弹窗，但要记下来")
        XCTAssertEqual(model.lastCheckError?.contains("离线"), true)
        let reopened = UpdateModel(defaults: store)
        XCTAssertEqual(reopened.lastCheckError, model.lastCheckError, "重启后设置里仍看得到上次失败")
        await model.check(); XCTAssertNotNil(model.errorMessage); XCTAssertFalse(model.busy)
        await model.changelog(); XCTAssertFalse(model.showChangelog); XCTAssertFalse(model.busy)
    }
    func testChangelogPreservesNotesAndIgnoresWindowsInstaller() async throws {
        let model = UpdateModel(defaults: defaults(), client: try FakeUpdates())
        await model.changelog(); XCTAssertTrue(model.showChangelog); XCTAssertEqual(model.releases.first?.notes, "改动")
    }
}
