import JPCorpusModel
import JPCore
import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct ContentView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        @Bindable var app = app
        Group {
            if app.client == nil {
                WelcomeView()
            } else {
                AppShell {
                    Group {
                        switch app.section {
                        case .home: HomeView()
                        case .search: SearchView()
                        case .library: LibraryView()
                        case .analysis: AnalysisView()
                        case .dictionaries: DictionaryView()
                        case .scrape: ScrapeView()
                        case .importer: ImportView()
                        case .settings: SettingsView(embedded: true)
                        case .anki: AnkiView()
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .safeAreaInset(edge: .top) {
                        MediaHealthBanner()
                        if let notice = app.notice {
                            HStack { Text(notice).font(.callout); Spacer(); Button { app.notice = nil } label: { Image(systemName: "xmark") }.buttonStyle(.borderless) }.padding(10).glassCard().padding(.horizontal,12)
                        }
                    }
                    // 播放条是浮在正文下方的一张玻璃卡片。只挂在正文这一栏：
                    // 挂在整个分栏视图上时，各栏不会给它让位，卡片会盖住侧栏底部和正文最后几行
                    .safeAreaInset(edge: .bottom, spacing: 0) {
                        PlayerBar()
                            .onPreferenceChange(PlayerBarHeightKey.self) { height in
                                if SmokeRun.isEnabled { SmokeRun.playerBarHeight = height }
                            }
                            .padding(.horizontal, 12)
                            .padding(.bottom, 10)
                            .padding(.top, 4)
                    }
                    .glass(.underWindowBackground)
                    .inspector(isPresented: $app.showLyrics) {
                        LyricsPanel()
                            .inspectorColumnWidth(min: 280, ideal: 340, max: 520)
                    }
                    .toolbarBackground(.hidden, for: .windowToolbar)
                    .sheet(isPresented: $app.showQuickSearch) {
                        QuickSearchView()
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
                .glass(.sidebar)
            }
        }
        .modifier(LibraryPickerHost())
        .background(GlassWindowConfigurator())
        .background(WindowReader { MainWindow.window = $0 })
        .onAppear { MainWindow.open = openWindow }
        .background(PlaybackKeyboard(app: app))
        .background(UpdatePresentation())
        // 全屏歌词和 Windows 版一样是主窗口里铺满的一层：盖住侧栏、正文和播放条；
        // 工具栏收起来，免得在歌词上面露出一条按钮
        // 全屏词云：同样是盖住整个窗口的一层，在全屏歌词下面
        .overlay {
            if app.visualizations.cloudPresent, app.client != nil {
                CloudView(presenting: true)
                    .ignoresSafeArea()
                    .transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.25), value: app.visualizations.cloudPresent)
        .overlay {
            if app.showStage, app.client != nil {
                LyricsStage()
                    .ignoresSafeArea()
                    .transition(.opacity)
            }
        }
        .toolbar(app.showStage || app.visualizations.cloudPresent ? .hidden : .automatic, for: .windowToolbar)
        .animation(.easeInOut(duration: 0.25), value: app.showStage)
        // 从没有歌到有歌（第一次播放、试听）时打开歌词栏。之后用户自己关了，换歌不再弹出来
        .onChange(of: app.player.track?.id) { old, new in
            if old == nil, new != nil, !app.showLyrics { app.showLyrics = true }
        }
        .task(id: "\(app.player.track?.id ?? "")|\(app.showFurigana)|\(app.furiganaMode.rawValue)") {
            await app.loadFurigana()
        }
        .alert("出错了", isPresented: Binding(get: { app.errorMessage != nil }, set: { if !$0 { app.errorMessage = nil } })) {
            Button("好") { app.errorMessage = nil }
        } message: {
            Text(app.errorMessage ?? "")
        }
    }
}

/// 还没有库的时候
@MainActor
struct WelcomeView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        VStack(spacing: 18) {
            Image(systemName: "music.quarternote.3")
                .font(.system(size: 44, weight: .light))
                .foregroundStyle(.tint)
            Text("JPOP Corpus")
                .font(.largeTitle.weight(.semibold))
            Text("新建一个语料库，从音乐文件夹（音频 + 同名 .lrc）导入；\n或者打开已有的语料库（选文件夹或里面的 corpus.db），Windows 版的库目录整个拷过来就能用。")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            HStack(spacing: 12) {
                Button("新建语料库") { Task { await app.createLibrary() } }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .help("在默认位置建库（已经有就打开它），然后去导入页")
                Button("打开已有语料库…") { app.showLibraryPicker = true }
                    .help("选语料库文件夹，或直接选里面的 corpus.db")
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
    }
}
