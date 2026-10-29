import Foundation
import JPCore

/// 报给系统「正在播放」的那几项。只在这些变了时才报（不是每个时钟周期）
public struct NowPlayingState: Equatable, Sendable {
    public var track: Track?
    public var duration: Double
    public var position: Double
    public var isPlaying: Bool
    public var rate: Float
    /// 队列里还有下一首（控制中心的「下一首」据此变灰）
    public var hasNext: Bool

    public init(track: Track?, duration: Double, position: Double, isPlaying: Bool, rate: Float, hasNext: Bool = false) {
        self.track = track
        self.duration = duration
        self.position = position
        self.isPlaying = isPlaying
        self.rate = rate
        self.hasNext = hasNext
    }
}

/// 接系统媒体中心的那一端（应用里是 MPNowPlayingInfoCenter，测试里可以换成记录器）
@MainActor
public protocol SystemMediaPublishing: AnyObject {
    func publish(_ state: NowPlayingState)
}

/// 媒体键、耳机按键、控制中心发来的命令。
/// 「上一首 / 下一首」（F7 / F9）在播放队列里切歌；跳行用 ⌥⌘↑ / ⌥⌘↓
public enum MediaCommand: Equatable, Sendable {
    case play, pause, toggle
    case nextTrack, previousTrack
    case skip(Double)
    case seek(Double)
}

extension PlayerModel {
    /// 执行一条系统媒体命令。返回 false 表示现在没有能操作的歌（系统会把按键交给别的应用）
    @discardableResult
    public func perform(_ command: MediaCommand) async -> Bool {
        guard track != nil else { return false }
        switch command {
        // 这首放不了（找不到音频）时，切歌照样能用：才能离开这首
        case .nextTrack: return await next()
        case .previousTrack: await previous(); return true
        default: break
        }
        guard canPlay else { return false }
        switch command {
        case .nextTrack, .previousTrack: break
        case .play: if !isPlaying { play() }
        case .pause: if isPlaying { pause() }
        case .toggle: toggle()
        case .skip(let seconds): await nudge(seconds)
        case .seek(let seconds): await seek(to: seconds)
        }
        return true
    }
}
