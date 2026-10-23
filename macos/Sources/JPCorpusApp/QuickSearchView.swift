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
    @FocusState private var focused: Bool

    private var groups: [(String, [QuickHit])] {
        guard let r = results else { return [] }
        return [("歌曲", r.tracks), ("歌手和词曲作者", r.people), ("专辑", r.albums), ("词", r.words), ("歌词", r.lyrics)]
            .filter { !$0.1.isEmpty }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                TextField("搜歌、专辑、歌手、词、歌词", text: $query)
                    .textFieldStyle(.plain)
                    .font(.title3)
                    .focused($focused)
                    .onSubmit {
                        if let first = groups.first?.1.first { pick(first) }
                    }
            }
            .padding(14)
            Divider()
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2, pinnedViews: []) {
                    ForEach(groups, id: \.0) { title, hits in
                        Text(title)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 14)
                            .padding(.top, 10)
                        ForEach(hits) { hit in
                            Button { pick(hit) } label: { row(hit) }
                                .buttonStyle(QuickRowStyle())
                        }
                    }
                    if results?.isEmpty == true {
                        Text("没有找到「\(query)」")
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity)
                            .padding(30)
                    }
                }
                .padding(.vertical, 6)
            }
            .frame(height: 380)
        }
        .frame(width: 560)
        .background(GlassBackground(material: .popover))
        .onAppear {
            focused = true
            if !app.quickSearchSeed.isEmpty {
                query = app.quickSearchSeed
                app.quickSearchSeed = ""
            }
        }
        .onExitCommand { dismiss() }
        .task(id: query) {
            let q = query.trimmingCharacters(in: .whitespaces)
            guard !q.isEmpty else {
                results = nil
                return
            }
            // 打字时别每个键都查
            try? await Task.sleep(nanoseconds: 150_000_000)
            guard !Task.isCancelled else { return }
            results = try? await app.core?.quickSearch(q)
        }
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
