import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct LyricsLibraryView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var targets: [TimedLyricsTarget] = []
    @State private var missing: [MissingLyrics] = []
    @State private var selected: String?
    @State private var open = false
    @State private var loading = false
    @State private var error: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("歌词管理").font(.title2.bold())
                Spacer()
                Button("刷新名单") { Task { await refresh() } }.disabled(loading)
                Button("关闭") { dismiss() }
            }
            Button("来源设置与全库后台任务") { selected = nil; open = true }
            Text("缺失歌词 \(missing.count) 首；以下歌曲缺歌词或逐字时间不完整，可逐首预览更换。").font(.caption)
            if let error { Text(error).foregroundStyle(.orange) }
            List(targets) { song in
                HStack {
                    VStack(alignment: .leading) {
                        Text("\(song.title) — \(song.artist)")
                        Text("现有 \(song.lyricLines) 行，真实逐字 \(song.timedLines) 行").font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("预览与更换") { selected = song.songId; open = true }
                }
            }
            if targets.isEmpty && !loading { Text("没有待补逐字歌词的歌曲").foregroundStyle(.secondary) }
        }
        .padding(20).frame(width: 760, height: 560)
        .task { await refresh() }
        .sheet(isPresented: $open, onDismiss: { Task { await refresh() } }) {
            LyricsManagementView(songId: selected ?? "", initialTab: 2).id(selected ?? "global")
        }
    }
    private func refresh() async {
        guard !loading, let client = app.core else { return }
        loading = true
        defer { loading = false }
        do { targets = try await client.lyricsTimedTargets(); missing = try await client.lyricsMissing() }
        catch { self.error = error.localizedDescription }
    }
}
