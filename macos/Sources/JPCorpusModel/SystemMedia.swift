import Foundation
import JPCore

/// 报给系统「正在播放」的那几项。只在这些变了时才报（不是每个时钟周期）
public struct NowPlayingState: Equatable, Sendable {
    public var track: Track?
    public var duration: Double
    public var position: Double
    public var isPlaying: Bool
    public var rate: Float

    public init(track: Track?, duration: Double, position: Double, isPlaying: Bool, rate: Float) {
        self.track = track
        self.duration = duration
        self.position = position
        self.isPlaying = isPlaying
        self.rate = rate
    }
}

/// 接系统媒体中心的那一端（应用里是 MPNowPlayingInfoCenter，测试里可以换成记录器）
@MainActor
public protocol SystemMediaPublishing: AnyObject {
    func publish(_ state: NowPlayingState)
}

/// 媒体键、耳机按键、控制中心发来的命令。
/// 没有播放队列，「上一首 / 下一首」按上一行 / 下一行处理：学歌词时最常用的就是这个
public enum MediaCommand: Equatable, Sendable {
    case play, pause, toggle
    case nextLine, previousLine
    case skip(Double)
    case seek(Double)
}

extension PlayerModel {
    /// 执行一条系统媒体命令。返回 false 表示现在没有能操作的歌（系统会把按键交给别的应用）
    @discardableResult
    public func perform(_ command: MediaCommand) async -> Bool {
        guard track != nil, canPlay else { return false }
        switch command {
        case .play: if !isPlaying { play() }
        case .pause: if isPlaying { pause() }
        case .toggle: toggle()
        case .nextLine: await step(1)
        case .previousLine: await step(-1)
        case .skip(let seconds): await nudge(seconds)
        case .seek(let seconds): await seek(to: seconds)
        }
        return true
    }
}
