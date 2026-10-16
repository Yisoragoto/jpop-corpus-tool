import AVFoundation
import Foundation
import JPCore
import Observation

/// 播放器。音频走 AVFoundation（原生解码 FLAC / MP3 / AAC，系统的音量与输出设备），
/// **定位规则走 Rust**：每行的起止（`lineSpans`）、试听窗口（`listenWindow`）都由
/// jp-corpus 的 segments.rs 算，和 Windows 客户端完全同一套——这里不自己推「下一行在哪」。
///
/// 精度只到行：时间戳来自 LRC，没有词级时间。
@MainActor
@Observable
final class PlayerModel {
    private(set) var track: Track?
    private(set) var lines: [LyricLine] = []
    private(set) var spans: [Int64: LineSpan] = [:]
    private(set) var position: Double = 0
    private(set) var duration: Double = 0
    private(set) var isPlaying = false
    /// 正在循环的那一行
    private(set) var loop: LineSpan?
    /// 试听一句：播到这里自动停
    private(set) var stopAt: Double?
    /// 歌词面板要滚到的那一行（检索结果点进来时）
    var focusUtterance: Int64?
    var rate: Float = 1 {
        didSet { if isPlaying { player.rate = rate } }
    }
    var lastError: String?

    weak var app: AppModel?

    private let player = AVPlayer()
    private var timeObserver: Any?
    /// 这首听了多久（播放历史用，和 Windows 客户端写同一张表）
    private var listened: Double = 0
    private var lastTick: Double?
    private var source = "library"

    init() {
        player.automaticallyWaitsToMinimizeStalling = false
        timeObserver = player.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 0.05, preferredTimescale: 600),
            queue: .main
        ) { [weak self] time in
            MainActor.assumeIsolated { self?.tick(time.seconds) }
        }
    }

    /// 当前唱到哪一行：最后一个时间不晚于当前位置的行
    var currentUtterance: Int64? {
        guard track != nil else { return nil }
        var current: Int64?
        for line in lines {
            guard let t = line.timeSec else { continue }
            if t <= position + 0.05 { current = line.utteranceId } else { break }
        }
        return current
    }

    // MARK: - 加载

    /// 打开一首歌。`at` 有值就跳过去，`autoplay` 决定放不放
    func open(_ track: Track, at: Double? = nil, autoplay: Bool = true, focus: Int64? = nil, source: String = "library") async {
        guard let client = app?.client else { return }
        if self.track?.id != track.id {
            flushHistory()
            do {
                async let l = client.lyrics(track.id)
                async let s = client.lineSpans(track.id)
                let (fetchedLines, fetchedSpans) = try await (l, s)
                lines = fetchedLines
                spans = Dictionary(uniqueKeysWithValues: fetchedSpans.map { ($0.utteranceId, $0) })
            } catch {
                lastError = error.localizedDescription
                lines = []
                spans = [:]
            }
            guard let path = try? await client.resolveMedia(track.audioPath) else {
                self.track = track
                player.replaceCurrentItem(with: nil)
                isPlaying = false
                lastError = "找不到《\(track.title)》的音频：\(track.audioPath)。可以在设置里添加音频所在的文件夹。"
                focusUtterance = focus
                return
            }
            self.track = track
            loop = nil
            stopAt = nil
            listened = 0
            let item = AVPlayerItem(url: URL(fileURLWithPath: path))
            player.replaceCurrentItem(with: item)
            duration = track.durationSec ?? 0
            if duration == 0, let d = try? await item.asset.load(.duration) {
                duration = d.seconds.isFinite ? d.seconds : 0
            }
        }
        self.source = source
        focusUtterance = focus
        if let at { await seek(to: at) }
        if autoplay { play() }
    }

    /// 检索结果的「试听」：加载那首歌、从窗口起点放、到窗口终点自动停
    func audition(_ utteranceId: Int64, before: Int = 0, after: Int = 0) async {
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
    func playFrom(songId: String, utteranceId: Int64, timeSec: Double?) async {
        guard let track = try? await app?.track(songId) else { return }
        let start = timeSec.map { max(0, $0 - 0.4) }
        await open(track, at: start, autoplay: timeSec != nil, focus: utteranceId, source: "search")
        stopAt = nil
    }

    // MARK: - 控制

    func play() {
        guard player.currentItem != nil else { return }
        player.playImmediately(atRate: rate)
        isPlaying = true
    }

    func pause() {
        player.pause()
        isPlaying = false
    }

    func toggle() {
        isPlaying ? pause() : play()
    }

    func seek(to seconds: Double) async {
        let target = CMTime(seconds: max(0, seconds), preferredTimescale: 600)
        await player.seek(to: target, toleranceBefore: .zero, toleranceAfter: .zero)
        position = max(0, seconds)
        lastTick = nil
    }

    /// 点歌词某一行：跳过去接着放
    func playLine(_ line: LyricLine) async {
        guard let t = line.timeSec else { return }
        stopAt = nil
        await seek(to: max(0, t - 0.2))
        play()
    }

    /// 循环这一行（再点一次取消）。区间用 Rust 算好的行起止：间奏前的那一句不会把整段伴奏循环进去
    func toggleLoop(_ utteranceId: Int64?) async {
        if loop != nil, utteranceId == nil || loop?.utteranceId == utteranceId {
            loop = nil
            return
        }
        guard let id = utteranceId ?? currentUtterance, let span = spans[id] else { return }
        loop = span
        stopAt = nil
        await seek(to: span.startSec)
        play()
    }

    /// 上一行 / 下一行
    func step(_ delta: Int) async {
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
