import AppKit
import JPCore
import SwiftUI

/// 全屏歌词。对应 Windows 版的 `LyricsStage`（照 Spotify / Apple Music 的全屏歌词）：
///
/// - 盖住整个窗口，窗口同时进 macOS 的原生全屏（退出时如果是我们进的就退回来）；
/// - 底是整张封面大幅虚化铺满，再压一层暗色，字一律白色；没有封面时是深色渐变；
/// - 左边大封面和歌名，右边歌词：当前行放大、停在可视高度 45% 处，换行时平滑滚过去；
/// - 点一行跳到那一句；振假名开关和歌词面板是同一份；底部是同一个播放条；
/// - Esc 或右上角的按钮退出。
@MainActor
struct LyricsStage: View {
    @Environment(AppModel.self) private var app
    @State private var cover: NSImage?
    /// 进全屏歌词时窗口是不是已经全屏了：是我们切进去的，退出时才切回来
    @State private var enteredFullScreen = false

    /// 当前行停在可视高度的这个位置（和 Windows 版一样）
    private static let anchor = UnitPoint(x: 0.5, y: 0.45)

    var body: some View {
        let player = app.player
        GeometryReader { geo in
            ZStack {
                backdrop
                    .ignoresSafeArea()
                VStack(spacing: 0) {
                    topBar
                    HStack(alignment: .center, spacing: 48) {
                        if geo.size.width > 900, let track = player.track {
                            nowPlaying(track, width: min(geo.size.width * 0.28, geo.size.height * 0.45, 420))
                        }
                        lyrics
                    }
                    // 夹在顶栏和播放条之间，剩多高占多高：不给上限的话歌词那一栏按内容的理想高度撑开，把播放条挤出窗口
                    .frame(maxHeight: .infinity)
                    .padding(.horizontal, geo.size.width > 900 ? 64 : 28)
                    PlayerBar()
                        .environment(\.colorScheme, .dark)
                        .padding(.horizontal, 24)
                        .padding(.bottom, 18)
                        .frame(maxWidth: 1100)
                }
            }
        }
        .environment(\.colorScheme, .dark)
        .onExitCommand { close() }
        .task(id: player.track?.id) {
            cover = nil
            guard let track = player.track, !track.coverPath.isEmpty, let client = app.client else { return }
            cover = loadImage(atPath: (try? await client.resolveMedia(track.coverPath)) ?? nil)
        }
        .onAppear { enterFullScreen() }
        // 菜单（⇧⌘F）、按钮、Esc 关掉都走这里：是我们切进的全屏，就切回来
        .onDisappear { leaveFullScreen() }
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

    private var window: NSWindow? {
        NSApplication.shared.keyWindow ?? NSApplication.shared.windows.first { $0.isVisible }
    }

    private func enterFullScreen() {
        // 冒烟截图时不切窗口：CI 的虚拟显示器上切全屏要等动画，截出来的可能是半截
        guard !SmokeRun.isEnabled, let window, !window.styleMask.contains(.fullScreen) else { return }
        enteredFullScreen = true
        window.toggleFullScreen(nil)
    }

    private func leaveFullScreen() {
        if enteredFullScreen, let window, window.styleMask.contains(.fullScreen) {
            window.toggleFullScreen(nil)
        }
        enteredFullScreen = false
    }

    private func close() {
        app.showStage = false
    }
}
