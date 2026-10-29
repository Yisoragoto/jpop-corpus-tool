import Foundation
import JPCore
import Observation
import XCTest
@testable import JPCorpusModel

@MainActor
private final class FakePlaybackEngine: PlaybackEngine {
    var isLoaded = false
    var isPlaying = false
    var position: Double = 0
    var duration: Double = 30
    var rate: Float = 1
    var pitch = 0
    var volume: Float = 1
    var spectrumEnabled = true
    var spectrum = [Float](repeating: 0, count: 48)
    var onTick: ((Double) -> Void)?
    var onStateChange: (() -> Void)?
    var lastError: String?
    var failPlay = false
    var seeks: [Double] = []
    func load(_ url: URL) throws { isLoaded = true; position = 0 }
    func unload() { isLoaded = false; isPlaying = false; position = 0 }
    func play() throws {
        if failPlay { throw FakeError.injected }
        isPlaying = isLoaded
    }
    func pause() { isPlaying = false }
    func seek(to seconds: Double) { position = seconds; seeks.append(seconds) }
    func emit(_ seconds: Double, ended: Bool = false) {
        position = seconds
        if ended { isPlaying = false }
        onTick?(seconds)
    }
    func configurationChanged(recoverySucceeds: Bool) {
        let resume = isPlaying
        isPlaying = false // Simulate AVAudioEngine being stopped by the system.
        lastError = recoverySucceeds ? nil : "音频输出设备变化后无法恢复播放：测试错误"
        if recoverySucceeds { isPlaying = resume }
        onStateChange?()
    }
}

final class PlayerModelTests: XCTestCase {
    private let trackJSON = #"{"id":"001","title":"試験曲","artist":"歌手","album":"","year":"","genre":"","audioPath":"tone.flac","coverPath":"","durationSec":99,"lineCount":2}"#

    @MainActor
    func testCloudUsesExactWordStartAndKwicKeepsLeadIn() async throws {
        let (app,engine,_,_) = try await fixture()
        await app.player.playFrom(songId: "001",utteranceId: 1,timeSec: 2.5,leadIn: 0)
        XCTAssertEqual(engine.position,2.5,accuracy: 0.000001); XCTAssertEqual(app.player.focusUtterance,1)
        await app.player.playFrom(songId: "001",utteranceId: 1,timeSec: 2.5)
        XCTAssertEqual(engine.position,2.1,accuracy: 0.000001); XCTAssertNil(app.player.stopAt)
    }

    @MainActor
    func testDeletingCurrentSongStopsAndClearsPlaybackBeforeDatabaseWrite() async throws {
        let (app, engine, _, _) = try await fixture()
        await app.player.toggleLoop(1)
        await app.player.prepareDeletion("another-song")
        XCTAssertNotNil(app.player.track)
        await app.player.prepareDeletion("001")
        XCTAssertNil(app.player.track); XCTAssertFalse(app.player.isPlaying); XCTAssertFalse(app.player.canPlay)
        XCTAssertFalse(engine.isLoaded); XCTAssertTrue(app.player.lines.isEmpty); XCTAssertTrue(app.player.spans.isEmpty)
        XCTAssertNil(app.player.loop); XCTAssertNil(app.player.stopAt); XCTAssertNil(app.player.focusUtterance)
        engine.emit(5)
        for _ in 0..<5 { await Task.yield() }
        XCTAssertFalse(engine.isPlaying)
    }

    @MainActor
    private func fixture(openClient: (@Sendable (URL) async throws -> any CorpusCalling)? = nil) async throws -> (AppModel, FakePlaybackEngine, FakeCorpus, UserDefaults) {
        let suite = "PlayerModelTests-\(UUID())"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        addTeardownBlock { defaults.removePersistentDomain(forName: suite) }
        let corpus = FakeCorpus()
        await corpus.respondToLyrics("track", json: trackJSON)
        await corpus.respondToLyrics("lyrics", json: #"[{"utteranceId":1,"lineIdx":0,"timeSec":2,"text":"きゃ","tokens":[]},{"utteranceId":2,"lineIdx":1,"timeSec":5,"text":"空","tokens":[]}]"#)
        await corpus.respondToLyrics("lineSpans", json: #"[{"utteranceId":1,"lineIdx":0,"startSec":2,"endSec":5,"endKind":"nextLine"},{"utteranceId":2,"lineIdx":1,"startSec":5,"endSec":8,"endKind":"estimated"}]"#)
        await corpus.respondToLyrics("listenWindow", json: #"{"songId":"001","startSec":1.8,"endSec":5,"focusUtteranceId":1,"focusStartSec":2,"focusEndSec":5,"utteranceIds":[1]}"#)
        await corpus.respondToLyrics("resolveMedia", json: #""/tmp/tone.flac""#)
        let engine = FakePlaybackEngine()
        let app = openClient.map { AppModel(defaults: defaults, client: corpus, playbackEngine: engine, openClient: $0) }
            ?? AppModel(defaults: defaults, client: corpus, playbackEngine: engine)
        await app.player.open(try decoded(Track.self, trackJSON), autoplay: false)
        XCTAssertNil(app.player.lastError)
        XCTAssertTrue(app.player.canPlay)
        return (app, engine, corpus, defaults)
    }

    @MainActor
    private func waitParked(_ corpus: FakeCorpus, _ key: String) async {
        var tries = 0
        while tries < 1000 {
            if await corpus.isParked(key) { return }
            await Task.yield()
            tries += 1
        }
    }

    /// 连点两条试听：先点的那条慢，不能在后点的那首上设停止点、也不能把歌换回去
    @MainActor
    func testSlowAuditionCannotTakeOverTheLaterOne() async throws {
        let (app, _, corpus, _) = try await fixture()
        let second = trackJSON.replacingOccurrences(of: #""id":"001""#, with: #""id":"002""#).replacingOccurrences(of: "tone.flac", with: "two.flac")
        await corpus.respondToLyrics("track:002", json: second)
        await corpus.respondToLyrics("resolveMedia:two.flac", json: #""/tmp/two.flac""#)
        await corpus.respondToLyrics("listenWindow:7,0,0", json: #"{"songId":"002","startSec":10,"endSec":12,"focusUtteranceId":7,"focusStartSec":10,"focusEndSec":12,"utteranceIds":[7]}"#)

        // 旧请求卡在查试听窗口
        await corpus.hold("listenWindow:7,0,0")
        let slowWindow = Task { await app.player.audition(7) }
        await waitParked(corpus, "listenWindow:7,0,0")
        await app.player.audition(1)
        await corpus.release("listenWindow:7,0,0")
        await slowWindow.value
        XCTAssertEqual(app.player.track?.id, "001")
        XCTAssertEqual(app.player.stopAt, 5)
        XCTAssertTrue(app.player.isPlaying)

        // 旧请求卡在加载音频
        await corpus.hold("resolveMedia:two.flac")
        let slowLoad = Task { await app.player.audition(7) }
        await waitParked(corpus, "resolveMedia:two.flac")
        await app.player.audition(1)
        await corpus.release("resolveMedia:two.flac")
        await slowLoad.value
        XCTAssertEqual(app.player.track?.id, "001")
        XCTAssertEqual(app.player.stopAt, 5, "旧请求的停止点（12 秒）不能套到这首上")
        XCTAssertTrue(app.player.isPlaying)

        // 被顶掉的加载要告诉调用方
        await corpus.hold("resolveMedia:two.flac")
        let superseded = Task { await app.player.open(try decoded(Track.self, second), autoplay: true) }
        await waitParked(corpus, "resolveMedia:two.flac")
        let current = await app.player.open(try decoded(Track.self, trackJSON), autoplay: false)
        await corpus.release("resolveMedia:two.flac")
        let stale = try await superseded.value
        XCTAssertTrue(current)
        XCTAssertFalse(stale)
        XCTAssertEqual(app.player.track?.id, "001")
    }

    /// 换库：旧库这首的收听记回旧库，播放器清空，新库里一笔都不写
    @MainActor
    func testSwitchingLibraryKeepsHistoryInTheOldLibraryAndClearsThePlayer() async throws {
        let other = FakeCorpus()
        let (app, engine, corpus, _) = try await fixture(openClient: { _ in other })
        app.player.play()
        for i in 0...30 { engine.emit(Double(i) / 10) }
        app.libraryPath = [.song("001")]
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("switch-\(UUID())", isDirectory: true)
        await app.createOrOpen(folder: folder)
        XCTAssertNil(app.errorMessage)
        XCTAssertTrue(app.client === other)
        let old = await corpus.recordedPlays
        XCTAssertEqual(old.count, 1)
        XCTAssertEqual(old.first?.songId, "001")
        XCTAssertNil(app.player.track)
        XCTAssertTrue(app.player.queue.isEmpty)
        XCTAssertFalse(engine.isLoaded)
        XCTAssertTrue(app.libraryPath.isEmpty, "详情页的歌曲 id 是旧库的")
        app.player.pause()
        for _ in 0..<5 { await Task.yield() }
        let new = await other.recordedPlays
        XCTAssertTrue(new.isEmpty)
    }

    /// 旧库还在导入：不让换库，播放器也不动
    @MainActor
    func testSwitchingLibraryIsRefusedWhileAnImportRuns() async throws {
        let other = FakeCorpus()
        let (app, _, corpus, _) = try await fixture(openClient: { _ in other })
        await corpus.setImportRunning(true)
        await app.createOrOpen(folder: FileManager.default.temporaryDirectory.appendingPathComponent("busy-\(UUID())", isDirectory: true))
        XCTAssertTrue(app.client === corpus)
        XCTAssertTrue(app.errorMessage?.contains("导入") == true)
        XCTAssertEqual(app.player.track?.id, "001")
        let jobs = await app.runningJobs()
        XCTAssertEqual(jobs, ["导入"])
    }

    @MainActor
    func testDeviceChangePreservesPositionTuningAndPlaybackIntent() async throws {
        let (app, engine, _, _) = try await fixture()
        let player = app.player
        player.setRate(1.25); player.setPitch(2); player.setVolume(0.37)
        await player.seek(to: 7.5)
        player.play()
        engine.configurationChanged(recoverySucceeds: true)
        XCTAssertTrue(engine.isPlaying); XCTAssertTrue(player.isPlaying)
        XCTAssertEqual(player.position, 7.5)
        XCTAssertEqual(engine.rate, 1.25); XCTAssertEqual(engine.pitch, 2)
        XCTAssertEqual(engine.volume, 0.37, accuracy: 0.0001)
        XCTAssertNil(player.lastError)
        player.pause()
        engine.configurationChanged(recoverySucceeds: true)
        XCTAssertFalse(engine.isPlaying); XCTAssertFalse(player.isPlaying)
        XCTAssertEqual(player.position, 7.5)
        XCTAssertNil(player.lastError)
    }

    @MainActor
    func testDeviceChangeFailureStopsModelImmediatelyAndDoesNotRestartLoop() async throws {
        let (app, engine, _, _) = try await fixture()
        let player = app.player
        await player.toggleLoop(1)
        engine.emit(4.99)
        engine.configurationChanged(recoverySucceeds: false)
        XCTAssertFalse(engine.isPlaying); XCTAssertFalse(player.isPlaying)
        XCTAssertEqual(player.position, 4.99)
        XCTAssertEqual(player.lastError, engine.lastError)
        XCTAssertNotNil(player.lastError)
        engine.emit(5.01)
        for _ in 0..<5 { await Task.yield() }
        XCTAssertFalse(engine.isPlaying); XCTAssertFalse(player.isPlaying)
        XCTAssertEqual(engine.seeks, [2])
    }

    @MainActor
    func testUnexpectedEngineStopCannotLeaveModelPlaying() async throws {
        let (app, engine, _, _) = try await fixture()
        app.player.play()
        engine.emit(7.5, ended: true)
        XCTAssertFalse(app.player.isPlaying)
        XCTAssertEqual(app.player.position, 7.5)
        XCTAssertNotNil(app.player.lastError)
    }

    @MainActor
    func testFiveSecondSeekClampsBothEndsAndUsesLiveClock() async throws {
        let (app, engine, _, _) = try await fixture()
        let player = app.player
        XCTAssertEqual(player.duration, 30, "真实解码时长优先于库里旧时长")
        await player.nudge(-5)
        XCTAssertEqual(player.position, 0)
        await player.nudge(5)
        XCTAssertEqual(player.position, 5)
        engine.position = 28
        await player.nudge(5)
        XCTAssertEqual(player.position, 30)
        await player.nudge(-5)
        XCTAssertEqual(player.position, 25)
        await player.seek(to: .nan)
        XCTAssertEqual(engine.seeks.last, 25)
    }

    @MainActor
    func testMuteRestoreVolumeClampsAndPersistsAcrossRestart() async throws {
        let (app, engine, _, defaults) = try await fixture()
        let player = app.player
        player.setVolume(0.37)
        player.toggleMute()
        XCTAssertEqual(engine.volume, 0)
        let restarted = PlayerModel(engine: FakePlaybackEngine(), defaults: defaults)
        XCTAssertEqual(restarted.volume, 0)
        restarted.toggleMute()
        XCTAssertEqual(restarted.volume, 0.37, accuracy: 0.0001)
        player.toggleMute()
        XCTAssertEqual(engine.volume, 0.37, accuracy: 0.0001)
        player.setVolume(-2); XCTAssertEqual(player.volume, 0)
        player.setVolume(2); XCTAssertEqual(player.volume, 1)
        player.setVolume(.nan); XCTAssertEqual(player.volume, 1)
    }

    @MainActor
    func testTuningMatchesWindowsAndResetDoesNotSeekOrResume() async throws {
        let (app, engine, _, defaults) = try await fixture()
        let player = app.player
        XCTAssertEqual(PlayerModel.rateSteps, [0.5, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 2])
        XCTAssertEqual(PlayerModel.pitchSteps, Array(-6...6))
        for rate in PlayerModel.rateSteps { player.setRate(rate); XCTAssertEqual(engine.rate, rate) }
        for pitch in PlayerModel.pitchSteps { player.setPitch(pitch); XCTAssertEqual(engine.pitch, pitch) }
        player.setRate(1.25); player.setPitch(2)
        XCTAssertEqual(player.tuneBadge, "1.25× · +2")
        player.setRate(.nan); player.setPitch(7)
        XCTAssertEqual(player.rate, 1.25); XCTAssertEqual(player.pitch, 2)
        let before = engine.seeks
        player.resetTuning()
        XCTAssertEqual(engine.rate, 1); XCTAssertEqual(engine.pitch, 0)
        XCTAssertEqual(player.tuneBadge, ""); XCTAssertEqual(engine.seeks, before)
        XCTAssertFalse(player.isPlaying)
        player.setRate(2); player.setPitch(-6)
        let restarted = PlayerModel(engine: FakePlaybackEngine(), defaults: defaults)
        XCTAssertEqual(restarted.rate, 1); XCTAssertEqual(restarted.pitch, 0)
    }

    @MainActor
    func testRemainingTimeStateAndPlaybackSettings() async throws {
        let (app, engine, _, defaults) = try await fixture()
        XCTAssertTrue(app.player.showRemaining)
        app.player.toggleTimeDisplay(); XCTAssertFalse(app.player.showRemaining)
        app.player.toggleTimeDisplay(); XCTAssertTrue(app.player.showRemaining)
        app.player.spectrumEnabled = false
        XCTAssertFalse(engine.spectrumEnabled)
        app.stageCoverBlur = false; app.stageBlurRadius = 80
        app.karaokeLyrics = false
        let reopened = AppModel(defaults: defaults, playbackEngine: FakePlaybackEngine())
        XCTAssertFalse(reopened.player.spectrumEnabled)
        XCTAssertFalse(reopened.stageCoverBlur); XCTAssertEqual(reopened.stageBlurRadius, 80)
        XCTAssertFalse(reopened.karaokeLyrics)
    }

    @MainActor
    func testLoopAndAuditionUseSourceSecondsAtEveryRateAndPitch() async throws {
        let (app, engine, _, _) = try await fixture()
        let player = app.player
        for rate in PlayerModel.rateSteps {
            for pitch in [-6, 0, 2, 6] {
                player.setRate(rate); player.setPitch(pitch)
                await player.toggleLoop(1)
                XCTAssertEqual(player.loop?.startSec, 2)
                XCTAssertEqual(engine.position, 2)
                engine.emit(4.99)
                XCTAssertEqual(engine.position, 4.99)
                engine.emit(5.01)
                // 循环定位通过主 actor Task 执行。
                for _ in 0..<5 { await Task.yield() }
                XCTAssertEqual(engine.position, 2)
                XCTAssertTrue(player.isPlaying)
                await player.toggleLoop(nil)
                await player.audition(1)
                XCTAssertNil(player.loop); XCTAssertEqual(player.stopAt, 5)
                engine.emit(4.99); XCTAssertTrue(player.isPlaying)
                engine.emit(5.01); XCTAssertFalse(player.isPlaying)
                XCTAssertNil(player.stopAt)
                XCTAssertEqual(engine.rate, rate); XCTAssertEqual(engine.pitch, pitch)
            }
        }
    }

    @MainActor
    func testHistoryAndCompletionAndPlaybackError() async throws {
        let (app, engine, corpus, _) = try await fixture()
        app.player.setRate(2); app.player.setPitch(2)
        app.player.play()
        for i in 0...60 { engine.emit(Double(i) / 10) }
        app.player.pause()
        for _ in 0..<5 { await Task.yield() }
        let history = await corpus.recordedPlays
        XCTAssertEqual(history.count, 1)
        XCTAssertEqual(history.first?.listenedSec ?? 0, 3, accuracy: 0.001)
        XCTAssertEqual(history.first?.positionSec, 6)
        XCTAssertEqual(history.first?.source, "library")
        await app.player.seek(to: 24)
        app.player.play()
        for i in 0...60 { engine.emit(24 + Double(i) / 10, ended: i == 60) }
        XCTAssertFalse(app.player.isPlaying)
        for _ in 0..<5 { await Task.yield() }
        let completed = await corpus.recordedPlays
        XCTAssertEqual(completed.count, 2)
        XCTAssertEqual(completed.last?.completed, true)
        engine.failPlay = true
        app.player.play()
        XCTAssertFalse(app.player.isPlaying)
        XCTAssertTrue(app.player.lastError?.contains("播放失败") == true)
    }

    /// 播放历史写不进去：要有提示（只提示一次），之后写成功就清掉
    @MainActor
    func testHistoryWriteFailureIsReportedOnceAndClearsOnSuccess() async throws {
        let (app, engine, corpus, _) = try await fixture()
        await corpus.failRecordPlay(true)
        app.player.play()
        for i in 0...30 { engine.emit(Double(i) / 10) }
        app.player.pause()
        await app.player.finishHistory()
        XCTAssertNotNil(app.player.historyError)
        XCTAssertEqual(app.notice?.contains("播放记录没写进去"), true)
        app.notice = nil
        app.player.play()
        for i in 31...60 { engine.emit(Double(i) / 10) }
        app.player.pause()
        await app.player.finishHistory()
        XCTAssertNil(app.notice, "连续失败不重复提示")
        await corpus.failRecordPlay(false)
        app.player.play()
        for i in 61...90 { engine.emit(Double(i) / 10) }
        await app.player.finishHistory()
        XCTAssertNil(app.player.historyError)
        let plays = await corpus.recordedPlays
        XCTAssertEqual(plays.count, 1, "退出前的收集把正在听的这一段也记下")
    }

    @MainActor
    func testRealUnitsAndEstimatedLoopTailSurviveTuning() async throws {
        let (app, engine, corpus, _) = try await fixture()
        await corpus.respondToLyrics("lyricTimeline", json: #"[{"utteranceId":2,"mode":"karaoke","units":[{"charStart":0,"charEnd":1,"startSec":5.3,"endSec":9,"source":"manual"}]}]"#)
        await app.player.refreshLyrics(songId: "001")
        let line = try XCTUnwrap(app.player.lines.last)
        let unit = try XCTUnwrap(app.player.timingUnits(for: line)?.first)
        app.player.setRate(1.25); app.player.setPitch(2)
        await app.player.toggleLoop(2)
        XCTAssertEqual(app.player.loop?.endSec, 9, "估算行尾不能截断真实单位")
        await app.player.playUnit(unit, in: line)
        XCTAssertEqual(engine.position, 5.3, accuracy: 0.000001)
        engine.position = 6.1
        XCTAssertEqual(app.player.timingPosition, 6.1, accuracy: 0.000001, "打轴取实时源时钟，不等界面下一拍")
        XCTAssertEqual(app.player.position, 5.3, accuracy: 0.000001)
        XCTAssertEqual(app.player.displayMode(for: line), .karaoke)
        engine.emit(8.5)
        XCTAssertEqual(engine.position, 8.5)
        engine.emit(9.01)
        for _ in 0..<5 { await Task.yield() }
        XCTAssertEqual(engine.position, 5)
        XCTAssertEqual(engine.rate, 1.25); XCTAssertEqual(engine.pitch, 2)
    }

    @MainActor
    func testLoopAtTrackEndResumesButUserPauseWinsOverPendingLoop() async throws {
        let (app, engine, corpus, _) = try await fixture()
        await corpus.respondToLyrics("lineSpans", json: #"[{"utteranceId":1,"lineIdx":0,"startSec":2,"endSec":30,"endKind":"trackEnd"}]"#)
        await app.player.refreshLyrics(songId: "001")
        await app.player.toggleLoop(1)
        app.player.setPitch(2); app.player.setRate(1.25)
        engine.emit(30, ended: true)
        for _ in 0..<5 { await Task.yield() }
        XCTAssertEqual(engine.position, 2)
        XCTAssertTrue(engine.isPlaying)
        engine.emit(30, ended: true)
        app.player.pause()
        for _ in 0..<5 { await Task.yield() }
        XCTAssertFalse(engine.isPlaying)
        XCTAssertFalse(app.player.isPlaying)
        XCTAssertEqual(engine.position, 30)
    }
}

/// 记下报给系统「正在播放」的每一次
@MainActor
private final class MediaRecorder: SystemMediaPublishing {
    var states: [NowPlayingState] = []
    func publish(_ state: NowPlayingState) { states.append(state) }
}

private final class Flag: @unchecked Sendable { var value = false }

extension PlayerModelTests {
    /// 当前行是存起来的：只在跨过行首时变，跳转后立即跟上
    @MainActor
    func testCurrentLineIsStoredAndOnlyChangesAtLineBoundaries() async throws {
        let (app, engine, _, _) = try await fixture()
        let player = app.player
        XCTAssertNil(player.currentUtterance); XCTAssertNil(player.currentIndex)
        player.play()
        engine.emit(2); XCTAssertEqual(player.currentUtterance, 1); XCTAssertEqual(player.currentIndex, 0)
        engine.emit(4.9); XCTAssertEqual(player.currentUtterance, 1)
        engine.emit(5); XCTAssertEqual(player.currentUtterance, 2); XCTAssertEqual(player.currentIndex, 1)
        await player.seek(to: 0.5)
        XCTAssertNil(player.currentUtterance)
        XCTAssertEqual(player.coarsePosition, 0.5)
        engine.emit(5.37)
        XCTAssertEqual(player.coarsePosition, 5.25, "粗位置取整到 0.25 秒")
        XCTAssertEqual(player.position, 5.37)
    }

    /// 暂停时时钟照走，但位置没变就不能通知界面（以前每 50ms 重算一次整个歌词面板）
    @MainActor
    func testUnchangedClockDoesNotInvalidateObservers() async throws {
        let (app, engine, _, _) = try await fixture()
        engine.emit(3)
        let fired = Flag()
        withObservationTracking {
            _ = app.player.position
            _ = app.player.coarsePosition
            _ = app.player.currentUtterance
        } onChange: { fired.value = true }
        engine.emit(3); engine.emit(3)
        XCTAssertFalse(fired.value, "位置没变：不通知")
        engine.emit(3.1)
        XCTAssertTrue(fired.value)

        let coarse = Flag()
        withObservationTracking { _ = app.player.coarsePosition; _ = app.player.currentUtterance } onChange: { coarse.value = true }
        engine.emit(3.15); engine.emit(3.2)
        XCTAssertFalse(coarse.value, "同一个 0.25 秒内、同一行里：只看粗位置和当前行的视图不重算")
        engine.emit(3.3)
        XCTAssertTrue(coarse.value)
    }

    /// 媒体键：F8 播放 / 暂停，控制中心快进快退 5 秒；系统「正在播放」只在状态变化时报
    @MainActor
    func testSystemMediaCommandsAndNowPlayingOnlyOnStateChanges() async throws {
        let (app, engine, _, _) = try await fixture()
        let player = app.player
        let media = MediaRecorder()
        player.systemMedia = media
        XCTAssertEqual(media.states.last?.track?.id, "001")
        XCTAssertEqual(media.states.last?.duration, 30)

        let toggled = await player.perform(.toggle)
        XCTAssertTrue(toggled); XCTAssertTrue(engine.isPlaying)
        XCTAssertEqual(media.states.last?.isPlaying, true)
        let published = media.states.count
        for i in 0..<20 { engine.emit(Double(i) / 10) }
        XCTAssertEqual(media.states.count, published, "时钟走动不报：系统按播放速度自己推进度")

        await player.perform(.seek(4))
        XCTAssertEqual(engine.position, 4); XCTAssertEqual(media.states.last?.position, 4)
        await player.perform(.skip(5))
        XCTAssertEqual(engine.position, 9, accuracy: 0.000001)
        await player.perform(.skip(-10))
        XCTAssertEqual(engine.position, 0)
        await player.perform(.pause)
        XCTAssertFalse(engine.isPlaying); XCTAssertEqual(media.states.last?.isPlaying, false)
        player.setRate(1.5)
        XCTAssertEqual(media.states.last?.rate, 1.5)

        await player.prepareDeletion("001")
        let handled = await player.perform(.play)
        XCTAssertFalse(handled, "没有能放的歌：交给系统转给别的应用")
        XCTAssertNil(media.states.last?.track)
    }
}

// MARK: - 播放队列

extension PlayerModelTests {
    private func song(_ id: String, audio: String? = nil) throws -> Track {
        let path = audio ?? "\(id).flac"
        return try decoded(Track.self, #"{"id":"\#(id)","title":"曲\#(id)","artist":"歌手","album":"盘","year":"","genre":"","audioPath":"\#(path)","coverPath":"","durationSec":30,"lineCount":2}"#)
    }

    /// 等异步的切歌做完（自然放完后接下一首是另起的 Task）
    @MainActor
    private func waitUntil(_ what: String, _ condition: @MainActor () -> Bool) async throws {
        for _ in 0..<400 {
            if condition() { return }
            try await Task.sleep(nanoseconds: 5_000_000)
        }
        XCTFail("等不到：\(what)")
    }

    /// 从专辑放：一首放完自动接下一首，放到最后一首就停
    @MainActor
    func testQueueAdvancesOnNaturalEndAndStopsAfterLast() async throws {
        let (app, engine, _, _) = try await fixture()
        let player = app.player
        let album = try ["101", "102", "103"].map { try song($0) }
        await app.play(songId: "101", queue: album)
        XCTAssertEqual(player.queue.map(\.id), ["101", "102", "103"])
        XCTAssertEqual(player.queueIndex, 0)
        XCTAssertTrue(player.isPlaying); XCTAssertTrue(player.hasNext); XCTAssertFalse(player.hasPrevious)

        engine.emit(30, ended: true)
        try await waitUntil("放完接第二首") { player.track?.id == "102" && player.isPlaying }
        XCTAssertEqual(player.queueIndex, 1); XCTAssertEqual(engine.position, 0)
        XCTAssertTrue(player.hasPrevious)

        engine.emit(30, ended: true)
        try await waitUntil("放完接第三首") { player.track?.id == "103" && player.isPlaying }
        XCTAssertFalse(player.hasNext)

        engine.emit(30, ended: true)
        for _ in 0..<20 { await Task.yield() }
        XCTAssertEqual(player.track?.id, "103", "最后一首放完就停，不绕回去")
        XCTAssertFalse(player.isPlaying)
        let moved = await player.next()
        XCTAssertFalse(moved)
    }

    /// 循环一行到行尾：跳回行首，不算「放完了」，不能切歌
    @MainActor
    func testLoopDoesNotAdvanceQueue() async throws {
        let (app, engine, _, _) = try await fixture()
        let player = app.player
        let album = try ["001", "102"].map { try song($0) }
        await app.play(songId: "001", queue: album)
        await player.toggleLoop(1)
        engine.emit(5.01)
        for _ in 0..<20 { await Task.yield() }
        XCTAssertEqual(player.track?.id, "001")
        XCTAssertEqual(engine.position, 2, accuracy: 0.000001, "循环回到行首")
    }

    /// 自然放完时跳过找不到音频的歌；手动「下一首」停在那首上并提示，再按一次能离开
    @MainActor
    func testUnplayableSongsAreSkippedOnlyWhenAdvancingAutomatically() async throws {
        let (app, engine, corpus, _) = try await fixture()
        let player = app.player
        await corpus.respondToLyrics("resolveMedia:missing.flac", json: "null")
        let album = [try song("101"), try song("102", audio: "missing.flac"), try song("103")]

        await app.play(songId: "101", queue: album)
        engine.emit(30, ended: true)
        try await waitUntil("跳过放不了的第二首") { player.track?.id == "103" && player.isPlaying }
        XCTAssertEqual(player.queueIndex, 2)

        await app.play(songId: "101", queue: album)
        let moved = await player.perform(.nextTrack)
        XCTAssertTrue(moved)
        XCTAssertEqual(player.track?.id, "102"); XCTAssertFalse(player.canPlay)
        XCTAssertTrue(player.lastError?.contains("找不到") == true)
        let left = await player.perform(.nextTrack)
        XCTAssertTrue(left, "这首放不了也能用「下一首」离开")
        XCTAssertEqual(player.track?.id, "103"); XCTAssertTrue(player.isPlaying)
    }

    /// 上一首：放过 3 秒回到这首开头；开头附近才回到前一首；第一首只回开头
    @MainActor
    func testPreviousRestartsAfterThresholdThenGoesBack() async throws {
        let (app, engine, _, _) = try await fixture()
        let player = app.player
        let album = try ["101", "102"].map { try song($0) }
        await app.play(songId: "102", queue: album)
        engine.emit(10)
        await player.perform(.previousTrack)
        XCTAssertEqual(player.track?.id, "102"); XCTAssertEqual(engine.position, 0)
        XCTAssertTrue(player.isPlaying)

        await player.perform(.previousTrack)
        XCTAssertEqual(player.track?.id, "101"); XCTAssertEqual(player.queueIndex, 0)
        engine.emit(1)
        await player.previous()
        XCTAssertEqual(player.track?.id, "101", "第一首：只回开头"); XCTAssertEqual(engine.position, 0)
    }

    /// 队列去重；放队列里的歌只挪位置，放队列外的歌队列就只剩它
    @MainActor
    func testOpeningInsideQueueMovesAndOutsideReplaces() async throws {
        let (app, _, _, _) = try await fixture()
        let player = app.player
        XCTAssertEqual(player.queue.map(\.id), ["001"], "单独放一首：队列只有它")
        let a = try song("101"), b = try song("102"), c = try song("103")
        await app.play(songId: "102", queue: [a, b, a, c])
        XCTAssertEqual(player.queue.map(\.id), ["101", "102", "103"]); XCTAssertEqual(player.queueIndex, 1)
        await player.open(c)
        XCTAssertEqual(player.queue.count, 3); XCTAssertEqual(player.queueIndex, 2)
        await player.open(try song("109"))
        XCTAssertEqual(player.queue.map(\.id), ["109"]); XCTAssertFalse(player.hasNext)
        await player.open(a, queue: [b, c])
        XCTAssertEqual(player.queue.map(\.id), ["101"], "给的队列里没有这首：不拿别的歌冒充")
    }

    /// 删掉队列里前面的歌，当前位置跟着挪；系统「正在播放」的「下一首」随队列变灰
    @MainActor
    func testDeletionShiftsQueueAndNowPlayingReportsNext() async throws {
        let (app, _, _, _) = try await fixture()
        let player = app.player
        let media = MediaRecorder()
        player.systemMedia = media
        XCTAssertEqual(media.states.last?.hasNext, false)
        let album = try ["101", "102", "103"].map { try song($0) }
        await app.play(songId: "102", queue: album)
        XCTAssertEqual(media.states.last?.hasNext, true)
        await player.prepareDeletion("101")
        XCTAssertEqual(player.queue.map(\.id), ["102", "103"]); XCTAssertEqual(player.queueIndex, 0)
        XCTAssertEqual(player.track?.id, "102")
        await player.prepareDeletion("103")
        XCTAssertFalse(player.hasNext)
        XCTAssertEqual(media.states.last?.hasNext, false)
        let moved = await player.perform(.nextTrack)
        XCTAssertFalse(moved)
    }
}
