import AppKit
import JPCore
import SwiftUI
import JPCorpusModel

/// ⌘K 全库搜索：歌、专辑、人、词、歌词一起搜（Windows 版的 Ctrl K 面板，同一个 quickSearch）。
/// 回车选第一条；每种结果落到它该去的地方
@MainActor
struct QuickSearchView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var results: QuickSearchResults?
    @State private var active = 0
    @State private var busy = false
    @FocusState private var focused: Bool
    private var commandMode: Bool { query.hasPrefix(">") }
    private var needle: String { (commandMode ? String(query.dropFirst()) : query).trimmingCharacters(in: .whitespacesAndNewlines) }
    private var hits: [QuickHit] { groups.flatMap(\.1) }
    private var visibleCommands: [PaletteAction] { commands.filter { needle.isEmpty || $0.label.localizedCaseInsensitiveContains(needle) || $0.hint.localizedCaseInsensitiveContains(needle) } }
    private var commands: [PaletteAction] {
        let player = app.player
        return [
            PaletteAction(id: "play",label: player.isPlaying ? "暂停" : "播放",hint: "Space",disabled: player.canPlay ? nil : "没有音频设备",run: { player.toggle() }),
            PaletteAction(id: "loop-off",label: "取消单句循环",disabled: player.loop == nil ? "当前没有循环" : nil,run: { Task { await player.toggleLoop(nil) } }),
            PaletteAction(id: "fullscreen",label: "全屏 / 退出全屏",hint: "F11",run: { let key = NSApplication.shared.keyWindow; (key?.sheetParent ?? key)?.toggleFullScreen(nil) }),
            PaletteAction(id: "go-home",label: "打开 首页",hint: "继续听 · 今日 · 语料库",run: { app.section = .home }),
            PaletteAction(id: "go-kwic",label: "打开 检索",hint: "KWIC",run: { app.section = .search }),
            PaletteAction(id: "go-library",label: "打开 资料库",hint: "音乐人 · 专辑 · 歌曲",run: { app.section = .library }),
            PaletteAction(id: "go-analytics",label: "打开 分析",run: { app.section = .analysis }),
            PaletteAction(id: "go-import",label: "打开 导入",hint: "扫描目录并写入语料",run: { app.section = .importer }),
            PaletteAction(id: "go-scrape",label: "打开 刮削",hint: "识别 · metadata · 封面",run: { app.section = .scrape }),
            PaletteAction(id: "go-anki",label: "打开 Anki",hint: "用语料例句做卡片",disabled: "Anki 浏览将在第 5 阶段补齐",run: {}),
            PaletteAction(id: "go-settings",label: "打开 设置",hint: "外观 · 歌词 · 词典 · 制卡",run: { app.section = .settings }),
            PaletteAction(id: "search",label: "全局搜索…",hint: "⌘K",run: { query = ""; focused = true }),
            PaletteAction(id: "backfill",label: "回填时长",hint: "扫描音频文件补 duration_sec",run: { Task {
                guard await app.library.backfillDurations() else { app.errorMessage = app.library.errorMessage; return }
                while app.library.job?.running == true {
                    try? await Task.sleep(nanoseconds: 250_000_000); await app.library.pollJob()
                }
                if let error = app.library.errorMessage { app.errorMessage = error } else { app.notice = app.library.note; await app.reloadTracks() }
            } })
        ]
    }

    private var groups: [(String, [QuickHit])] {
        guard let r = results else { return [] }
        return [("歌曲", r.tracks), ("歌手和词曲作者", r.people), ("专辑", r.albums), ("词", r.words), ("歌词", r.lyrics)]
            .filter { !$0.1.isEmpty }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                TextField(commandMode ? "输入命令…" : "搜歌、专辑、人物、词、歌词（> 切到命令）", text: $query)
                    .textFieldStyle(.plain)
                    .font(.title3)
                    .focused($focused)
                    .onSubmit {
                        execute(active)
                    }
            }
            .padding(14)
            Divider()
            ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2, pinnedViews: []) {
                    if commandMode {
                        ForEach(Array(visibleCommands.enumerated()),id: \.element.id) { i,c in
                            Button { execute(i) } label: {
                                HStack { Text(c.label); Spacer(); Text(c.disabled ?? c.hint).font(.caption).foregroundStyle(.secondary) }.padding(.vertical,4)
                            }.buttonStyle(QuickRowStyle()).disabled(c.disabled != nil)
                                .background(i == active ? Color.accentColor.opacity(0.16) : Color.clear).id(i)
                                .onHover { if $0 { active = i } }
                        }
                        if visibleCommands.isEmpty { Text("没有匹配的命令").padding(20).foregroundStyle(.secondary) }
                    } else {
                        ForEach(Array(hits.enumerated()),id: \.element.id) { i,hit in
                            Button { pick(hit) } label: { row(hit) }
                                .buttonStyle(QuickRowStyle())
                                .background(i == active ? Color.accentColor.opacity(0.16) : Color.clear).id(i)
                                .onHover { if $0 { active = i } }
                        }
                    }
                    if !commandMode && results?.isEmpty == true && !busy {
                        Text("没有找到「\(query)」")
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity)
                            .padding(30)
                    }
                }
                .padding(.vertical, 6)
            }
            .frame(height: 380)
            .onChange(of: active) { _, value in proxy.scrollTo(value,anchor: .center) }
            }
            HStack { Text("↑↓ 选择 · Enter 执行 · Esc 关闭"); Spacer(); if busy { ProgressView().controlSize(.mini) } }.font(.caption).foregroundStyle(.secondary).padding(12)
        }
        .frame(width: 560)
        .background(GlassBackground(material: .popover))
        .onAppear {
            focused = true
            if app.paletteCommands { query = ">"; app.paletteCommands = false }
            if !app.quickSearchSeed.isEmpty {
                query = app.quickSearchSeed
                app.quickSearchSeed = ""
            }
        }
        .onExitCommand { dismiss() }
        .onKeyPress(.downArrow) { move(1); return .handled }
        .onKeyPress(.upArrow) { move(-1); return .handled }
        .onChange(of: query) { _, _ in active = 0 }
        .task(id: query) {
            let q = needle
            guard !q.isEmpty && !commandMode else {
                results = nil
                busy = false
                return
            }
            // 打字时别每个键都查
            busy = true
            try? await Task.sleep(nanoseconds: 140_000_000)
            guard !Task.isCancelled else { return }
            let fresh = try? await app.core?.quickSearch(q)
            guard !Task.isCancelled else { return }
            results = fresh; active = 0; busy = false
        }
    }
    private func move(_ delta: Int) { let count = commandMode ? visibleCommands.count : hits.count; active = count == 0 ? 0 : (active+delta+count)%count }
    private func execute(_ index: Int) {
        if commandMode {
            guard visibleCommands.indices.contains(index) else { return }; let command = visibleCommands[index]
            guard command.disabled == nil else { return }; if command.id != "search" { dismiss() }; command.run()
        } else if hits.indices.contains(index) { pick(hits[index]) }
    }

    @ViewBuilder
    private func row(_ hit: QuickHit) -> some View {
        switch hit {
        case let .track(_, title, artist, album, duration):
            line("music.note", title, [artist, album].filter { !$0.isEmpty }.joined(separator: " · "), duration.map(formatTime))
        case let .album(_, title, artist, count):
            line("square.stack", title, artist, "\(count) 首")
        case let .person(_, name, roles, count):
            line("person", name, roles.map { roleLabels[$0] ?? $0 }.joined(separator: " / "), "\(count) 首")
        case let .word(lemma, pos, freq, songs):
            line("character.book.closed", lemma, posLabels[pos] ?? pos, "\(freq) 次 · \(songs) 首")
        case let .lyric(_, title, artist, _, time, text):
            line("quote.bubble", text, "\(title) · \(artist)", time.map(formatTime))
        }
    }

    private func line(_ symbol: String, _ title: String, _ subtitle: String, _ trailing: String?) -> some View {
        HStack(spacing: 10) {
            Image(systemName: symbol).frame(width: 18).foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).lineLimit(1)
                if !subtitle.isEmpty {
                    Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer()
            if let trailing {
                Text(trailing).font(.caption.monospacedDigit()).foregroundStyle(.tertiary)
            }
        }
    }

    private func pick(_ hit: QuickHit) {
        dismiss()
        switch hit {
        case let .track(songId, _, _, _, _):
            app.show(.song(songId))
        case let .album(albumId, _, _, _):
            Task {
                if let album = try? await app.core?.albums().first(where: { $0.id == albumId }) {
                    app.show(.album(album))
                }
            }
        case let .person(personId, _, _, _):
            app.show(.person(personId))
        case let .word(lemma, _, _, _):
            app.search(lemma)
        case let .lyric(songId, _, _, utteranceId, time, _):
            Task { await app.player.playFrom(songId: songId, utteranceId: utteranceId, timeSec: time) }
            app.showLyrics = true
        }
    }
}

@MainActor
private struct PaletteAction: Identifiable {
    var id: String; var label: String; var hint = ""; var disabled: String?; var run: () -> Void
}

private struct QuickRowStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        QuickRow(configuration: configuration)
    }

    private struct QuickRow: View {
        let configuration: ButtonStyleConfiguration
        @State private var hovering = false

        var body: some View {
            configuration.label
                .padding(.horizontal, 14)
                .padding(.vertical, 6)
                .background(
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(hovering || configuration.isPressed ? AnyShapeStyle(.tint.opacity(0.18)) : AnyShapeStyle(.clear))
                        .padding(.horizontal, 6)
                )
                .contentShape(Rectangle())
                .onHover { hovering = $0 }
        }
    }
}
