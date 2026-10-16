import JPCore
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        @Bindable var app = app
        Group {
            if app.client == nil {
                WelcomeView()
            } else {
                NavigationSplitView {
                    List(SidebarItem.allCases, selection: Binding<SidebarItem?>(get: { app.section }, set: { if let v = $0 { app.section = v } })) { item in
                        Label(item.title, systemImage: item.symbol).tag(item)
                    }
                    .navigationSplitViewColumnWidth(min: 150, ideal: 170)
                    .safeAreaInset(edge: .bottom) { LibraryBadge() }
                } detail: {
                    Group {
                        switch app.section {
                        case .search: SearchView()
                        case .library: LibraryView()
                        case .analysis: AnalysisView()
                        }
                    }
                    .inspector(isPresented: $app.showLyrics) {
                        LyricsPanel()
                            .inspectorColumnWidth(min: 280, ideal: 340, max: 520)
                    }
                    .toolbar {
                        ToolbarItem(placement: .primaryAction) {
                            Button {
                                app.showLyrics.toggle()
                            } label: {
                                Label("歌词", systemImage: "quote.bubble")
                            }
                            .help("显示 / 隐藏歌词面板（⌘Y）")
                        }
                    }
                }
                .safeAreaInset(edge: .bottom, spacing: 0) { PlayerBar() }
            }
        }
        .alert("出错了", isPresented: Binding(get: { app.errorMessage != nil }, set: { if !$0 { app.errorMessage = nil } })) {
            Button("好") { app.errorMessage = nil }
        } message: {
            Text(app.errorMessage ?? "")
        }
    }
}

/// 侧栏底部：当前打开的是哪个库
private struct LibraryBadge: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(app.isDemo ? "示例语料库" : (app.libraryURL?.lastPathComponent ?? ""))
                .font(.caption.weight(.medium))
            Text("\(app.tracks.count) 首")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
    }
}

/// 还没有库的时候
struct WelcomeView: View {
    @Environment(AppModel.self) private var app
    @State private var picking = false

    var body: some View {
        VStack(spacing: 18) {
            Image(systemName: "music.quarternote.3")
                .font(.system(size: 44, weight: .light))
                .foregroundStyle(.tint)
            Text("JPOP Corpus")
                .font(.largeTitle.weight(.semibold))
            Text("选择语料库文件夹（里面有 corpus.db）。Windows 版的库目录整个拷过来就能用；\n音频不在同一处的话，之后在设置里添加音频文件夹。")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            HStack(spacing: 12) {
                Button("选择语料库文件夹…") { picking = true }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                Button("先看看示例") { Task { await app.openDemo() } }
                    .controlSize(.large)
            }
            Text("默认位置：\(AppModel.defaultLibraryFolder.path)")
                .font(.caption)
                .foregroundStyle(.tertiary)
                .textSelection(.enabled)
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result {
                Task { await app.open(folder: url) }
            }
        }
    }
}
