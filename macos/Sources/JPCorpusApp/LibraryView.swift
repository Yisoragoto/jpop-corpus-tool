import JPCore
import SwiftUI

/// 曲库：所有歌，双击放、右边歌词跟着走
@MainActor
struct LibraryView: View {
    @Environment(AppModel.self) private var app
    @State private var filter = ""
    @State private var selection: Set<Track.ID> = []
    @State private var sortOrder = [KeyPathComparator(\Track.artist), KeyPathComparator(\Track.title)]

    private var rows: [Track] {
        let needle = filter.trimmingCharacters(in: .whitespaces)
        let base = needle.isEmpty ? app.tracks : app.tracks.filter {
            $0.title.localizedCaseInsensitiveContains(needle)
                || $0.artist.localizedCaseInsensitiveContains(needle)
                || $0.album.localizedCaseInsensitiveContains(needle)
        }
        return base.sorted(using: sortOrder)
    }

    var body: some View {
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("曲名", value: \.title)
            TableColumn("歌手", value: \.artist)
            TableColumn("专辑", value: \.album)
            TableColumn("年份", value: \.year).width(56)
            TableColumn("歌词") { t in
                Text(t.lineCount > 0 ? "\(t.lineCount) 行" : "无").foregroundStyle(.secondary)
            }
            .width(56)
            TableColumn("时长") { t in
                Text(t.durationSec.map(formatTime) ?? "—").monospacedDigit().foregroundStyle(.secondary)
            }
            .width(52)
        }
        .contextMenu(forSelectionType: Track.ID.self) { ids in
            if let id = ids.first, let track = app.tracks.first(where: { $0.id == id }) {
                Button("播放") { play(track) }
            }
        } primaryAction: { ids in
            if let id = ids.first, let track = app.tracks.first(where: { $0.id == id }) { play(track) }
        }
        .searchable(text: $filter, placement: .toolbar, prompt: "曲名、歌手、专辑")
        .navigationTitle("曲库")
        .navigationSubtitle("\(app.tracks.count) 首")
    }

    private func play(_ track: Track) {
        app.showLyrics = true
        Task { await app.player.open(track, at: 0, autoplay: true) }
    }
}
