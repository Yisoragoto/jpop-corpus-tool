import SwiftUI
import JPCore
import JPCorpusModel

@MainActor
struct SongAdminButton: View {
    @Environment(AppModel.self) private var app
    let track: Track
    var onChanged: () -> Void = {}
    @State private var editing = false
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Button { editing = true } label: { Image(systemName: "pencil") }.help("编辑曲目信息")
                Button { app.library.pendingDeletion = track.id } label: { Image(systemName: "trash") }.help("删除曲目")
            }
            if !app.library.note.isEmpty { Text(app.library.note).font(.caption).foregroundStyle(.secondary) }
            if let error = app.library.errorMessage { Text(error).font(.caption).foregroundStyle(.orange) }
        }
        .buttonStyle(PlayerGlyphStyle())
        .sheet(isPresented: $editing) { SongEditorView(track: track, onChanged: onChanged) }
        .alert("删除《\(track.title)》？", isPresented: Binding(get: { app.library.pendingDeletion == track.id }, set: { if !$0 { app.library.pendingDeletion = nil } })) {
            Button("删除", role: .destructive) {
                // alert 关闭会清 binding，先把确认的 id 捕获。
                let id = track.id
                Task {
                    await app.player.prepareDeletion(id)
                    app.library.pendingDeletion = id
                    if await app.library.deleteConfirmed() { await app.reloadTracks(); app.homeVersion += 1; app.libraryPath = [] }
                    else { app.errorMessage = app.library.errorMessage }
                }
            }
            Button("取消", role: .cancel) { app.library.pendingDeletion = nil }
        } message: { Text("歌词、分词、署名、收听记录也会删除。分词校正留着，重新导入时恢复。音频文件不会删除。") }
    }
}

@MainActor
struct SongEditorView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    let track: Track
    let onChanged: () -> Void
    @State private var draft: SongEdit
    init(track: Track, onChanged: @escaping () -> Void) { self.track = track; self.onChanged = onChanged; _draft = State(initialValue: SongEdit(track)) }
    private var valid: Bool { !draft.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !draft.artist.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    var body: some View {
        Form {
            Text("编辑曲目信息").font(.title2.bold())
            TextField("歌名", text: $draft.title)
            TextField("歌手（多人用 / 隔开）", text: $draft.artist)
            TextField("专辑", text: $draft.album)
            TextField("年份", text: $draft.year)
            TextField("流派", text: $draft.genre)
            Text("只改曲库，不写音频标签。演唱署名和专辑归属会跟着更新。").font(.caption).foregroundStyle(.secondary)
            if let error = app.library.errorMessage { Text(error).foregroundStyle(.orange).textSelection(.enabled) }
            HStack {
                Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("保存") { Task { if await app.library.save(track.id, edit: draft) { await app.reloadTracks(); app.homeVersion += 1; onChanged(); dismiss() } } }
                    .disabled(!valid || draft == SongEdit(track) || app.library.busy).keyboardShortcut(.defaultAction)
            }
        }.padding(24).frame(width: 480).glass(.sidebar)
    }
}
