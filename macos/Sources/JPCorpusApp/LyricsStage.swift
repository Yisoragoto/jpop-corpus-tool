import AppKit
import JPCore
import SwiftUI
import JPCorpusModel

/// 全屏歌词。对应 Windows 版的 `LyricsStage`（照 Spotify / Apple Music 的全屏歌词）：
///
/// - 单独一个没有标题栏的窗口，打开就进 macOS 的原生全屏，关掉就没了（主窗口不受影响）；
/// - 底是整张封面大幅虚化铺满，再压一层暗色，字一律白色；没有封面时是深色渐变；
/// - 左边大封面和歌名，右边歌词：当前行放大、停在可视高度 45% 处，换行时平滑滚过去；
/// - 点一行跳到那一句；振假名开关和歌词面板是同一份；底部是同一个播放条；
/// - Esc 或右上角的按钮退出。
@MainActor
struct LyricsStage: View {
    @Environment(AppModel.self) private var app
    @State private var cover: NSImage?

    /// 当前行停在可视高度的这个位置（和 Windows 版一样）
    private static let anchor = UnitPoint(x: 0.5, y: 0.45)

    /// 顶栏和播放条的高度。写死而不是让它们自己撑：中间那一栏拿剩下的，绝不会把播放条挤出窗口
    private static let topHeight: CGFloat = 56
    private static let barHeight: CGFloat = 92

    var body: some View {
        let player = app.player
        GeometryReader { geo in
            let middle = max(geo.size.height - Self.topHeight - Self.barHeight, 160)
            VStack(spacing: 0) {
                topBar
                    .frame(height: Self.topHeight)
                HStack(alignment: .center, spacing: 48) {
                    if geo.size.width > 900, let track = player.track {
                        nowPlaying(track, width: min(geo.size.width * 0.28, middle * 0.7, 420))
                    }
                    lyrics
                }
                .padding(.horizontal, geo.size.width > 900 ? 64 : 28)
                .frame(height: middle)
                PlayerBar()
                    .padding(.horizontal, 24)
                    .frame(maxWidth: 1100)
                    .frame(height: Self.barHeight)
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .onAppear {
                if SmokeRun.isEnabled { SmokeRun.trace("全屏歌词窗口 \(Int(geo.size.width))×\(Int(geo.size.height))") }
            }
        }
        .background { backdrop.ignoresSafeArea() }
        .environment(\.colorScheme, .dark)
        .onExitCommand { close() }
        .task(id: player.track?.id) {
            cover = nil
            guard let track = player.track, !track.coverPath.isEmpty, let client = app.client else { return }
            cover = loadImage(atPath: (try? await client.resolveMedia(track.coverPath)) ?? nil)
        }
        .background(WindowReader { window = $0 })
        .onChange(of: window) { _, w in enterFullScreen(w) }
        // 窗口被关掉（红色按钮、⇧⌘F、Esc）都会走到这里：把状态收回来
        .onDisappear { app.showStage = false }
    }

    // MARK: - 各块

    private var backdrop: some View {
        ZStack {
            LinearGradient(colors: [Color(white: 0.12), Color(white: 0.04)], startPoint: .top, endPoint: .bottom)
            if let cover {
                Image(nsImage: cover)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .blur(radius: 90, opaque: true)
                    .saturation(1.5)
                    .clipped()
                    .transition(.opacity)
                Color.black.opacity(0.42)
            }
        }
        .animation(.easeInOut(duration: 0.8), value: cover != nil)
    }

    private var topBar: some View {
        HStack(spacing: 14) {
            Spacer()
            FuriganaMenu()
            Button {
                close()
            } label: {
                Image(systemName: "arrow.down.right.and.arrow.up.left")
                    .font(.title3)
            }
            .buttonStyle(.borderless)
            .keyboardShortcut(.cancelAction)
            .help("退出全屏歌词（Esc）")
        }
        .foregroundStyle(.white.opacity(0.8))
        .padding(.horizontal, 24)
        .padding(.top, 18)
        .frame(height: 56)
    }

    private func nowPlaying(_ track: Track, width: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Group {
                if let cover {
                    Image(nsImage: cover).resizable().aspectRatio(contentMode: .fill)
                } else {
                    ZStack {
                        Rectangle().fill(.white.opacity(0.08))
                        Image(systemName: "music.note").font(.system(size: width * 0.25)).foregroundStyle(.white.opacity(0.4))
                    }
                }
            }
            .frame(width: width, height: width)
            .layoutPriority(1)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .shadow(color: .black.opacity(0.45), radius: 30, y: 14)
            Text(track.title)
                .font(.system(size: 26, weight: .bold))
                .foregroundStyle(.white)
                .lineLimit(2)
            Text([track.artist, track.album, track.year].filter { !$0.isEmpty }.joined(separator: " · "))
                .font(.title3)
                .foregroundStyle(.white.opacity(0.65))
                .lineLimit(2)
        }
        .frame(width: width)
    }

    private var lyrics: some View {
        let player = app.player
        return ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 18) {
                    // 上下各留半屏空，第一行和最后一行也能滚到中间
                    Color.clear.frame(height: 260)
                    ForEach(player.lines) { line in
                        stageLine(line, current: line.utteranceId == player.currentUtterance)
                            .id(line.utteranceId)
                    }
                    Color.clear.frame(height: 320)
                }
                .padding(.horizontal, 8)
            }
            .scrollIndicators(.never)
            .mask(
                LinearGradient(stops: [
                    .init(color: .clear, location: 0),
                    .init(color: .black, location: 0.12),
                    .init(color: .black, location: 0.85),
                    .init(color: .clear, location: 1),
                ], startPoint: .top, endPoint: .bottom)
            )
            .onChange(of: player.currentUtterance) { _, id in
                guard let id else { return }
                withAnimation(.spring(duration: 0.55)) { proxy.scrollTo(id, anchor: Self.anchor) }
            }
            .onAppear {
                if let id = player.currentUtterance ?? player.focusUtterance {
                    proxy.scrollTo(id, anchor: Self.anchor)
                }
            }
            .overlay {
                if player.lines.isEmpty {
                    Text("这首歌没有歌词").font(.title2).foregroundStyle(.white.opacity(0.5))
                }
            }
        }
        .frame(maxWidth: .infinity)
    }

    private func stageLine(_ line: LyricLine, current: Bool) -> some View {
        FuriganaText(text: line.text, rubies: app.furigana[line.utteranceId] ?? [], size: 34, weight: .bold)
            .foregroundStyle(.white)
            .opacity(current ? 1 : (line.timeSec == nil ? 0.25 : 0.38))
            .scaleEffect(current ? 1 : 0.94, anchor: .leading)
            .blur(radius: current ? 0 : 0.6)
            .animation(.easeInOut(duration: 0.35), value: current)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture { Task { await app.player.playLine(line) } }
            .help(line.timeSec.map { "点一下从 \(formatTime($0)) 放" } ?? "这一行没有时间戳")
    }

    // MARK: - 窗口

    @State private var window: NSWindow?

    /// 自己的窗口一拿到就进原生全屏。冒烟截图时不切：CI 的虚拟显示器上切全屏要等动画，截出来的可能是半截
    private func enterFullScreen(_ window: NSWindow?) {
        guard !SmokeRun.isEnabled, let window, !window.styleMask.contains(.fullScreen) else { return }
        window.toggleFullScreen(nil)
    }

    private func close() {
        app.showStage = false
    }
}

/// 拿到这个视图所在的 NSWindow
@MainActor
struct WindowReader: NSViewRepresentable {
    let found: (NSWindow?) -> Void

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async { found(view.window) }
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {}
}
