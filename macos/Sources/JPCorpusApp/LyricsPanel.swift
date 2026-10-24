import JPCorpusModel
import AppKit
import JPCore
import SwiftUI

/// 右侧检查器：当前这首的歌词，跟着播放高亮、自动滚动。
///
/// 从播放反向进入语料：右键一行可以检索行里的任何一个实词、循环这一句；
/// 顶上「语料画像」看这首歌的重复行比例和它相对其余歌的特征词。
@MainActor
struct LyricsPanel: View {
    @Environment(AppModel.self) private var app
    @State private var showProfile = false
    /// 正在查的那段文字（从点的词开始到行尾，Yomitan 式从开头查）
    @State private var lookup: LookupRequest?
    /// 右键「选择原文」打开的那一行
    @State private var correcting: Int64?
    @State private var selectionLine: LyricLine?

    private static let contentPOS: Set<String> = ["NOUN", "PROPN", "VERB", "ADJ", "ADV"]

    /// 当前这首的封面（磁盘上的文件，库从别的机器拷来时按目录尾巴找）
    @State private var cover: NSImage?

    var body: some View {
        let player = app.player
        VStack(alignment: .leading, spacing: 0) {
            if let track = player.track {
                header(track)
                ScrollView(.horizontal) {
                    HStack(spacing: 4) {
                        LyricsToolsButton(songId: track.id, iconOnly: true)
                        FuriganaMenu()
                        LyricsDisplayButton()
                        Button { app.stageKaraoke = true; app.showStage = true } label: { Image(systemName: "music.mic") }
                            .help("打开卡拉 OK").accessibilityLabel("打开卡拉 OK")
                    }
                    .buttonStyle(PlayerGlyphStyle()).font(.system(size: 15, weight: .semibold))
                }
                .scrollIndicators(.never)
                .frame(height: 34)
                .padding(.horizontal, 14)
                .padding(.bottom, 6)
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: app.lyricsDisplay.lineGap) {
                            ForEach(player.lines) { line in
                                row(line, current: line.utteranceId == player.currentUtterance)
                                    .id(line.utteranceId)
                            }
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 40)
                    }
                    .scrollIndicators(.never)
                    .mask(
                        // 上下两头渐隐，像 Apple Music 的歌词
                        LinearGradient(stops: [
                            .init(color: .clear, location: 0),
                            .init(color: .black, location: 0.08),
                            .init(color: .black, location: 0.9),
                            .init(color: .clear, location: 1),
                        ], startPoint: .top, endPoint: .bottom)
                    )
                    .onChange(of: player.currentUtterance) { _, id in
                        guard let id, player.isPlaying else { return }
                        withAnimation(.spring(duration: 0.45)) { proxy.scrollTo(id, anchor: UnitPoint(x: 0, y: 0.35)) }
                    }
                    .onChange(of: player.focusUtterance) { _, id in
                        guard let id else { return }
                        withAnimation { proxy.scrollTo(id, anchor: UnitPoint(x: 0, y: 0.35)) }
                    }
                    .onAppear {
                        if let id = player.focusUtterance ?? player.currentUtterance {
                            proxy.scrollTo(id, anchor: UnitPoint(x: 0, y: 0.35))
                        }
                    }
                }
            } else {
                ContentUnavailableView("还没有在放的歌", systemImage: "music.note", description: Text("在检索结果里按空格试听，或在曲库里双击一首歌。"))
            }
            if let lookup {
                LookupPane(request: lookup) { self.lookup = nil }
                    .frame(maxHeight: 260)
                    .glassCard(cornerRadius: 12)
                    .padding(10)
            }
            if app.showFurigana, let error = app.furiganaError {
                Text("振假名：\(error)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 14)
                    .padding(.bottom, 6)
            }
            if let error = player.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .padding(10)
                    .textSelection(.enabled)
            }
        }
        .background { backdrop }
        .sheet(isPresented: Binding(get: { correcting != nil }, set: { if !$0 { correcting = nil } })) {
            if let id = correcting { TokenCorrectionView(utteranceId: id) }
        }
        .popover(item: $selectionLine) { line in
            LyricSelectionView(text: line.text) { selected in
                lookup = LookupRequest(text: selected, songId: player.track?.id, utteranceId: line.utteranceId, tokenIndex: nil)
                selectionLine = nil
            }
        }
        .task(id: player.track?.id) {
            cover = nil
            guard let track = player.track, !track.coverPath.isEmpty, let client = app.client else { return }
            let path = try? await client.resolveMedia(track.coverPath)
            cover = loadImage(atPath: path ?? nil)
        }
    }

    /// 底：毛玻璃，有封面就再铺一层大幅虚化的封面，整栏带上这首歌的颜色
    private var backdrop: some View {
        ZStack {
            GlassBackground(material: .sidebar)
            if let cover {
                Image(nsImage: cover)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .blur(radius: 70, opaque: true)
                    .saturation(1.4)
                    .opacity(0.55)
                    .clipped()
                    .transition(.opacity)
            }
        }
        .ignoresSafeArea()
        .animation(.easeInOut(duration: 0.6), value: cover != nil)
    }

    private func header(_ track: Track) -> some View {
        HStack(alignment: .center, spacing: 12) {
            Group {
                if let cover {
                    Image(nsImage: cover).resizable().aspectRatio(contentMode: .fill)
                } else {
                    ZStack {
                        Rectangle().fill(.quaternary)
                        Image(systemName: "music.note").foregroundStyle(.secondary)
                    }
                }
            }
            .frame(width: 52, height: 52)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .shadow(color: .black.opacity(0.2), radius: 6, y: 2)
            VStack(alignment: .leading, spacing: 2) {
                Text(track.title).font(.headline).lineLimit(2)
                Text([track.artist, track.year].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            Button {
                app.stageKaraoke = false
                app.showStage = true
            } label: {
                Image(systemName: "arrow.up.left.and.arrow.down.right")
            }
            .buttonStyle(.borderless)
            .help("全屏歌词（⇧⌘F）")
            Button {
                showProfile.toggle()
            } label: {
                Image(systemName: "chart.bar.doc.horizontal")
            }
            .buttonStyle(.borderless)
            .help("这首歌的语料画像")
            .popover(isPresented: $showProfile, arrowEdge: .bottom) {
                ProfileView(songId: track.id)
            }
        }
        .padding(.horizontal, 14)
        .padding(.top, 14)
        .padding(.bottom, 6)
    }

    private func row(_ line: LyricLine, current: Bool) -> some View {
        let player = app.player
        let looping = player.loop?.utteranceId == line.utteranceId
        let focused = player.focusUtterance == line.utteranceId
        let units = player.timingUnits(for: line)
        let mode = player.displayMode(for: line)
        let lineTime = line.timeSec.flatMap { $0.isFinite && $0 >= 0 ? $0 : nil }
        let hint = units != nil ? "真实逐字时间 · 点单位从词首放，右键选择原文 / 查词 / 循环"
            : lineTime.map { formatTime($0) + " · 点一下从这里放，右键检索 / 查词 / 循环" } ?? "纯文本 · 右键选择原文 / 查词"
        return HStack(alignment: .firstTextBaseline, spacing: 6) {
            if app.karaokeLyrics, let units {
                TimedLyricText(text: line.text, units: units, position: player.position) { unit in
                    Task { await player.playUnit(unit, in: line) }
                }
                .opacity(current ? 1 : 0.55)
            } else {
                FuriganaText(text: line.text, rubies: app.furigana[line.utteranceId] ?? [])
                    .foregroundStyle(current && mode == .lineSynced ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
                    .opacity(mode == .plain ? 0.65 : (current ? 1 : 0.5))
                    .scaleEffect(current ? 1.0 : 0.97, anchor: .leading)
                    .fixedSize(horizontal: false, vertical: true)
                    .onTapGesture { if mode != .plain { Task { await player.playLine(line) } } }
            }
            if looping {
                Image(systemName: "repeat").font(.callout.weight(.semibold)).foregroundStyle(.tint)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 6)
        .padding(.horizontal, 8)
        .background(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(focused ? Color.accentColor.opacity(0.14) : .clear)
        )
        .animation(.easeInOut(duration: 0.25), value: current)
        .contentShape(Rectangle())
        .help(hint)
        .contextMenu {
            Button("从这里放") { Task { await player.playLine(line) } }
                .disabled(lineTime == nil)
            Button(looping ? "取消循环" : "循环这一句") { Task { await player.toggleLoop(line.utteranceId) } }
                .disabled(lineTime == nil)
            Button("分词校正…") { correcting = line.utteranceId }
            Button("选择原文 / 查选中文本") { selectionLine = line }
            let words = line.tokens.filter { Self.contentPOS.contains($0.pos) }
            if !words.isEmpty {
                Divider()
                ForEach(Array(Set(words.map(\.lemma))).sorted(), id: \.self) { lemma in
                    Button("检索「\(lemma)」") { app.search(lemma) }
                }
                Divider()
                ForEach(Array(line.tokens.enumerated()).filter { Self.contentPOS.contains($0.element.pos) }, id: \.offset) { index, token in
                    Button("查词「\(token.surface)」") {
                        lookup = LookupRequest(
                            text: line.tokens[index...].map(\.surface).joined(),
                            songId: player.track?.id,
                            utteranceId: line.utteranceId,
                            tokenIndex: index
                        )
                    }
                }
            }
        }
    }
}

/// 一首歌的语料画像：重复行、特征词。特征词点了去检索
@MainActor
struct ProfileView: View {
    @Environment(AppModel.self) private var app
    let songId: String
    @State private var profile: SongProfile?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let p = profile {
                Text("语料画像").font(.headline)
                Text("\(p.lines) 行，其中 \(p.repeatedLines) 行是重复（副歌等）；\(p.tokens) 词，去掉重复行后 \(p.distinctTokens) 词。")
                    .font(.callout)
                Text("相对其余歌的特征词").font(.subheadline.weight(.medium)).padding(.top, 4)
                ForEach(p.keywords.prefix(10)) { k in
                    Button {
                        app.search(k.lemma)
                    } label: {
                        HStack {
                            Text(k.lemma)
                            Text(posLabels[k.pos] ?? k.pos).font(.caption).foregroundStyle(.secondary)
                            Spacer()
                            Text("\(k.targetFreq) 次 · LL \(String(format: "%.1f", k.logLikelihood))")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                    }
                    .buttonStyle(.plain)
                }
                Text("指纹 \(String(p.provenance.snapshot.fingerprint.prefix(8))) · \(p.provenance.rules)")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            } else {
                ProgressView()
            }
        }
        .padding(14)
        .frame(width: 320)
        .task {
            profile = try? await app.client?.songProfile(songId)
        }
    }
}

/// 振假名：开关 + 注法（只注汉字 / 整词注音）。歌词面板和全屏歌词各放一个，状态是同一份
@MainActor
struct FuriganaMenu: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        Menu {
            Toggle("显示振假名", isOn: Binding(get: { app.showFurigana }, set: { app.showFurigana = $0 }))
            Divider()
            Picker("注法", selection: Binding(get: { app.furiganaMode }, set: { app.furiganaMode = $0; app.showFurigana = true })) {
                Text("只注汉字").tag(FuriganaMode.kanji)
                Text("整词注音").tag(FuriganaMode.word)
            }
            .pickerStyle(.inline)
        } label: {
            Image(systemName: app.showFurigana ? "character.bubble.fill" : "character.bubble")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(.primary.opacity(app.showFurigana ? 1 : 0.6))
                .modifier(PlayerGlyphSurface())
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .buttonStyle(.plain)
        .fixedSize()
        .accessibilityLabel(app.showFurigana ? "振假名：已开启" : "振假名：已关闭")
        .help("振假名（⇧⌘R）。读音来自分词词典，和 Windows 版同一套规则")
    }
}
