import AppKit
import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct PlayerBar: View {
    @Environment(AppModel.self) private var app
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var cover: NSImage?
    @State private var colors = ArtworkColors()

    var body: some View {
        VStack(spacing: 4) {
            if app.player.spectrumEnabled { PlayerSpectrum().frame(height: 18) }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 16) {
                    PlayerTransport(tint: colors.highlight)
                    song.frame(minWidth: 160, maxWidth: .infinity)
                    PlayerTools()
                    PlayerVolume().frame(width: 110)
                }
                VStack(spacing: 6) {
                    HStack(spacing: 10) {
                        PlayerTransport(tint: colors.highlight)
                        song.frame(minWidth: 90, maxWidth: .infinity)
                    }
                    ViewThatFits(in: .horizontal) {
                        HStack {
                            PlayerTools()
                            Spacer(minLength: 8)
                            PlayerVolume().frame(width: 90)
                        }
                        VStack(alignment: .trailing, spacing: 6) {
                            PlayerTools()
                            PlayerVolume().frame(width: 110)
                        }.frame(maxWidth: .infinity, alignment: .trailing)
                    }
                }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background {
            ZStack {
                GlassBackground(material: .hudWindow, blending: .withinWindow)
                colors.backdrop.opacity(0.5)
                if let cover {
                    Image(nsImage: cover).resizable().scaledToFill()
                        .blur(radius: 50).saturation(1.6).opacity(0.3)
                        .transition(.opacity)
                }
            }
            .clipped()
            .clipShape(RoundedRectangle(cornerRadius: 14))
            .animation(reduceMotion ? nil : .easeInOut(duration: 0.8), value: colors.highlight)
        }
        .overlay(alignment: .top) {
            LinearGradient(colors: [.clear, colors.highlight.opacity(0.5), .clear], startPoint: .leading, endPoint: .trailing)
                .frame(height: 1).padding(.horizontal, 14).allowsHitTesting(false)
        }
        .task(id: app.player.track?.coverPath) {
            cover = nil; colors = ArtworkColors()
            guard let track = app.player.track, !track.coverPath.isEmpty, let client = app.client,
                  let path = try? await client.resolveMedia(track.coverPath), !Task.isCancelled else { return }
            cover = loadImage(atPath: path)
            colors = ArtworkColors(image: cover)
        }
    }

    private var song: some View {
        VStack(spacing: 2) {
            Button { app.showStage = true } label: {
                HStack(spacing: 9) {
                    PlayerCover(image: cover).frame(width: 38, height: 38)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(app.player.track?.title ?? "未在播放").font(.callout.weight(.semibold)).lineLimit(1)
                        Text(app.player.track?.artist ?? "选择一首歌开始播放").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .id(app.player.track?.id)
                .transition(.asymmetric(insertion: .move(edge: .trailing).combined(with: .opacity), removal: .opacity))
            }
            .buttonStyle(.plain)
            .disabled(app.player.track == nil)
            .help("打开全屏歌词（⇧⌘F）")
            .clipped()
            .animation(reduceMotion ? nil : .spring(duration: 0.45), value: app.player.track?.id)
            PlayerProgress()
        }
    }
}

@MainActor
struct PlayerCover: View {
    let image: NSImage?
    var body: some View {
        GeometryReader { geo in
            Group {
                if let image { Image(nsImage: image).resizable().scaledToFill() }
                else {
                    ZStack {
                        Color.white.opacity(0.08)
                        Image(systemName: "music.note").font(.system(size: geo.size.width * 0.3)).foregroundStyle(.secondary)
                    }
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .clipShape(RoundedRectangle(cornerRadius: min(14, geo.size.width * 0.12)))
        }
    }
}

@MainActor
struct PlayerTransport: View {
    @Environment(AppModel.self) private var app
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    var tint: Color = .white
    var large = false
    var body: some View {
        HStack(spacing: large ? 20 : 3) {
            Button { Task { await app.player.nudge(-5) } } label: { Image(systemName: "gobackward.5") }
                .help("后退 5 秒（←）").accessibilityLabel("后退 5 秒")
            Button { app.player.toggle() } label: {
                ZStack {
                    Image(systemName: "play.fill").opacity(app.player.isPlaying ? 0 : 1)
                        .scaleEffect(app.player.isPlaying ? 0.7 : 1)
                    Image(systemName: "pause.fill").opacity(app.player.isPlaying ? 1 : 0)
                        .scaleEffect(app.player.isPlaying ? 1 : 0.7)
                }
                .font(.system(size: large ? 32 : 22, weight: .bold))
                .shadow(color: tint.opacity(app.player.isPlaying ? 0.65 : 0), radius: 12)
                .animation(reduceMotion ? nil : .easeInOut(duration: 0.2), value: app.player.isPlaying)
            }
            .help("播放 / 暂停（空格）").accessibilityLabel(app.player.isPlaying ? "暂停" : "播放")
            Button { Task { await app.player.nudge(5) } } label: { Image(systemName: "goforward.5") }
                .help("前进 5 秒（→）").accessibilityLabel("前进 5 秒")
        }
        .font(.system(size: large ? 23 : 18, weight: .semibold))
        .buttonStyle(PlayerGlyphStyle(diameter: large ? 48 : 32))
        .disabled(!app.player.canPlay)
    }
}

@MainActor
struct PlayerProgress: View {
    @Environment(AppModel.self) private var app
    var alwaysShowTime = false
    @State private var hover = false
    @State private var scrubbing: Double?
    @State private var seekVersion = 0
    @State private var committingVersion: Int?
    @GestureState private var dragging = false

    var body: some View {
        let player = app.player
        let value = scrubbing ?? player.position
        VStack(spacing: 2) {
            GeometryReader { geo in
                let fraction = min(1, max(0, value / max(1, player.duration)))
                ZStack(alignment: .leading) {
                    Capsule().fill(.primary.opacity(0.14))
                    Capsule().fill(.primary.opacity(0.65)).frame(width: geo.size.width * fraction)
                    if hover || scrubbing != nil {
                        Circle().fill(.primary).frame(width: 8, height: 8)
                            .offset(x: max(0, geo.size.width - 8) * fraction)
                    }
                }
                .frame(height: hover || scrubbing != nil ? 5 : 3)
                .frame(height: 12)
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0)
                    .updating($dragging) { _, active, _ in active = true }
                    .onChanged { event in
                        seekVersion += 1
                        scrubbing = min(1, max(0, event.location.x / max(1, geo.size.width))) * player.duration
                    }
                    .onEnded { _ in commit() })
                .accessibilityElement()
                .accessibilityLabel("播放进度")
                .accessibilityValue(formatTime(value))
                .accessibilityAdjustableAction { direction in
                    switch direction {
                    case .increment: Task { await player.nudge(5) }
                    case .decrement: Task { await player.nudge(-5) }
                    @unknown default: break
                    }
                }
            }.frame(height: 12)
            HStack {
                Text(formatTime(value))
                Spacer()
                Button { player.toggleTimeDisplay() } label: {
                    Text(player.duration > 0 ? (player.showRemaining ? "−" + formatTime(max(0, player.duration - value)) : formatTime(player.duration)) : "—")
                }
                .buttonStyle(.plain)
                .help(player.showRemaining ? "显示总时长" : "显示剩余时长")
                .disabled(player.duration <= 0)
            }
            .font(.system(size: 10).monospacedDigit()).foregroundStyle(.secondary)
            .opacity(alwaysShowTime || hover || scrubbing != nil ? 1 : 0)
        }
        .onHover { hover = $0 }
        .onChange(of: dragging) { _, active in if !active { commit() } }
        .disabled(!player.canPlay)
        .onChange(of: player.track?.id) { _, _ in seekVersion += 1; scrubbing = nil }
    }

    private func commit() {
        guard let target = scrubbing, committingVersion != seekVersion else { return }
        let request = seekVersion
        committingVersion = request
        Task {
            await app.player.seek(to: target)
            if seekVersion == request { scrubbing = nil }
        }
    }
}

@MainActor
struct PlayerTools: View {
    @Environment(AppModel.self) private var app
    var body: some View {
        HStack(spacing: 3) {
            Button { Task { await app.player.toggleLoop(nil) } } label: { Image(systemName: "repeat.1") }
                .foregroundStyle(app.player.loop == nil ? Color.secondary : Color.accentColor)
                .disabled(app.player.loop == nil)
                .help(app.player.loop.map { "单句循环 \(formatTime($0.startSec))–\(formatTime($0.endSec))，点击取消" } ?? "在歌词里选择一句开启循环（⌘L）")
                .accessibilityLabel("取消单句循环")
            PlayerTune()
            Button { app.showLyrics.toggle() } label: { Image(systemName: "quote.bubble.fill") }
                .help("歌词面板（⌘Y）").accessibilityLabel("歌词面板")
            Button { app.showStage.toggle() } label: { Image(systemName: "arrow.up.left.and.arrow.down.right") }
                .disabled(app.player.track == nil).help("全屏歌词（⇧⌘F）").accessibilityLabel("全屏歌词")
        }
        .buttonStyle(PlayerGlyphStyle()).font(.system(size: 15, weight: .semibold))
    }
}

@MainActor
struct PlayerTune: View {
    @Environment(AppModel.self) private var app
    @State private var open = false
    var body: some View {
        Button { open.toggle() } label: {
            Image(systemName: "slider.horizontal.3")
                .overlay(alignment: .bottom) {
                    if !app.player.tuneBadge.isEmpty {
                        Text(app.player.tuneBadge).font(.system(size: 8, weight: .medium)).fixedSize()
                            .padding(.horizontal, 3).background(.regularMaterial, in: Capsule()).offset(y: 12)
                    }
                }
        }
        .foregroundStyle(app.player.tuneBadge.isEmpty ? Color.primary : Color.accentColor)
        .help("调音：速度与音调").accessibilityLabel("调音 \(app.player.tuneBadge)")
        .popover(isPresented: $open, arrowEdge: .top) {
            VStack(alignment: .leading, spacing: 12) {
                HStack { Text("速度").bold(); Spacer(); Text("不改变音高").foregroundStyle(.secondary) }
                LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 4), spacing: 8) {
                    ForEach(PlayerModel.rateSteps, id: \.self) { rate in
                        Button("\(String(format: "%g", rate))×") { app.player.setRate(rate) }
                            .tint(app.player.rate == rate ? .accentColor : .secondary)
                    }
                }
                HStack { Text("音调").bold(); Spacer(); Text("半音").foregroundStyle(.secondary) }
                LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 7), spacing: 8) {
                    ForEach(PlayerModel.pitchSteps, id: \.self) { pitch in
                        Button(pitch == 0 ? "原调" : String(format: "%+d", pitch)) { app.player.setPitch(pitch) }
                            .tint(app.player.pitch == pitch ? .accentColor : .secondary)
                    }
                }
                Button("恢复原速原调") { app.player.resetTuning() }.disabled(app.player.tuneBadge.isEmpty)
            }
            .buttonStyle(.bordered).padding(18).frame(width: 330)
            .onExitCommand { open = false }
        }
    }
}

@MainActor
struct PlayerVolume: View {
    @Environment(AppModel.self) private var app
    var body: some View {
        HStack(spacing: 2) {
            Button { app.player.toggleMute() } label: {
                Image(systemName: app.player.volume == 0 ? "speaker.slash.fill" : "speaker.fill")
            }
            .buttonStyle(PlayerGlyphStyle(diameter: 26))
            .help(app.player.volume == 0 ? "恢复音量" : "静音").accessibilityLabel(app.player.volume == 0 ? "恢复音量" : "静音")
            Slider(value: Binding(get: { Double(app.player.volume) }, set: { app.player.setVolume(Float($0)) }), in: 0...1)
                .controlSize(.mini).tint(.primary.opacity(0.7)).accessibilityLabel("音量")
        }
    }
}

@MainActor
struct PlayerSpectrum: View {
    @Environment(AppModel.self) private var app
    var body: some View {
        TimelineView(.animation(minimumInterval: 0.05, paused: !app.player.isPlaying)) { _ in
            let values = app.player.spectrum
            Canvas { context, size in
                let width = size.width / 48
                for index in 0..<48 {
                    let height = max(1, CGFloat(values[index]) * size.height)
                    let rect = CGRect(x: CGFloat(index) * width, y: size.height - height, width: max(1, width - 2), height: height)
                    context.fill(Path(roundedRect: rect, cornerRadius: 1), with: .color(.primary.opacity(0.18)))
                }
            }
        }.accessibilityHidden(true)
    }
}

struct PlayerGlyphStyle: ButtonStyle {
    var diameter: CGFloat = 32
    func makeBody(configuration: Configuration) -> some View {
        GlyphBody(configuration: configuration, diameter: diameter)
    }
    private struct GlyphBody: View {
        let configuration: Configuration
        let diameter: CGFloat
        @State private var hover = false
        @Environment(\.isEnabled) private var enabled
        var body: some View {
            configuration.label.frame(width: diameter, height: diameter)
                .background {
                    if hover || configuration.isPressed { Circle().fill(.ultraThinMaterial) }
                }
                .contentShape(Circle()).onHover { hover = $0 }
                .opacity(enabled ? (configuration.isPressed ? 0.65 : 1) : 0.35)
        }
    }
}
