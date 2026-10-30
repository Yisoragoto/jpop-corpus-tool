import JPCorpusModel
import JPCore
import SwiftUI
import UniformTypeIdentifiers

/// 设置（⌘,）：语料库在哪、音频在哪
@MainActor
struct SettingsView: View {
    @Environment(AppModel.self) private var app
    var embedded = false
    private enum PickKind { case library, audio }
    @State private var pick = FilePickState<PickKind>()
    @State private var lyricsOpen = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        Form {
            UpdateSettingsView()
            DiagnosticsSettingsView()
            CloudPreferencesView()
            HealthSettingsView()
            Section("Anki") { MineSettingsView() }
            Section("播放") {
                LabeledContent("每天的收听目标") {
                    Slider(value: Binding(get: { app.dailyGoalMinutes }, set: { app.dailyGoalMinutes = $0 }), in: 5...180, step: 1)
                    Text("\(Int(app.dailyGoalMinutes)) 分钟").monospacedDigit()
                }
                Toggle("逐字点亮当前行", isOn: Binding(get: { app.karaokeLyrics }, set: { app.karaokeLyrics = $0 }))
                Toggle("显示频谱", isOn: Binding(get: { app.player.spectrumEnabled }, set: { app.player.spectrumEnabled = $0 }))
                Toggle("全屏歌词使用虚化封面", isOn: Binding(get: { app.stageCoverBlur }, set: { app.stageCoverBlur = $0 }))
                LabeledContent("封面模糊") {
                    Slider(value: Binding(get: { app.stageBlurRadius }, set: { app.stageBlurRadius = $0 }), in: 20...120, step: 1)
                    Text("\(Int(app.stageBlurRadius))").monospacedDigit().frame(width: 28)
                }.disabled(!app.stageCoverBlur)
            }
            Section("歌词") {
                Toggle("默认显示振假名", isOn: Binding(get: { app.showFurigana }, set: { app.showFurigana = $0 }))
                HStack { Text("字体、备用字体、字号、注音与间距"); Spacer(); LyricsDisplayButton() }
                Button("歌词来源、批量补全与逐首预览") { lyricsOpen = true }.disabled(app.core == nil)
            }
            Section("资料库管理") {
                Toggle("启动时检查新歌", isOn: Binding(get: { app.autoCheckNewSongs }, set: { app.autoCheckNewSongs = $0 }))
                HStack { Text("歌曲、音频、成员、维护与迁移"); Spacer(); LibraryManageButton() }.disabled(app.core == nil)
            }
            Section("语料库") {
                LabeledContent("文件夹") {
                    Text(app.isDemo ? "示例语料库" : (app.libraryURL?.path ?? "未选择"))
                        .textSelection(.enabled)
                        .lineLimit(2)
                }
                HStack {
                    Button("换一个…") { pick.present(.library) }
                    Button("打开示例") { Task { await app.openDemo() } }
                }
                Text("和 Windows 版共用同一种库：整个文件夹（corpus.db、raw/）拷过来就能打开。旧库会自动补齐结构；比本程序新的库会拒绝打开，不会被改坏。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Section("音频目录（路径映射）") {
                Text("这里只决定已在库里的曲目去哪找音频文件；导入新歌请去『导入』页。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if app.audioRoots.isEmpty {
                    Text("没有。库里记的音频路径在这台电脑上不存在时，会到这些文件夹里按文件名找。")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                ForEach(app.audioRoots, id: \.self) { root in
                    HStack {
                        Text(root).lineLimit(1).truncationMode(.middle)
                        Spacer()
                        Button(role: .destructive) {
                            app.audioRoots.removeAll { $0 == root }
                        } label: {
                            Image(systemName: "minus.circle")
                        }
                        .buttonStyle(.borderless)
                    }
                }
                HStack {
                    Button("添加文件夹…") { pick.present(.audio) }
                    Button("去导入") { app.section = .importer; if !embedded { dismiss() } }
                        .disabled(app.client == nil)
                }
            }
            if let error = app.errorMessage {
                Text(error).foregroundStyle(.orange).textSelection(.enabled)
            }
        }
        .formStyle(.grouped)
        .clearScrollBackground()
        .glass(.sidebar)
        .frame(width: embedded ? nil : 560, height: embedded ? nil : 460)
        .sheet(isPresented: $lyricsOpen) { LyricsLibraryView() }
        .fileImporter(isPresented: $pick.isPresented, allowedContentTypes: pick.intent == .library ? [.folder, UTType(filenameExtension: "db") ?? .data] : [.folder]) { result in
            let kind = pick.finish()
            switch result {
            case .failure(let error):
                app.errorMessage = error.localizedDescription
            case .success(let url):
                switch kind {
                case .library:
                    Task { await app.selectLibrary(picked: url) }
                case .audio:
                    if !app.audioRoots.contains(url.path) { app.audioRoots.append(url.path) }
                case nil:
                    app.errorMessage = "文件选择的用途已丢失，请重新选择"
                }
            }
        }
        .alert("新建语料库？", isPresented: Binding(
            get: { app.pendingCreationURL != nil },
            set: { if !$0 { app.cancelCreation() } }
        )) {
            if let folder = app.pendingCreationURL {
                Button("新建") { Task { await app.confirmCreation(in: folder) } }
                Button("取消", role: .cancel) { app.cancelCreation() }
            }
        } message: {
            Text("这里没有 corpus.db，要在这里新建语料库吗？\n\(app.pendingCreationURL?.path ?? "")")
        }
    }
}
