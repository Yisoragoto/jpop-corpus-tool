import AVFoundation
import Foundation

/// 所有位置均为源音频秒数，调音不改变歌词时间轴。
@MainActor
public protocol PlaybackEngine: AnyObject {
    var isLoaded: Bool { get }
    var isPlaying: Bool { get }
    var position: Double { get }
    var duration: Double { get }
    var rate: Float { get set }
    var pitch: Int { get set }
    var volume: Float { get set }
    var spectrumEnabled: Bool { get set }
    var spectrum: [Float] { get }
    var onTick: ((Double) -> Void)? { get set }
    func load(_ url: URL) throws
    func unload()
    func play() throws
    func pause()
    func seek(to seconds: Double)
}

@MainActor
public final class NativePlaybackEngine: PlaybackEngine {
    private let engine = AVAudioEngine()
    private let node = AVAudioPlayerNode()
    private let timePitch = AVAudioUnitTimePitch()
    private let analyzer = PlaybackSpectrum()
    private var file: AVAudioFile?
    private var origin: Double = 0
    private var heldPosition: Double = 0
    private var generation: UInt64 = 0
    private var scheduled = false
    private var tapped = false
    private var timer: Timer?
    public var onTick: ((Double) -> Void)?
    public private(set) var isPlaying = false
    public var isLoaded: Bool { file != nil }
    public var duration: Double { file.map { Double($0.length) / $0.processingFormat.sampleRate } ?? 0 }
    public var rate: Float = 1 { didSet { timePitch.rate = rate } }
    public var pitch: Int = 0 { didSet { timePitch.pitch = Float(pitch * 100) } }
    public var volume: Float = 1 { didSet { engine.mainMixerNode.outputVolume = volume } }
    public var spectrumEnabled = true { didSet { updateTap() } }
    public var spectrum: [Float] { isPlaying && spectrumEnabled ? analyzer.snapshot() : Array(repeating: 0, count: 48) }

    public var position: Double {
        guard isPlaying, let time = node.lastRenderTime,
              let sourceTime = node.playerTime(forNodeTime: time), sourceTime.sampleRate > 0 else { return heldPosition }
        // playerTime 位于 TimePitch 之前，已是源音频帧；不能再乘 rate。
        return min(duration, max(origin, origin + Double(sourceTime.sampleTime) / sourceTime.sampleRate))
    }

    public init() {
        engine.attach(node)
        engine.attach(timePitch)
        let ticker = Timer(timeInterval: 0.05, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.onTick?(self.position)
            }
        }
        RunLoop.main.add(ticker, forMode: .common)
        timer = ticker
    }

    deinit { timer?.invalidate() }

    public func load(_ url: URL) throws {
        unload()
        let decoded = try AVAudioFile(forReading: url)
        guard decoded.length > 0, decoded.processingFormat.sampleRate > 0 else {
            throw NSError(domain: "Playback", code: 1, userInfo: [NSLocalizedDescriptionKey: "音频没有可播放的采样"])
        }
        engine.connect(node, to: timePitch, format: decoded.processingFormat)
        engine.connect(timePitch, to: engine.mainMixerNode, format: decoded.processingFormat)
        file = decoded
        updateTap()
    }

    public func unload() {
        generation &+= 1
        isPlaying = false
        node.stop()
        engine.stop()
        removeTap()
        engine.disconnectNodeOutput(node)
        engine.disconnectNodeOutput(timePitch)
        file = nil
        origin = 0; heldPosition = 0; scheduled = false
    }

    public func play() throws {
        guard isLoaded, !isPlaying else { return }
        if heldPosition >= duration { seek(to: 0) }
        if !scheduled { schedule() }
        if !engine.isRunning { engine.prepare(); try engine.start() }
        node.play()
        isPlaying = true
    }

    public func pause() {
        heldPosition = position
        node.pause()
        isPlaying = false
    }

    public func seek(to seconds: Double) {
        guard seconds.isFinite, let file else { return }
        let resume = isPlaying
        generation &+= 1
        node.stop()
        // 取整到源采样帧，seek 后不使用上一次调音的渲染时钟。
        let frame = min(file.length, max(0, AVAudioFramePosition(seconds * file.processingFormat.sampleRate)))
        heldPosition = Double(frame) / file.processingFormat.sampleRate
        origin = heldPosition
        scheduled = false
        schedule()
        if resume && scheduled { node.play() }
        isPlaying = resume && scheduled
    }

    private func schedule() {
        guard let file else { return }
        let frame = min(file.length, AVAudioFramePosition(heldPosition * file.processingFormat.sampleRate))
        let remaining = file.length - frame
        guard remaining > 0 else { return }
        let request = generation
        // 单段最多 UInt32.max 帧；正常歌曲远低于此上限。
        let count = AVAudioFrameCount(min(remaining, AVAudioFramePosition(UInt32.max)))
        let segmentEnd = heldPosition + Double(count) / file.processingFormat.sampleRate
        scheduled = true
        node.scheduleSegment(file, startingFrame: frame, frameCount: count, at: nil,
                             completionCallbackType: .dataPlayedBack) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, self.generation == request else { return }
                self.heldPosition = min(self.duration, segmentEnd)
                self.scheduled = false
                if self.heldPosition < self.duration {
                    self.schedule()
                    if self.isPlaying { self.node.play() }
                } else {
                    self.isPlaying = false
                    self.onTick?(self.heldPosition)
                }
            }
        }
    }

    private func updateTap() {
        removeTap()
        guard spectrumEnabled, isLoaded else { return }
        let analyzer = analyzer
        engine.mainMixerNode.installTap(onBus: 0, bufferSize: 1024, format: nil) { buffer, _ in
            analyzer.consume(buffer)
        }
        tapped = true
    }

    private func removeTap() {
        if tapped { engine.mainMixerNode.removeTap(onBus: 0); tapped = false }
        analyzer.clear()
    }
}
