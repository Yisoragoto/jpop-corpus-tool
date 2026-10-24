import Foundation
import JPCore
import Observation

/// 播放器。音频走 AVFoundation（原生解码 FLAC / MP3 / AAC，系统的音量与输出设备），
/// **定位规则走 Rust**：每行的起止（`lineSpans`）、试听窗口（`listenWindow`）都由
/// jp-corpus 的 segments.rs 算，和 Windows 客户端完全同一套——这里不自己推「下一行在哪」。
///
/// 精度来自 Rust 验证过的真实单位；没有完整词级时间时只跟随行，不估算。
@MainActor
@Observable
public final class PlayerModel {
    public private(set) var track: Track?
    public private(set) var lines: [LyricLine] = []
    public private(set) var timelines: [Int64: LineTimeline] = [:]
    public private(set) var spans: [Int64: LineSpan] = [:]
    public private(set) var position: Double = 0
    public private(set) var duration: Double = 0
    public private(set) var isPlaying = false
    /// 正在循环的那一行
    public private(set) var loop: LineSpan?
    /// 试听一句：播到这里自动停
    public private(set) var stopAt: Double?
    /// 歌词面板要滚到的那一行（检索结果点进来时）
    public var focusUtterance: Int64?
    public static let rateSteps: [Float] = [0.5, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 2]
    public static let pitchSteps = Array(-6...6)
    public private(set) var rate: Float = 1
    public private(set) var pitch: Int = 0
    public private(set) var volume: Float
    public var showRemaining = true
    public var spectrumEnabled: Bool {
        didSet {
            defaults.set(spectrumEnabled, forKey: "player-spectrum")
            engine.spectrumEnabled = spectrumEnabled
        }
    }
    public var spectrum: [Float] { engine.spectrum }
    public private(set) var canPlay = false
    public var tuneBadge: String {
        [rate == 1 ? nil : "\(String(format: "%g", rate))×",
         pitch == 0 ? nil : String(format: "%+d", pitch)].compactMap { $0 }.joined(separator: " · ")
    }
    public var lastError: String?

    weak var app: AppModel?

    @ObservationIgnored private let engine: any PlaybackEngine
    @ObservationIgnored private let defaults: UserDefaults
    private var lastAudibleVolume: Float
    /// 这首听了多久（播放历史用，和 Windows 客户端写同一张表）
    private var listened: Double = 0
    private var lastTick: Double?
    private var source = "library"
    private var loadSequence: UInt64 = 0
    private var seekSequence: UInt64 = 0
    private var loadingLyrics = false
    private var loadedClientID: ObjectIdentifier?

    public init(engine: (any PlaybackEngine)? = nil, defaults: UserDefaults = .standard) {
        let audio = engine ?? NativePlaybackEngine()
        self.engine = audio
        self.defaults = defaults
        let saved = defaults.object(forKey: "player-volume") as? NSNumber
        let value = saved?.floatValue ?? 1
        let initialVolume: Float = value.isFinite ? min(1, max(0, value)) : 1
        volume = initialVolume
        let audible = defaults.float(forKey: "player-audible-volume")
        lastAudibleVolume = initialVolume > 0 ? initialVolume : (audible.isFinite && audible > 0 ? min(1, audible) : 1)
        spectrumEnabled = defaults.object(forKey: "player-spectrum") as? Bool ?? true
        audio.volume = volume
        audio.spectrumEnabled = spectrumEnabled
        audio.onTick = { [weak self] seconds in self?.tick(seconds) }
        audio.onStateChange = { [weak self] in
            guard let self else { return }
            self.position = self.engine.position
            self.isPlaying = self.engine.isPlaying
            self.lastTick = nil
            if let error = self.engine.lastError { self.lastError = error }
            if !self.isPlaying { self.flushHistory() }
        }
    }

    /// 当前唱到哪一行：最后一个时间不晚于当前位置的行
    public var currentUtterance: Int64? {
        guard track != nil else { return nil }
        var current: Int64?
        var latest = -Double.infinity
        for line in lines {
            let start = line.timeSec.flatMap { $0.isFinite && $0 >= 0 ? $0 : nil }
                ?? timingUnits(for: line)?.first?.startSec
            guard let t = start, t <= position, t >= latest else { continue }
            latest = t
            current = line.utteranceId
        }
        return current
    }

    public func timingUnits(for line: LyricLine) -> [LyricTimingUnit]? {
        // The estimated listen/loop window is not a bound on real source clocks.
        let bound = spans[line.utteranceId].flatMap { $0.endKind == .estimated ? nil : $0.endSec }
        return timelines[line.utteranceId]?.validatedUnits(for: line, endSec: bound)
    }

    public func displayMode(for line: LyricLine) -> LyricTimelineMode {
        if timingUnits(for: line) != nil { return .karaoke }
        if let t = line.timeSec, t.isFinite, t >= 0 { return .lineSynced }
        return .plain
    }

    // MARK: - 加载

    /// 打点时直接读取引擎时钟，避免使用界面刷新前的旧位置。
    public var timingPosition: Double {
        let seconds = engine.position
        return seconds.isFinite ? seconds : position
    }

    /// 换歌词后清理旧行 ID 与循环区间，保持音频播放位置。
    public func refreshLyrics(songId: String) async {
        guard track?.id == songId, let client = app?.client else { return }
        let request = loadSequence
        do {
            async let l = client.lyrics(songId)
            async let s = client.lineSpans(songId)
            async let t = client.lyricTimeline(songId)
            let (newLines, newSpans, newTimelines) = try await (l, s, t)
            guard request == loadSequence, track?.id == songId, app?.client === client else { return }
            lines = newLines
            spans = Dictionary(uniqueKeysWithValues: newSpans.map { ($0.utteranceId, $0) })
            timelines = Dictionary(newTimelines.map { ($0.utteranceId, $0) }, uniquingKeysWith: { first, _ in first })
            loop = nil; stopAt = nil; focusUtterance = nil
        } catch { lastError = error.localizedDescription }
    }

    /// 打开一首歌。`at` 有值就跳过去，`autoplay` 决定放不放
    public func open(_ track: Track, at: Double? = nil, autoplay: Bool = true, focus: Int64? = nil, source: String = "library") async {
        guard let client = app?.client else { return }
        loadSequence &+= 1
        let request = loadSequence
        defer { if request == loadSequence { loadingLyrics = false } }
        if self.track?.id != track.id || loadingLyrics || loadedClientID != ObjectIdentifier(client) || !engine.isLoaded {
            flushHistory()
            loadingLyrics = true
            lastError = nil
            pause()
            loop = nil
            stopAt = nil
            position = 0
            lastTick = nil
            lines = []
            spans = [:]
            timelines = [:]
            engine.unload()
            canPlay = false
            duration = 0
            do {
                async let l = client.lyrics(track.id)
                async let s = client.lineSpans(track.id)
                async let t = try? client.lyricTimeline(track.id)
                let (fetchedLines, fetchedSpans, fetchedTimelines) = try await (l, s, t)
                guard request == loadSequence, app?.client === client else { return }
                lines = fetchedLines
                spans = Dictionary(uniqueKeysWithValues: fetchedSpans.map { ($0.utteranceId, $0) })
                timelines = Dictionary((fetchedTimelines ?? []).map { ($0.utteranceId, $0) }, uniquingKeysWith: { first, _ in first })
            } catch {
                guard request == loadSequence, app?.client === client else { return }
                lastError = error.localizedDescription
                lines = []
                spans = [:]
                timelines = [:]
            }
            let path = try? await client.resolveMedia(track.audioPath)
            guard request == loadSequence, app?.client === client else { return }
            guard let path else {
                self.track = track
                engine.unload()
                canPlay = false
                isPlaying = false
                lastError = "找不到《\(track.title)》的音频：\(track.audioPath)。可以在设置里添加音频所在的文件夹。"
                focusUtterance = focus
                return
            }
            self.track = track
            loadedClientID = ObjectIdentifier(client)
            loop = nil
            stopAt = nil
            listened = 0
            do {
                try engine.load(URL(fileURLWithPath: path))
                duration = engine.duration
                canPlay = true
            } catch {
                engine.unload()
                canPlay = false
                loadedClientID = nil
                lastError = "无法播放《\(track.title)》：\(error.localizedDescription)"
                return
            }
        }
        self.source = source
        focusUtterance = focus
        if let at { await seek(to: at) }
        guard request == loadSequence, app?.client === client else { return }
        if autoplay { play() }
    }

    /// 检索结果的「试听」：加载那首歌、从窗口起点放、到窗口终点自动停
    public func audition(_ utteranceId: Int64, before: Int = 0, after: Int = 0) async {
        guard let client = app?.client else { return }
        do {
            guard let window = try await client.listenWindow(utteranceId, before: before, after: after) else {
                lastError = "这一行没有时间戳，没法定位到音频"
                return
            }
            guard let track = try await app?.track(window.songId) else { return }
            await open(track, at: window.startSec, autoplay: false, focus: utteranceId, source: "search")
            loop = nil
            stopAt = window.endSec
            play()
        } catch {
            lastError = error.localizedDescription
        }
    }

    /// 从某一行接着往下放（不停）。KWIC 里双击 / 回车就是这个
    public func playFrom(songId: String, utteranceId: Int64, timeSec: Double?) async {
        guard let track = try? await app?.track(songId) else { return }
        let start = timeSec.map { max(0, $0 - 0.4) }
        await open(track, at: start, autoplay: timeSec != nil, focus: utteranceId, source: "search")
        stopAt = nil
    }

    // MARK: - 控制

    public func play() {
        guard engine.isLoaded else { return }
        do {
            try engine.play()
            isPlaying = engine.isPlaying
            lastError = nil
            lastTick = nil
        } catch {
            isPlaying = false
            lastError = "播放失败：\(error.localizedDescription)"
        }
    }

    public func pause() {
        engine.pause()
        position = engine.position
        isPlaying = false
        lastTick = nil
        // 暂停也记一笔：首页「继续听」要停在这里
        flushHistory()
    }

    public func toggle() {
        isPlaying ? pause() : play()
    }

    public func seek(to seconds: Double) async {
        guard seconds.isFinite, engine.isLoaded else { return }
        seekSequence &+= 1
        let request = seekSequence
        let load = loadSequence
        engine.seek(to: min(duration, max(0, seconds)))
        guard request == seekSequence, load == loadSequence else { return }
        position = engine.position
        lastTick = nil
    }

    public func nudge(_ seconds: Double) async { await seek(to: timingPosition + seconds) }

    public func setVolume(_ value: Float) {
        guard value.isFinite else { return }
        volume = min(1, max(0, value))
        engine.volume = volume
        defaults.set(volume, forKey: "player-volume")
        if volume > 0 {
            lastAudibleVolume = volume
            defaults.set(volume, forKey: "player-audible-volume")
        }
    }

    public func toggleMute() { setVolume(volume > 0 ? 0 : lastAudibleVolume) }
    public func toggleTimeDisplay() { showRemaining.toggle() }

    public func setRate(_ value: Float) {
        guard Self.rateSteps.contains(value) else { return }
        rate = value
        engine.rate = value
        lastTick = nil
    }

    public func setPitch(_ value: Int) {
        guard Self.pitchSteps.contains(value) else { return }
        pitch = value
        engine.pitch = value
    }

    public func resetTuning() { setRate(1); setPitch(0) }

    /// 点歌词某一行：跳过去接着放
    public func playLine(_ line: LyricLine) async {
        guard let t = line.timeSec, t.isFinite, t >= 0 else { return }
        stopAt = nil
        await seek(to: max(0, t - 0.2))
        play()
    }

    /// Native unit buttons seek to the original source onset, with no character
    /// interpolation or preroll. A unit from stale/invalid lyrics cannot seek.
    public func playUnit(_ unit: LyricTimingUnit, in line: LyricLine) async {
        guard timingUnits(for: line)?.contains(unit) == true else { return }
        let load = loadSequence
        if loop?.utteranceId != line.utteranceId { loop = nil }
        stopAt = nil
        focusUtterance = line.utteranceId
        await seek(to: unit.startSec)
        guard load == loadSequence else { return }
        play()
    }

    /// 循环这一行（再点一次取消）。区间用 Rust 算好的行起止：间奏前的那一句不会把整段伴奏循环进去
    public func toggleLoop(_ utteranceId: Int64?) async {
        if loop != nil, utteranceId == nil || loop?.utteranceId == utteranceId {
            loop = nil
            return
        }
        guard let id = utteranceId ?? currentUtterance, var span = spans[id] else { return }
        if let line = lines.first(where: { $0.utteranceId == id }),
           let units = timingUnits(for: line), let end = units.last?.endSec {
            // An estimated listen window must not cut off a real timed word.
            span.endSec = end
        }
        loop = span
        stopAt = nil
        await seek(to: span.startSec)
        play()
    }

    /// 上一行 / 下一行
    public func step(_ delta: Int) async {
        let timed = lines.filter { $0.timeSec != nil }
        guard !timed.isEmpty else { return }
        let index = timed.firstIndex { $0.utteranceId == currentUtterance } ?? -1
        let next = min(max(index + delta, 0), timed.count - 1)
        await playLine(timed[next])
    }

    // MARK: - 时钟

    private func tick(_ seconds: Double) {
        guard seconds.isFinite else { return }
        position = seconds
        // A stopped engine must not leave the UI playing, even if it did not send a state callback.
        // Natural EOF still goes through the loop/completion paths below.
        if isPlaying, !engine.isPlaying, engine.lastError != nil || seconds < duration - 0.05 {
            isPlaying = false
            lastTick = nil
            lastError = engine.lastError ?? "音频引擎意外停止，请重新播放"
            flushHistory()
            return
        }
        if isPlaying {
            if let last = lastTick, seconds > last, seconds - last < 1 { listened += (seconds - last) / Double(rate) }
            lastTick = seconds
        }
        if isPlaying, let loop, seconds >= loop.endSec {
            let request = loadSequence
            Task {
                guard isPlaying, request == loadSequence, self.loop?.utteranceId == loop.utteranceId else { return }
                await seek(to: loop.startSec)
                play()
            }
            return
        }
        if isPlaying, let stop = stopAt, seconds >= stop {
            stopAt = nil
            pause()
        }
        // 放完了
        if isPlaying, !engine.isPlaying, duration > 0, seconds >= duration - 0.05 {
            isPlaying = false
            flushHistory(completed: true)
        }
    }

    /// 把这一首的收听记进播放历史。听不到 2 秒的不记
    @ObservationIgnored private var historyWrite: Task<Void, Never>?

    public func prepareDeletion(_ songId: String) async {
        if track?.id == songId {
            pause(); engine.unload(); track = nil; lines = []; isPlaying = false; loadSequence += 1
            canPlay = false; position = 0; duration = 0; loop = nil; stopAt = nil; focusUtterance = nil
            spans = [:]; timelines = [:]; loadedClientID = nil
        }
        await historyWrite?.value
        historyWrite = nil
    }

    public func refreshMetadata(_ tracks: [Track]) {
        if let id = track?.id, let updated = tracks.first(where: { $0.id == id }) { track = updated }
    }

    private func flushHistory(completed: Bool = false) {
        guard let track, listened >= 2, let client = app?.client else { return }
        let event = PlayEvent(songId: track.id, listenedSec: listened, positionSec: position, completed: completed, source: source)
        listened = 0
        let previous = historyWrite
        historyWrite = Task<Void, Never> {
            await previous?.value
            try? await client.recordPlay(event)
            app?.homeVersion += 1
        }
    }
}
