import AVFoundation
import Foundation

/// 所有位置均为源音频秒数，调音不改变歌词时间轴。
@MainActor
public protocol PlaybackEngine: AnyObject {
    var outputAvailable: Bool { get }
    var isLoaded: Bool { get }
    var isPlaying: Bool { get }
    var position: Double { get }
    var duration: Double { get }
    var rate: Float { get set }
    var pitch: Int { get set }
    var volume: Float { get set }
    var spectrumEnabled: Bool { get set }
    var spectrum: [Float] { get }
    var lastError: String? { get }
    var onTick: ((Double) -> Void)? { get set }
    var onStateChange: (() -> Void)? { get set }
    func load(_ url: URL) throws
    func unload()
    func play() throws
    func pause()
    func seek(to seconds: Double)
}

public extension PlaybackEngine { var outputAvailable: Bool { true } }

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
    private var reconnectRequired = false
    /// 只在播放时存在：暂停、卸载、放完就停，空闲时不再每 50ms 唤醒一次主线程
    private var timer: Timer?
    private var configurationObserver: NSObjectProtocol?
    public var onTick: ((Double) -> Void)?
    public var onStateChange: (() -> Void)?
    public private(set) var lastError: String?
    public private(set) var isPlaying = false
    public var outputAvailable: Bool { engine.outputNode.outputFormat(forBus: 0).channelCount > 0 }
    public var isLoaded: Bool { file != nil }
    public var duration: Double { file.map { Double($0.length) / $0.processingFormat.sampleRate } ?? 0 }
    public var rate: Float = 1 { didSet { timePitch.rate = rate; updateBypass() } }
    public var pitch: Int = 0 { didSet { timePitch.pitch = Float(pitch * 100); updateBypass() } }
    public var volume: Float = 1 { didSet { engine.mainMixerNode.outputVolume = volume } }
    public var spectrumEnabled = true { didSet { updateTap() } }
    public var spectrum: [Float] { isPlaying && spectrumEnabled ? analyzer.snapshot() : Array(repeating: 0, count: 48) }

    public var position: Double {
        guard isPlaying, engine.isRunning, node.isPlaying, let time = node.lastRenderTime,
              let sourceTime = node.playerTime(forNodeTime: time), sourceTime.sampleRate > 0 else { return heldPosition }
        // playerTime 位于 TimePitch 之前，已是源音频帧；不能再乘 rate。
        return min(duration, max(origin, origin + Double(sourceTime.sampleTime) / sourceTime.sampleRate))
    }

    public init() {
        engine.attach(node)
        engine.attach(timePitch)
        configurationObserver = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil
        ) { [weak self] _ in
            // Leave the audio engine's notification stack before mutating its graph on the main thread.
            Task { @MainActor [weak self] in self?.configurationChanged() }
        }
    }

    /// 开始走时钟（播放时）。已经在走就不重开
    private func startTicker() {
        guard timer == nil else { return }
        let ticker = Timer(timeInterval: 0.05, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tickNow() }
        }
        // 允许系统把这个计时器和别的唤醒合并：省电，进度显示看不出差别
        ticker.tolerance = 0.01
        RunLoop.main.add(ticker, forMode: .common)
        timer = ticker
    }

    private func stopTicker() {
        timer?.invalidate()
        timer = nil
    }

    private func tickNow() {
        guard isPlaying else { stopTicker(); return }
        if !engine.isRunning { configurationChanged() }
        let seconds = position
        // The render clock can disappear when the device stops. Keep the last source position (within one tick).
        heldPosition = seconds
        onTick?(seconds)
        if !isPlaying { stopTicker() }
    }

    deinit {
        timer?.invalidate()
        if let configurationObserver { NotificationCenter.default.removeObserver(configurationObserver) }
    }

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
        stopTicker()
        node.stop()
        engine.stop()
        removeTap()
        engine.disconnectNodeOutput(node)
        engine.disconnectNodeOutput(timePitch)
        file = nil
        lastError = nil
        reconnectRequired = false
        origin = 0; heldPosition = 0; scheduled = false
    }

    public func play() throws {
        guard isLoaded, !isPlaying else { return }
        if reconnectRequired { try rebuildConnections(at: heldPosition) }
        if heldPosition >= duration { seek(to: 0) }
        if !scheduled { schedule() }
        if !engine.isRunning { engine.prepare(); try engine.start() }
        node.play()
        isPlaying = true
        lastError = nil
        startTicker()
    }

    public func pause() {
        heldPosition = position
        node.pause()
        isPlaying = false
        stopTicker()
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
        if !isPlaying { stopTicker() }
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
                guard let self, self.generation == request, self.engine.isRunning else { return }
                self.heldPosition = min(self.duration, segmentEnd)
                self.scheduled = false
                if self.heldPosition < self.duration {
                    self.schedule()
                    if self.isPlaying { self.node.play() }
                } else {
                    self.isPlaying = false
                    self.stopTicker()
                    self.onTick?(self.heldPosition)
                }
            }
        }
    }

    private func configurationChanged() {
        // Hardware changes stop AVAudioEngine. Ignore stale notifications once it has already recovered.
        guard isLoaded, !engine.isRunning else { return }
        let resume = isPlaying
        let seconds = position
        isPlaying = false
        do {
            try rebuildConnections(at: seconds)
            if resume && heldPosition < duration { try play() }
            lastError = nil
        } catch {
            reconnectRequired = true
            generation &+= 1
            node.stop()
            engine.stop()
            removeTap()
            scheduled = false
            isPlaying = false
            lastError = "音频输出设备变化后无法恢复播放：\(error.localizedDescription)"
        }
        if !isPlaying { stopTicker() }
        onStateChange?()
    }

    private func rebuildConnections(at seconds: Double) throws {
        guard let file else { return }
        reconnectRequired = true
        generation &+= 1 // Discard completions from the device's discarded schedule.
        isPlaying = false
        node.stop()
        engine.stop()
        removeTap()
        scheduled = false
        heldPosition = seconds
        origin = seconds
        engine.disconnectNodeOutput(node)
        engine.disconnectNodeOutput(timePitch)
        engine.disconnectNodeOutput(engine.mainMixerNode)
        engine.reset()
        let outputFormat = engine.outputNode.inputFormat(forBus: 0)
        guard outputFormat.sampleRate > 0, outputFormat.channelCount > 0 else {
            throw NSError(domain: "Playback", code: 2, userInfo: [NSLocalizedDescriptionKey: "没有可用的音频输出设备"])
        }
        engine.connect(node, to: timePitch, format: file.processingFormat)
        engine.connect(timePitch, to: engine.mainMixerNode, format: file.processingFormat)
        engine.connect(engine.mainMixerNode, to: engine.outputNode, format: outputFormat)
        timePitch.rate = rate
        timePitch.pitch = Float(pitch * 100)
        updateBypass()
        engine.mainMixerNode.outputVolume = volume
        updateTap()
        seek(to: seconds)
        reconnectRequired = false
    }

    /// 原速、原调时跳过变速变调：TimePitch 在 1.0× / 0 音分时也照样逐帧做时频处理，
    /// 白占一份 CPU，还会轻微改变音色
    private func updateBypass() {
        timePitch.bypass = rate == 1 && pitch == 0
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
