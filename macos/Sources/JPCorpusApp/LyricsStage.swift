import AppKit
import JPCore
import SwiftUI
import JPCorpusModel

/// 全屏歌词。对应 Windows 版的 `LyricsStage`（照 Spotify / Apple Music 的全屏歌词）：
///
/// - 和 Windows 版一样是主窗口里铺满的一层（不另开窗口）；右上角可以把整个窗口切成系统全屏；
/// - 底是整张封面大幅虚化铺满，再压一层暗色，字一律白色；没有封面时是深色渐变；
/// - 左边大封面和歌名，右边歌词：当前行放大、停在可视高度 45% 处，换行时平滑滚过去；
/// - 点词查词（右边打开查词栏，不动播放进度），点行里别处跳到那一句；逐字点亮的行也一样；
/// - 振假名开关和歌词面板是同一份；底部是同一个播放条；
/// - Esc 或右上角的按钮退出。
@MainActor
struct LyricsStage: View {
    @Environment(AppModel.self) private var app
    @State private var cover: NSImage?
    /// 预先虚化好的封面（见 Artwork）：全屏的实时模糊是播放时 GPU 占用的大头
    @State private var blurredCover: NSImage?
    @State private var coverPath: String?
    @State private var karaoke = false
    @State private var stylingKaraoke = false
    @State private var credits: [Credit] = []
    @State private var colors = ArtworkColors()
    @State private var lookup: LookupRequest?
    @State private var selectionLine: LyricLine?
    @State private var manualScroll: Date?
    @AppStorage("stage-side-width") private var sideWidth = 340.0
    @State private var sideDragStart: Double?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    /// 当前行停在可视高度的这个位置（和 Windows 版一样）
    private static let anchor = UnitPoint(x: 0.5, y: 0.45)

    /// 顶栏和播放条的高度。写死而不是让它们自己撑：中间那一栏拿剩下的，绝不会把播放条挤出窗口
    private static let topHeight: CGFloat = 56
    private static let barHeight: CGFloat = PlayerBar.height + 16

    var body: some View {
        let player = app.player
        GeometryReader { geo in
            let wide = geo.size.width > 900
            // 和 Windows 一样：播放条一直在底下，右栏一直在（窄窗口只在查词时出来）。
            // 点词只换右栏里的内容，歌词和播放条的位置都不动
            let middle = max(geo.size.height - Self.topHeight - Self.barHeight, 160)
            let sideLimit = max(260, min(720, Double(geo.size.width) - 56 - 420))
            let lookupWidth = min(max(260, sideWidth), sideLimit)
            let showSide = wide || lookup != nil
            if karaoke {
                // 卡拉 OK 和 Windows 一样是另一种版面：封面铺底、两行大字压在底部，左上角是歌名
                VStack(spacing: 0) {
                    topBar(narrow: false)
                        .frame(height: Self.topHeight)
                    karaokeLyrics(width: geo.size.width)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    PlayerBar().padding(.horizontal, 24).frame(height: Self.barHeight)
                }
                .frame(width: geo.size.width, height: geo.size.height)
            } else {
            VStack(spacing: 0) {
                topBar(narrow: !wide)
                    .frame(height: Self.topHeight)
                HStack(alignment: .top, spacing: 16) {
                    lyrics
                    if showSide {
                        lookupDivider(width: lookupWidth, height: middle - 8)
                        sideColumn(width: CGFloat(lookupWidth))
                            .frame(width: CGFloat(lookupWidth), height: max(120, middle - 8))
                            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(.black.opacity(0.34)))
                            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                }
                .padding(.horizontal, 28)
                .padding(.bottom, 8)
                .frame(height: middle)
                PlayerBar().padding(.horizontal, 24).frame(height: Self.barHeight)
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .onAppear {
                if SmokeRun.isEnabled { SmokeRun.trace("全屏歌词 \(Int(geo.size.width))×\(Int(geo.size.height))") }
            }
            }
        }
        // 开着振假名却没有词典：在全屏歌词里也说清楚，并且能直接下载（以前这里什么都不提示）
        .overlay(alignment: .top) {
            FuriganaDictionaryNotice(style: .card).padding(.top, Self.topHeight + 8).padding(.horizontal, 28)
        }
        .background {
            if karaoke { karaokeBackdrop.ignoresSafeArea() } else { backdrop.ignoresSafeArea() }
        }
        .environment(\.colorScheme, .dark)
        .onAppear { karaoke = app.stageKaraoke }
        .onChange(of: karaoke) { _, value in app.stageKaraoke = value; lookup = nil }
        .onExitCommand { if selectionLine != nil { selectionLine = nil } else { close() } }
        .popover(item: $selectionLine) { line in
            LyricSelectionView(text: line.text) { selected in
                lookup = LookupRequest(text: selected, songId: player.track?.id, utteranceId: line.utteranceId, tokenIndex: nil)
                selectionLine = nil
            }
        }
        .task(id: "\(player.track?.id ?? "")|\(player.track?.coverPath ?? "")") {
            cover = nil; coverPath = nil; colors = ArtworkColors()
            let track = player.track
            let fetchedCredits = (try? await app.core?.trackCredits(track?.id ?? "")) ?? []
            guard !Task.isCancelled else { return }
            credits = fetchedCredits
            guard let track, !track.coverPath.isEmpty, let client = app.client else { return }
            let path = (try? await client.resolveMedia(track.coverPath)) ?? nil
            guard !Task.isCancelled else { return }
            let image = await Artwork.image(atPath: path, maxPixel: 1024)
            guard !Task.isCancelled else { return }
            cover = image
            coverPath = path
            colors = ArtworkColors(image: image)
        }
        // 模糊半径是设置里能调的：换了就重算一次（只算一次，不是每帧）
        .task(id: "\(coverPath ?? "")|\(app.stageCoverBlur)|\(Int(app.stageBlurRadius))") {
            guard app.stageCoverBlur, let coverPath else { blurredCover = nil; return }
            let blurred = await Artwork.blurred(atPath: coverPath, radius: app.stageBlurRadius, saturation: 1.4, displayWidth: 1500)
            guard !Task.isCancelled else { return }
            blurredCover = blurred
        }
        .background(PlaybackKeyboard(app: app, exit: { close() }))
    }

    // MARK: - 各块

    private var backdrop: some View {
        StageBackdrop(blurredCover: app.stageCoverBlur ? blurredCover : nil, colors: colors, drifting: app.player.isPlaying)
    }

    private func lookupDivider(width: Double, height: CGFloat) -> some View {
        Capsule().fill(.white.opacity(0.16)).frame(width: 4, height: 56)
            .frame(width: 8, height: height)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 1)
                .onChanged { value in
                    if sideDragStart == nil { sideDragStart = width }
                    sideWidth = min(720, max(260, (sideDragStart ?? width) - Double(value.translation.width)))
                }
                .onEnded { _ in sideDragStart = nil })
            .onTapGesture(count: 2) { sideWidth = 340 }
            .accessibilityElement().accessibilityLabel("查词栏宽度")
            .accessibilityAdjustableAction { direction in
                switch direction {
                case .increment: sideWidth = min(720, sideWidth + 20)
                case .decrement: sideWidth = max(260, sideWidth - 20)
                @unknown default: break
                }
            }
    }

    private func topBar(narrow: Bool) -> some View {
        HStack(spacing: 14) {
            if let track = app.player.track {
                VStack(alignment: .leading, spacing: 2) {
                    Text(track.title).font(.system(size: 16, weight: .semibold)).lineLimit(1)
                    Text(track.artist).font(.caption).foregroundStyle(.white.opacity(0.6)).lineLimit(1)
                }
                .foregroundStyle(.white)
                .shadow(color: .black.opacity(0.85), radius: 3, y: 1)
            }
            Spacer()
            if let track = app.player.track {
                Button { karaoke.toggle() } label: { Image(systemName: karaoke ? "quote.bubble" : "music.mic") }
                    .help(karaoke ? "切换为滚动歌词" : "切换为卡拉 OK")
                    .accessibilityLabel(karaoke ? "滚动歌词" : "卡拉 OK")
                LyricsToolsButton(songId: track.id, timing: true, iconOnly: true)
                if karaoke {
                    Button { stylingKaraoke.toggle() } label: { Image(systemName: "paintpalette") }
                        .help("字体、颜色、描边、发光、阴影").accessibilityLabel("字幕样式")
                        .popover(isPresented: $stylingKaraoke, arrowEdge: .bottom) {
                            ScrollView { KaraokeStyleControls(families: InstalledFonts.families).padding(14) }
                                .frame(width: 460, height: 560)
                        }
                }
            }
            FuriganaMenu()
            LyricsDisplayButton()
            Button { MainWindow.window?.toggleFullScreen(nil) } label: { Image(systemName: "arrow.up.left.and.arrow.down.right") }
                .help("切换窗口全屏").accessibilityLabel("窗口全屏")
            Button {
                close()
            } label: {
                Image(systemName: "arrow.down.right.and.arrow.up.left")
                    .font(.title3)
            }
            .help("退出全屏歌词（Esc）")
            .accessibilityLabel("退出全屏歌词")
        }
        .buttonStyle(PlayerGlyphStyle()).font(.system(size: 15, weight: .semibold))
        .foregroundStyle(.white.opacity(0.8))
        .padding(.horizontal, 24)
        .padding(.top, 18)
        .frame(height: 56)
    }

    /// 右栏：平时是这首歌（Windows 的 SongCard），查词时换成查词内容。宽度、位置都不变
    @ViewBuilder private func sideColumn(width: CGFloat) -> some View {
        if let lookup {
            LookupPane(request: lookup) { self.lookup = nil }
        } else if let track = app.player.track {
            ScrollView {
                songCard(track, width: width - 32).padding(16)
            }
            .scrollIndicators(.never)
        } else {
            Color.clear
        }
    }

    private func songCard(_ track: Track, width: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 10) {
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
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .compositingGroup()
            .shadow(color: .black.opacity(0.3), radius: 12, y: 4)
            .padding(.bottom, 4)
            Text(track.title)
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(.white)
                .lineLimit(2)
            Text([track.artist, track.album, track.year].filter { !$0.isEmpty }.joined(separator: " · "))
                .font(.callout)
                .foregroundStyle(.white.opacity(0.65))
                .lineLimit(2)
            VStack(alignment: .leading, spacing: 6) {
                ForEach(["performer", "lyricist", "composer", "arranger"], id: \.self) { role in
                    let names = credits.filter { $0.role == role }.map(\.name)
                    if !names.isEmpty {
                        Text("\(roleLabels[role] ?? role)：\(names.joined(separator: " / "))").font(.caption)
                    }
                }
                if !track.genre.isEmpty { Text("流派：\(track.genre)").font(.caption) }
                Text("\(app.player.lines.count) 行歌词 · \(formatTime(app.player.duration))").font(.caption)
            }.foregroundStyle(.white.opacity(0.55)).padding(.top, 6)
        }
        .frame(width: width, alignment: .leading)
    }

    private var lyrics: some View {
        let player = app.player
        return ScrollViewReader { proxy in
            ScrollView {
                // 行距跟歌词显示设置走（和歌词面板同一个滑块），按全屏的字号等比放大；默认值 6 时还是原来的 18
                LazyVStack(alignment: .leading, spacing: 4 + CGFloat(app.lyricsDisplay.lineGap) * 34 / 15) {
                    // 上下各留半屏空，第一行和最后一行也能滚到中间
                    Color.clear.frame(height: 260)
                    let active = player.currentIndex
                    ForEach(Array(player.lines.enumerated()), id: \.element.id) { index, line in
                        stageLine(line, index: index, active: active)
                            .id(line.utteranceId)
                    }
                    Color.clear.frame(height: 320)
                }
                .padding(.horizontal, 8)
            }
            .scrollIndicators(.never)
            .background(StageScrollObserver { manualScroll = Date() })
            .mask(
                LinearGradient(stops: [
                    .init(color: .clear, location: 0),
                    .init(color: .black, location: 0.12),
                    .init(color: .black, location: 0.85),
                    .init(color: .clear, location: 1),
                ], startPoint: .top, endPoint: .bottom)
            )
            .onChange(of: player.currentUtterance) { _, id in
                guard manualScroll == nil, let id else { return }
                withAnimation(reduceMotion ? nil : .spring(duration: 0.55)) { proxy.scrollTo(id, anchor: Self.anchor) }
            }
            .overlay(alignment: .bottom) {
                if manualScroll != nil, player.currentUtterance != nil {
                    Button("回到当前歌词") {
                        manualScroll = nil
                        if let id = player.currentUtterance {
                            withAnimation(reduceMotion ? nil : .spring(duration: 0.55)) { proxy.scrollTo(id, anchor: Self.anchor) }
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .padding(.bottom, 24)
                }
            }
            .onChange(of: player.track?.id) { _, _ in
                manualScroll = nil
                if let id = player.currentUtterance { proxy.scrollTo(id, anchor: Self.anchor) }
            }
            .task(id: "\(app.lyricsDisplay.fontSize)|\(app.lyricsDisplay.lineGap)|\(app.showFurigana)") {
                await Task.yield()
                guard manualScroll == nil, let id = player.currentUtterance else { return }
                proxy.scrollTo(id, anchor: Self.anchor)
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

    private func lookUp(_ line: LyricLine, token index: Int) {
        let token = line.tokens[index]
        lookup = LookupRequest(text: LyricSegments.lookupText(line.tokens, from: index), songId: app.player.track?.id,
                               utteranceId: line.utteranceId, tokenIndex: index,
                               lemma: token.lemma.isEmpty ? token.surface : token.lemma)
    }

    private func stageLine(_ line: LyricLine, index: Int, active: Int?) -> some View {
        let current = index == active
        let distance = min(5, abs(index - (active ?? 0)))
        let units = app.karaokeLyrics ? app.player.timingUnits(for: line) : nil
        return Group {
            if let units {
                TimedLyricLine(text: line.text, units: units, live: current,
                               frozen: index < (active ?? 0) ? .greatestFiniteMagnitude : -1,
                               size: 34, highlight: .white, rubies: app.furigana[line.utteranceId] ?? []) { unit in
                    // 和普通行一样：点到的是词就查词、不动进度；标点之类查不了的才从这个字放
                    if let token = LyricSegments.tokenIndex(atChar: unit.charStart, text: line.text, tokens: line.tokens),
                       LyricSegments.lookupCandidates(line.tokens).contains(token) {
                        lookUp(line, token: token)
                    } else {
                        Task { await app.player.playUnit(unit, in: line) }
                    }
                }
            } else {
                // 点词查词（右边打开查词栏）；点行里别的地方从这一句放
                FuriganaText(text: line.text, rubies: app.furigana[line.utteranceId] ?? [], size: 34, weight: .bold,
                             tokens: line.tokens, onToken: { lookUp(line, token: $0) })
            }
        }
            .foregroundStyle(.white)
            .opacity(current ? 1 : max(0.22, 0.65 - Double(distance) * 0.09))
            .scaleEffect(current ? 1 : 0.94, anchor: .leading)
            .animation(reduceMotion ? nil : .spring(duration: 0.4), value: current)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture {
                if line.timeSec != nil { manualScroll = nil }
                Task { await app.player.playLine(line) }
            }
            .help(line.timeSec.map { "点词查词，点空白处从 \(formatTime($0)) 放" } ?? "点词查词（这一行没有时间戳）")
            .contextMenu {
                Button("从这里放") { Task { await app.player.playLine(line) } }.disabled(line.timeSec == nil)
                Button(app.player.loop?.utteranceId == line.utteranceId ? "取消循环" : "循环这一句") {
                    Task { await app.player.toggleLoop(line.utteranceId) }
                }.disabled(app.player.spans[line.utteranceId] == nil)
                Button("选择原文 / 查选中文本") { selectionLine = line }
                let candidates = LyricSegments.lookupCandidates(line.tokens)
                if !candidates.isEmpty {
                    Divider()
                    ForEach(candidates, id: \.self) { index in
                        Button("查词「\(line.tokens[index].surface)」") { lookUp(line, token: index) }
                    }
                }
                let lemmas = Array(Set(line.tokens.filter { ["NOUN", "PROPN", "VERB", "ADJ", "ADV"].contains($0.pos) }.map(\.lemma))).sorted()
                if !lemmas.isEmpty {
                    Divider()
                    ForEach(lemmas, id: \.self) { lemma in
                        Button("检索「\(lemma)」") { app.search(lemma) }
                    }
                }
            }
    }

    /// 卡拉 OK 的底（Windows 的 .ktv-bg）：封面放大虚化、压暗铺满，下半截再渐暗一层；清楚的封面在 karaokeLyrics 里
    private var karaokeBackdrop: some View {
        ZStack {
            Color(red: 8 / 255, green: 8 / 255, blue: 11 / 255)
            if let blurredCover {
                Image(nsImage: blurredCover).resizable().scaledToFill()
                    .overlay(Color.black.opacity(0.45))
            } else {
                colors.backdrop.opacity(0.7)
            }
            LinearGradient(stops: [
                .init(color: .clear, location: 0.45),
                .init(color: Color(red: 8 / 255, green: 8 / 255, blue: 11 / 255).opacity(0.55), location: 1),
            ], startPoint: .top, endPoint: .bottom)
        }
        .clipped()
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    /// 两行 KTV（上行靠左、下行靠右，唱完一行换下下行），入句前倒数，前奏显示歌名和词曲。
    /// 30 帧足够让扫亮看着连贯；每行的起止是换歌词时算好的（player.ktvLines），每帧只做一次查找
    private func karaokeLyrics(width: CGFloat) -> some View {
        // 窗口够宽时是样式里定的字号，窄了按比例缩（和 Windows 的 clamp(20px, 0.075×字号 vw, 字号) 一样）
        let target = app.karaokeStyle.fontSize
        let size = min(target, max(20, width * target * 0.00075))
        return TimelineView(.animation(minimumInterval: 1.0 / 30, paused: !app.player.isPlaying)) { _ in
            let player = app.player
            let sec = player.timingPosition
            let times = player.ktvLines
            let layout = Karaoke.ktvLayout(times, sec)
            GeometryReader { geo in
                ZStack {
                    if let cover {
                        // 清楚的封面摆在中间偏上；前奏时退到后面，歌名才读得清
                        Image(nsImage: cover).resizable().aspectRatio(contentMode: .fit)
                            .frame(maxWidth: geo.size.width * 0.62, maxHeight: geo.size.height * 0.5)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .shadow(color: .black.opacity(0.85), radius: 40, y: 24)
                            .opacity(layout.intro ? 0.18 : 1)
                            .animation(.easeInOut(duration: 0.3), value: layout.intro)
                            .position(x: geo.size.width / 2, y: geo.size.height * 0.36)
                    }
                    if layout.intro, let track = player.track {
                        karaokeIntro(track)
                    }
                    if times.allSatisfy({ $0.start == nil }) {
                        VStack(spacing: 12) {
                            Text(player.lines.isEmpty ? "这首歌还没有歌词" : "这首歌的歌词没有时间轴，卡拉 OK 跟不上")
                            if let track = player.track, !player.lines.isEmpty { LyricsToolsButton(songId: track.id, timing: true) }
                        }
                        .font(.title3).foregroundStyle(.white)
                        .shadow(color: .black.opacity(0.85), radius: 3, y: 1)
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        Spacer(minLength: 0)
                        ForEach(0..<2, id: \.self) { slot in
                            karaokeRow(slot: slot, layout: layout, sec: sec, size: size)
                        }
                    }
                    .padding(.horizontal, geo.size.width * 0.06)
                    .padding(.bottom, 20)
                }
                .frame(width: geo.size.width, height: geo.size.height)
            }
        }
    }

    private func karaokeRow(slot: Int, layout: KtvLayout, sec: Double, size: CGFloat) -> some View {
        HStack(alignment: .lastTextBaseline, spacing: size * 0.3) {
            if slot == 1 || !app.karaokeStyle.staggered { Spacer(minLength: 0) }
            if let index = layout.slots[slot], app.player.lines.indices.contains(index) {
                if layout.active == index && layout.countdown > 0 {
                    Text(String(repeating: "●", count: layout.countdown))
                        .font(.system(size: size * 0.5))
                        .foregroundStyle(Color(hex: app.karaokeStyle.sungFill))
                        .accessibilityLabel("倒数 \(layout.countdown)")
                }
                let line = app.player.lines[index]
                KaraokeLyricText(line: line, units: app.player.timingUnits(for: line), sec: sec, size: size)
                    .opacity(index == layout.active ? 1 : 0.82)
                    .id(line.utteranceId)
            }
            if slot == 0 || !app.karaokeStyle.staggered { Spacer(minLength: 0) }
        }
        .frame(minHeight: size * 1.5 * (app.showFurigana ? 1.4 : 1), alignment: .bottom)
        // 不裁：描边、发光、立体阴影要画到行框外面，裁了会切出硬边
    }

    private func karaokeIntro(_ track: Track) -> some View {
        VStack(spacing: 8) {
            Text(track.title).font(.system(size: 52, weight: .bold)).multilineTextAlignment(.center)
            Text(track.artist).font(.system(size: 22)).padding(.bottom, 10)
            ForEach(["lyricist", "composer", "arranger"], id: \.self) { role in
                let names = credits.filter { $0.role == role }.map(\.name)
                if !names.isEmpty {
                    (Text(roleLabels[role] ?? role).foregroundColor(.white.opacity(0.7)) + Text("  " + names.joined(separator: " / ")))
                        .font(.system(size: 15))
                }
            }
        }
        .foregroundStyle(.white)
        .shadow(color: .black.opacity(0.85), radius: 6, y: 2)
        .transition(.opacity)
    }

    private func close() { app.showStage = false }
}

/// 全屏的底：预先虚化的封面 + 三团慢慢漂的颜色。
///
/// 以前这里每秒 20 帧重画，三团颜色各挂一个半径 80 的模糊、封面再挂一个可调半径的模糊，暂停时也照画：
/// 全屏下每一帧都要把整屏模糊几遍。现在封面是算好的静态图；色块本身就是径向渐变（边缘已经是虚的），
/// 不再叠模糊；只在播放时漂，每秒 10 帧（移动很慢，看不出差别）
@MainActor
private struct StageBackdrop: View {
    let blurredCover: NSImage?
    let colors: ArtworkColors
    let drifting: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    var body: some View {
        TimelineView(.animation(minimumInterval: 1.0 / 10, paused: reduceMotion || !drifting)) { timeline in
            let phase = reduceMotion ? 0 : timeline.date.timeIntervalSinceReferenceDate / 14
            GeometryReader { geo in
                ZStack {
                    colors.backdrop
                    if let blurredCover {
                        Image(nsImage: blurredCover).resizable().scaledToFill()
                            .frame(width: geo.size.width, height: geo.size.height)
                            .opacity(0.6)
                    }
                    ForEach(0..<3, id: \.self) { index in
                        Circle().fill(RadialGradient(colors: [index == 1 ? colors.secondary : colors.highlight, .clear],
                                                     center: .center, startRadius: 0, endRadius: geo.size.width * 0.4))
                            .frame(width: geo.size.width * 0.8, height: geo.size.width * 0.8)
                            .opacity(0.3)
                            .position(x: geo.size.width * CGFloat(0.5 + 0.35 * sin(phase + Double(index) * 2)),
                                      y: geo.size.height * CGFloat(0.5 + 0.35 * cos(phase * 0.8 + Double(index) * 2)))
                    }
                    Color.black.opacity(0.28)
                }.frame(width: geo.size.width, height: geo.size.height).clipped()
            }
        }.allowsHitTesting(false).accessibilityHidden(true)
    }
}

@MainActor
/// 用户自己滚了（滚轮 / 触控板）：歌词跟随暂停，直到点「回到当前歌词」、点一行或换歌
struct StageScrollObserver: NSViewRepresentable {
    let scrolled: () -> Void
    func makeCoordinator() -> Coordinator { Coordinator(scrolled) }
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        context.coordinator.view = view
        context.coordinator.install()
        return view
    }
    func updateNSView(_ view: NSView, context: Context) { context.coordinator.scrolled = scrolled }
    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) { coordinator.remove() }
    @MainActor
    final class Coordinator {
        weak var view: NSView?
        var scrolled: () -> Void
        var monitor: Any?
        init(_ scrolled: @escaping () -> Void) { self.scrolled = scrolled }
        func install() {
            monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
                MainActor.assumeIsolated {
                    if let self, let view = self.view, event.window === view.window,
                       view.bounds.contains(view.convert(event.locationInWindow, from: nil)) { self.scrolled() }
                    return event
                }
            }
        }
        func remove() { if let monitor { NSEvent.removeMonitor(monitor) }; monitor = nil }
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
