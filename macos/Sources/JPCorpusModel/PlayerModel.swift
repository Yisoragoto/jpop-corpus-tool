import AVFoundation
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
    public var rate: Float = 1 {
        didSet { if isPlaying { player.rate = rate } }
    }
    public var lastError: String?

    weak var app: AppModel?

    private let player = AVPlayer()
    private var timeObserver: Any?
    /// 这首听了多久（播放历史用，和 Windows 客户端写同一张表）
    private var listened: Double = 0
    private var lastTick: Double?
    private var source = "library"
    private var loadSequence: UInt64 = 0
    private var seekSequence: UInt64 = 0
    private var loadingLyrics = false
    private var loadedClientID: ObjectIdentifier?

    public init() {
        player.automaticallyWaitsToMinimizeStalling = false
        timeObserver = player.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 0.05, preferredTimescale: 600),
            queue: .main
        ) { [weak self] time in
            MainActor.assumeIsolated { self?.tick(time.seconds) }
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

    /// 打开一首歌。`at` 有值就跳过去，`autoplay` 决定放不放
    public func open(_ track: Track, at: Double? = nil, autoplay: Bool = true, focus: Int64? = nil, source: String = "library") async {
        guard let client = app?.client else { return }
        loadSequence &+= 1
        let request = loadSequence
        defer { if request == loadSequence { loadingLyrics = false } }
        if self.track?.id != track.id || loadingLyrics || loadedClientID != ObjectIdentifier(client) {
            flushHistory()
            loadingLyrics = true
            pause()
            loop = nil
            stopAt = nil
            position = 0
            lastTick = nil
            lines = []
            spans = [:]
            timelines = [:]
            player.replaceCurrentItem(with: nil)
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
                player.replaceCurrentItem(with: nil)
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
            let item = AVPlayerItem(url: URL(fileURLWithPath: path))
            player.replaceCurrentItem(with: item)
            duration = track.durationSec ?? 0
            if duration == 0, let d = try? await item.asset.load(.duration) {
                guard request == loadSequence, app?.client === client else { return }
                duration = d.seconds.isFinite ? d.seconds : 0
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
        guard player.currentItem != nil else { return }
        player.playImmediately(atRate: rate)
        isPlaying = true
    }

    public func pause() {
        player.pause()
        isPlaying = false
    }

    public func toggle() {
        isPlaying ? pause() : play()
    }

    public func seek(to seconds: Double) async {
        guard seconds.isFinite, let item = player.currentItem else { return }
        seekSequence &+= 1
        let request = seekSequence
        let load = loadSequence
        let target = CMTime(seconds: max(0, seconds), preferredTimescale: 1_000_000)
        await player.seek(to: target, toleranceBefore: .zero, toleranceAfter: .zero)
        guard request == seekSequence, load == loadSequence, player.currentItem === item else { return }
        position = max(0, seconds)
        lastTick = nil
    }

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
        if isPlaying {
            if let last = lastTick, seconds > last, seconds - last < 1 { listened += seconds - last }
            lastTick = seconds
        }
        if let loop, seconds >= loop.endSec {
            Task { await seek(to: loop.startSec) }
            return
        }
        if let stop = stopAt, seconds >= stop {
            stopAt = nil
            pause()
        }
        // 放完了
        if isPlaying, player.rate == 0, duration > 0, seconds >= duration - 0.3 {
            isPlaying = false
            flushHistory(completed: true)
        }
    }

    /// 把这一首的收听记进播放历史。听不到 2 秒的不记
    private func flushHistory(completed: Bool = false) {
        guard let track, listened >= 2, let client = app?.client else { return }
        let event = PlayEvent(songId: track.id, listenedSec: listened, positionSec: position, completed: completed, source: source)
        listened = 0
        Task { try? await client.recordPlay(event) }
    }
}
