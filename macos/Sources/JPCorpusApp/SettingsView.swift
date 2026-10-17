import JPCore
import SwiftUI
import UniformTypeIdentifiers

/// 设置（⌘,）：语料库在哪、音频在哪
@MainActor
struct SettingsView: View {
    @Environment(AppModel.self) private var app
    @State private var pickingLibrary = false
    @State private var pickingAudio = false

    var body: some View {
        Form {
            Section("语料库") {
                LabeledContent("文件夹") {
                    Text(app.isDemo ? "示例语料库" : (app.libraryURL?.path ?? "未选择"))
                        .textSelection(.enabled)
                        .lineLimit(2)
                }
                HStack {
                    Button("换一个…") { pickingLibrary = true }
                    Button("打开示例") { Task { await app.openDemo() } }
                }
                Text("和 Windows 版共用同一种库：整个文件夹（corpus.db、raw/）拷过来就能打开。旧库会自动补齐结构；比本程序新的库会拒绝打开，不会被改坏。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Section("音频文件夹") {
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
                Button("添加文件夹…") { pickingAudio = true }
            }
        }
        .formStyle(.grouped)
        .clearScrollBackground()
        .glass(.sidebar)
        .frame(width: 520, height: 380)
        .fileImporter(isPresented: $pickingLibrary, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result { Task { await app.open(folder: url) } }
        }
        .fileImporter(isPresented: $pickingAudio, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result, !app.audioRoots.contains(url.path) {
                app.audioRoots.append(url.path)
            }
        }
    }
}
