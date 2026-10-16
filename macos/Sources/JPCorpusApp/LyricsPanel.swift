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
    @State private var lookupText: String?

    private static let contentPOS: Set<String> = ["NOUN", "PROPN", "VERB", "ADJ", "ADV"]

    var body: some View {
        let player = app.player
        VStack(alignment: .leading, spacing: 0) {
            if let track = player.track {
                header(track)
                Divider()
                ScrollViewReader { proxy in
                    List {
                        ForEach(player.lines) { line in
                            row(line, current: line.utteranceId == player.currentUtterance)
                                .id(line.utteranceId)
                        }
                    }
                    .listStyle(.plain)
                    .onChange(of: player.currentUtterance) { _, id in
                        guard let id, player.isPlaying else { return }
                        withAnimation(.easeInOut(duration: 0.25)) { proxy.scrollTo(id, anchor: .center) }
                    }
                    .onChange(of: player.focusUtterance) { _, id in
                        guard let id else { return }
                        withAnimation { proxy.scrollTo(id, anchor: .center) }
                    }
                    .onAppear {
                        if let id = player.focusUtterance ?? player.currentUtterance { proxy.scrollTo(id, anchor: .center) }
                    }
                }
            } else {
                ContentUnavailableView("还没有在放的歌", systemImage: "music.note", description: Text("在检索结果里按空格试听，或在曲库里双击一首歌。"))
            }
            if let text = lookupText {
                Divider()
                LookupPane(text: text) { lookupText = nil }
                    .frame(maxHeight: 260)
            }
            if let error = player.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .padding(10)
                    .textSelection(.enabled)
            }
        }
    }

    private func header(_ track: Track) -> some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 2) {
                Text(track.title).font(.headline).lineLimit(2)
                Text([track.artist, track.year].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            Spacer()
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
        .padding(12)
    }

    private func row(_ line: LyricLine, current: Bool) -> some View {
        let player = app.player
        let looping = player.loop?.utteranceId == line.utteranceId
        let focused = player.focusUtterance == line.utteranceId
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(line.timeSec.map(formatTime) ?? "")
                .font(.caption2.monospacedDigit())
                .foregroundStyle(.tertiary)
                .frame(width: 34, alignment: .trailing)
            Text(line.text)
                .font(current ? .body.weight(.semibold) : .body)
                .foregroundStyle(current ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
            if looping {
                Image(systemName: "repeat").font(.caption).foregroundStyle(.tint)
            }
        }
        .padding(.vertical, 3)
        .contentShape(Rectangle())
        .listRowBackground(focused ? Color.accentColor.opacity(0.12) : Color.clear)
        .onTapGesture { Task { await player.playLine(line) } }
        .contextMenu {
            Button("从这里放") { Task { await player.playLine(line) } }
                .disabled(line.timeSec == nil)
            Button(looping ? "取消循环" : "循环这一句") { Task { await player.toggleLoop(line.utteranceId) } }
                .disabled(line.timeSec == nil)
            let words = line.tokens.filter { Self.contentPOS.contains($0.pos) }
            if !words.isEmpty {
                Divider()
                ForEach(Array(Set(words.map(\.lemma))).sorted(), id: \.self) { lemma in
                    Button("检索「\(lemma)」") { app.search(lemma) }
                }
                Divider()
                ForEach(Array(line.tokens.enumerated()).filter { Self.contentPOS.contains($0.element.pos) }, id: \.offset) { index, token in
                    Button("查词「\(token.surface)」") {
                        lookupText = line.tokens[index...].map(\.surface).joined()
                    }
                }
            }
        }
    }
}

/// 查词结果（语料库目录里的 Yomitan 词典库 dictionaries.db，和 Windows 版同一份、同一套查法）
@MainActor
private struct LookupPane: View {
    @Environment(AppModel.self) private var app
    let text: String
    let close: () -> Void
    @State private var result: LookupText?
    @State private var noDictionaries = false
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("查词").font(.headline)
                if let result, result.originalTextLength > 0 {
                    Text(String(text.prefix(result.originalTextLength))).foregroundStyle(.secondary)
                }
                Spacer()
                Button(action: close) { Image(systemName: "xmark.circle.fill") }
                    .buttonStyle(.borderless)
                    .foregroundStyle(.secondary)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    if noDictionaries {
                        Text("这个语料库目录里还没有词典（dictionaries.db）。在 Windows 版里导入 Yomitan 词典后，把 dictionaries.db 和 corpus.db 一起拷过来就能查。")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    } else if let error {
                        Text(error).font(.callout).foregroundStyle(.orange)
                    } else if let result {
                        if result.entries.isEmpty {
                            Text("没查到").foregroundStyle(.secondary)
                        }
                        ForEach(Array(result.entries.prefix(8).enumerated()), id: \.offset) { _, entry in
                            entryView(entry)
                        }
                    } else {
                        ProgressView().controlSize(.small)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(12)
        .task(id: text) { await load() }
    }

    private func entryView(_ entry: LookupEntry) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                ForEach(Array(entry.headwords.enumerated()), id: \.offset) { _, h in
                    Text(h.term).font(.title3.weight(.semibold))
                    if !h.reading.isEmpty && h.reading != h.term {
                        Text(h.reading).foregroundStyle(.secondary)
                    }
                }
                Spacer()
                if let first = entry.headwords.first {
                    Button("检索") { app.search(first.term) }
                        .buttonStyle(.borderless)
                        .font(.caption)
                }
            }
            ForEach(Array(entry.definitions.enumerated()), id: \.offset) { _, d in
                VStack(alignment: .leading, spacing: 2) {
                    Text(d.dictionary).font(.caption2).foregroundStyle(.tertiary)
                    ForEach(Array(d.text.enumerated()), id: \.offset) { _, line in
                        Text(line).font(.callout).textSelection(.enabled)
                    }
                }
            }
        }
    }

    private func load() async {
        guard let client = app.client else { return }
        result = nil
        error = nil
        do {
            noDictionaries = try await client.dictionaries().isEmpty
            if !noDictionaries {
                result = try await client.lookup(text)
            }
        } catch {
            self.error = error.localizedDescription
        }
    }
}

/// 一首歌的语料画像：重复行、特征词。特征词点了去检索
@MainActor
private struct ProfileView: View {
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

/// 底部播放条
@MainActor
struct PlayerBar: View {
    @Environment(AppModel.self) private var app
    @State private var scrubbing: Double?

    var body: some View {
        let player = app.player
        HStack(spacing: 14) {
            HStack(spacing: 6) {
                Button { Task { await player.step(-1) } } label: { Image(systemName: "backward.end.fill") }
                    .help("上一行")
                Button { player.toggle() } label: {
                    Image(systemName: player.isPlaying ? "pause.fill" : "play.fill").frame(width: 18)
                }
                .help("播放 / 暂停（⌥空格）")
                Button { Task { await player.step(1) } } label: { Image(systemName: "forward.end.fill") }
                    .help("下一行")
            }
            .buttonStyle(.borderless)
            .font(.title3)
            .disabled(player.track == nil)

            VStack(alignment: .leading, spacing: 2) {
                Text(player.track?.title ?? "—").font(.callout.weight(.medium)).lineLimit(1)
                Text(player.track?.artist ?? "").font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            .frame(width: 180, alignment: .leading)

            Text(formatTime(scrubbing ?? player.position)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
            Slider(
                value: Binding(get: { scrubbing ?? player.position }, set: { scrubbing = $0 }),
                in: 0...max(player.duration, 1),
                onEditingChanged: { editing in
                    if !editing, let target = scrubbing {
                        Task {
                            await player.seek(to: target)
                            scrubbing = nil
                        }
                    }
                }
            )
            .disabled(player.track == nil)
            Text(formatTime(player.duration)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)

            Button {
                Task { await player.toggleLoop(nil) }
            } label: {
                Image(systemName: "repeat.1")
            }
            .buttonStyle(.borderless)
            .foregroundStyle(player.loop != nil ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
            .help(player.loop.map { "单句循环 \(formatTime($0.startSec))–\(formatTime($0.endSec))，点击取消" } ?? "循环当前这一行（⌘L）")
            .disabled(player.track == nil)

            Menu {
                ForEach([0.5, 0.75, 0.9, 1.0, 1.25], id: \.self) { r in
                    Button("\(String(format: "%g", r))×") { app.player.rate = Float(r) }
                }
            } label: {
                Text("\(String(format: "%g", player.rate))×").monospacedDigit()
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .help("倍速（音高不变）")
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background(.bar)
        .overlay(alignment: .top) { Divider() }
    }
}
