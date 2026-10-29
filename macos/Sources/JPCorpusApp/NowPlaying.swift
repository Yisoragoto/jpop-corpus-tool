import AppKit
import JPCore
import JPCorpusModel
import MediaPlayer

/// 系统的「正在播放」：媒体键（F8 播放 / 暂停）、耳机线控、控制中心、锁屏都走这里。
///
/// macOS 只把媒体键发给「正在播放」的应用，而认不认你是正在播放的应用，看的是
/// MPNowPlayingInfoCenter 有没有信息、`playbackState` 是不是 playing / paused。
/// 所以光注册命令不够，状态也要报。
@MainActor
final class SystemMediaControls: SystemMediaPublishing {
    private weak var player: PlayerModel?
    private var resolveCover: @MainActor (Track) async -> String?
    private var targets: [(MPRemoteCommand, Any)] = []
    private var artworkFor: String?
    private var artwork: MPMediaItemArtwork?
    private var artworkTask: Task<Void, Never>?
    private var last: NowPlayingState?

    init(player: PlayerModel, resolveCover: @escaping @MainActor (Track) async -> String?) {
        self.player = player
        self.resolveCover = resolveCover
        let center = MPRemoteCommandCenter.shared()
        bind(center.playCommand, .play)
        bind(center.pauseCommand, .pause)
        bind(center.togglePlayPauseCommand, .toggle)
        bind(center.nextTrackCommand, .nextTrack)
        bind(center.previousTrackCommand, .previousTrack)
        center.skipForwardCommand.preferredIntervals = [5]
        center.skipBackwardCommand.preferredIntervals = [5]
        bind(center.skipForwardCommand, .skip(5))
        bind(center.skipBackwardCommand, .skip(-5))
        let seek = center.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            return Self.dispatch(self, .seek(event.positionTime))
        }
        targets.append((center.changePlaybackPositionCommand, seek))
        player.systemMedia = self
    }

    private func bind(_ command: MPRemoteCommand, _ action: MediaCommand) {
        command.isEnabled = true
        let target = command.addTarget { [weak self] _ in Self.dispatch(self, action) }
        targets.append((command, target))
    }

    /// 系统一般在主线程上调命令；万一不是，就切回主线程再做（先答应系统，免得按键被转给别的应用）
    nonisolated private static func dispatch(_ controls: SystemMediaControls?, _ action: MediaCommand) -> MPRemoteCommandHandlerStatus {
        if Thread.isMainThread {
            return MainActor.assumeIsolated { controls?.run(action) ?? .commandFailed }
        }
        Task { @MainActor in _ = controls?.run(action) }
        return .success
    }

    /// 真正的跳转是异步的，这里只判断现在有没有能操作的歌
    private func run(_ action: MediaCommand) -> MPRemoteCommandHandlerStatus {
        guard let player, player.track != nil else { return .noActionableNowPlayingItem }
        switch action {
        case .nextTrack: guard player.hasNext else { return .noSuchContent }
        case .previousTrack: break
        default: guard player.canPlay else { return .noActionableNowPlayingItem }
        }
        Task { await player.perform(action) }
        return .success
    }

    func publish(_ state: NowPlayingState) {
        guard state != last else { return }
        last = state
        // 队列放到最后一首：系统的「下一首」变灰，不留一个按了没反应的按钮
        MPRemoteCommandCenter.shared().nextTrackCommand.isEnabled = state.hasNext
        let center = MPNowPlayingInfoCenter.default()
        guard let track = state.track else {
            center.nowPlayingInfo = nil
            center.playbackState = .stopped
            return
        }
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: track.title,
            MPMediaItemPropertyArtist: track.artist,
            MPMediaItemPropertyAlbumTitle: track.album,
            MPMediaItemPropertyPlaybackDuration: max(0, state.duration),
            MPNowPlayingInfoPropertyElapsedPlaybackTime: max(0, state.position),
            // 系统按这个速度自己往前推进度条；暂停时是 0
            MPNowPlayingInfoPropertyPlaybackRate: state.isPlaying ? Double(state.rate) : 0,
            MPNowPlayingInfoPropertyDefaultPlaybackRate: 1.0,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
        ]
        if artworkFor == track.id, let artwork { info[MPMediaItemPropertyArtwork] = artwork }
        center.nowPlayingInfo = info
        center.playbackState = state.isPlaying ? .playing : .paused
        if artworkFor != track.id { loadArtwork(for: track) }
    }

    private func loadArtwork(for track: Track) {
        artworkFor = track.id
        artwork = nil
        artworkTask?.cancel()
        guard !track.coverPath.isEmpty else { return }
        artworkTask = Task { [weak self] in
            guard let self else { return }
            let path = await self.resolveCover(track)
            guard !Task.isCancelled, let image = await Artwork.image(atPath: path, maxPixel: 600),
                  !Task.isCancelled, self.artworkFor == track.id else { return }
            self.artwork = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
            let center = MPNowPlayingInfoCenter.default()
            if var info = center.nowPlayingInfo, self.last?.track?.id == track.id {
                info[MPMediaItemPropertyArtwork] = self.artwork
                center.nowPlayingInfo = info
            }
        }
    }
}
