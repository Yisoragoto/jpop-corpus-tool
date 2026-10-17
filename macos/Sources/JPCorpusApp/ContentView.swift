import JPCore
import SwiftUI
import UniformTypeIdentifiers

@MainActor
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
                        case .importer: ImportView()
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .glass(.underWindowBackground)
                    .inspector(isPresented: $app.showLyrics) {
                        LyricsPanel()
                            .inspectorColumnWidth(min: 280, ideal: 340, max: 520)
                    }
                    .toolbarBackground(.hidden, for: .windowToolbar)
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
                // 播放条是浮在玻璃上的一张卡片，不是贴底的一整条
                .safeAreaInset(edge: .bottom, spacing: 0) {
                    PlayerBar()
                        .padding(.horizontal, 12)
                        .padding(.bottom, 10)
                        .padding(.top, 4)
                }
                .glass(.sidebar)
            }
        }
        .background(GlassWindowConfigurator())
        .alert("出错了", isPresented: Binding(get: { app.errorMessage != nil }, set: { if !$0 { app.errorMessage = nil } })) {
            Button("好") { app.errorMessage = nil }
        } message: {
            Text(app.errorMessage ?? "")
        }
    }
}

/// 侧栏底部：当前打开的是哪个库
@MainActor
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
@MainActor
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
            Text("新建一个语料库，从音乐文件夹（音频 + 同名 .lrc）导入；\n或者选择已有的语料库文件夹（里面有 corpus.db），Windows 版的库目录整个拷过来就能用。")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            HStack(spacing: 12) {
                Button("新建语料库") { Task { await app.createLibrary() } }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .help("在默认位置建库（已经有就打开它），然后去导入页")
                Button("选择语料库文件夹…") { picking = true }
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
        .glass(.sidebar)
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result {
                Task { await app.open(folder: url) }
            }
        }
    }
}
