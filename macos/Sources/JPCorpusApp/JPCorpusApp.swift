import JPCorpusModel
import AppKit
import JPCore
import SwiftUI

@main
@MainActor
struct JPCorpusApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var app = AppModel(defaults:
        SmokeRun.isEnabled ? UserDefaults(suiteName: "JPCorpusSmoke-\(UUID().uuidString)")! : .standard)

    /// 新装第一次启动：先选界面语言（和 Windows 一样）
    @State private var askLanguage = false

    init() {
        // 老用户（打开过库）默认中文，不问；新装的跟随系统，首次启动再问一次
        if !SmokeRun.isEnabled, UserDefaults.standard.string(forKey: L10n.languageKey) == nil {
            if UserDefaults.standard.string(forKey: "libraryFolder") != nil {
                L10n.choose("zh")
            } else {
                _askLanguage = State(initialValue: true)
            }
        }
        // `swift run` 起来的是裸可执行文件，不是 .app：不设这一句窗口不会到前台、菜单栏也不出来
        NSApplication.shared.setActivationPolicy(.regular)
        PlaybackInputProtection.install()
        if SmokeRun.isEnabled { SmokeRun.markLaunched() }
    }

    var body: some Scene {
        // 单个主窗口（不是 WindowGroup）：⌘W 关掉后，点 Dock 图标、「窗口」菜单里的「JPOP Corpus」都能把它开回来，
        // 不会开出第二个窗口；播放、库都在 AppModel 里，窗口关了也不停
        Window("JPOP Corpus", id: MainWindow.id) {
            ContentView()
                .id(app.languageRevision)
                .environment(app)
                .frame(minWidth: 960, minHeight: 600)
                .sheet(isPresented: $askLanguage) {
                    LanguageChoiceView { language in
                        L10n.choose(language)
                        app.languageRevision += 1
                        askLanguage = false
                    }
                }
                .task {
                    // 窗口关了再开会再走一遍：库已经开着就不再开一次（否则会换掉正在用的连接）
                    guard !MainWindow.started else { return }
                    MainWindow.started = true
                    if SmokeRun.isEnabled {
                        await SmokeRun.run(app)
                    } else {
                        MainWindow.installMediaControls(app)
                        await app.restore()
                    }
                }
                .onAppear { NSApplication.shared.activate(ignoringOtherApps: true) }
        }
        .defaultSize(width: 1280, height: 800)
        .commands {
            CommandMenu("播放") {
                Button(app.player.isPlaying ? "暂停" : "播放") { app.player.toggle() }
                    .keyboardShortcut(.space, modifiers: [])
                Button("播放 / 暂停（备用）") { app.player.toggle() }
                    .keyboardShortcut(.space, modifiers: [.option])
                Button("后退 5 秒") { Task { await app.player.nudge(-5) } }
                    .keyboardShortcut(.leftArrow, modifiers: [])
                Button("前进 5 秒") { Task { await app.player.nudge(5) } }
                    .keyboardShortcut(.rightArrow, modifiers: [])
                Button("增加音量") { app.player.setVolume(app.player.volume + 0.05) }
                    .keyboardShortcut(.upArrow, modifiers: [])
                Button("减小音量") { app.player.setVolume(app.player.volume - 0.05) }
                    .keyboardShortcut(.downArrow, modifiers: [])
                Divider()
                // 不用 ⌘← / ⌘→：输入框里那是「到行首 / 行尾」，菜单快捷键会先把它抢走
                Button("上一首") { Task { await app.player.previous() } }
                    .keyboardShortcut(.leftArrow, modifiers: [.option, .command])
                    .disabled(app.player.track == nil)
                Button("下一首") { Task { await app.player.next() } }
                    .keyboardShortcut(.rightArrow, modifiers: [.option, .command])
                    .disabled(!app.player.hasNext)
                Button("上一行") { Task { await app.player.step(-1) } }
                    .keyboardShortcut(.upArrow, modifiers: [.option, .command])
                Button("下一行") { Task { await app.player.step(1) } }
                    .keyboardShortcut(.downArrow, modifiers: [.option, .command])
                Button(app.player.loop == nil ? "循环当前行" : "取消循环") {
                    Task { await app.player.toggleLoop(nil) }
                }
                .keyboardShortcut("l", modifiers: [.command])
                Divider()
                Button(app.showLyrics ? "隐藏歌词面板" : "显示歌词面板") { app.showLyrics.toggle() }
                    .keyboardShortcut("y", modifiers: [.command])
                Button(app.showStage ? "退出全屏歌词" : "全屏歌词") {
                    app.showStage.toggle()
                    // 全屏歌词在主窗口里：主窗口被 ⌘W 关掉了就先开回来
                    if app.showStage, MainWindow.window?.isVisible != true { MainWindow.open?(id: MainWindow.id) }
                }
                    .keyboardShortcut("f", modifiers: [.command, .shift])
                    .disabled(app.player.track == nil)
                Toggle("振假名", isOn: Binding(get: { app.showFurigana }, set: { app.showFurigana = $0 }))
                    .keyboardShortcut("r", modifiers: [.command, .shift])
                Button("窗口全屏") { NSApplication.shared.keyWindow?.toggleFullScreen(nil) }
                    .keyboardShortcut(KeyEquivalent("\u{F70E}"), modifiers: [])
            }
            // 文件菜单：打开 / 新建语料库。以前只有设置页里一个「换一个…」，打开过库之后很难找到
            CommandGroup(replacing: .newItem) {
                Button("打开语料库…") {
                    if app.showStage { app.showStage = false }
                    app.showLibraryPicker = true
                }
                .keyboardShortcut("o", modifiers: [.command])
                Button("新建语料库") { Task { await app.createLibrary() } }
            }
            CommandGroup(after: .textEditing) {
                Button("全库搜索…") { app.showQuickSearch = true }
                    .keyboardShortcut("k", modifiers: [.command])
                    .disabled(app.client == nil)
                Button("命令面板…") { app.paletteCommands = true; app.showQuickSearch = true }
                    .keyboardShortcut("p", modifiers: [.command])
                    .disabled(app.client == nil)
            }
            CommandGroup(after: .sidebar) {
                ForEach(Array(SidebarItem.allCases.enumerated()), id: \.element) { index, item in
                    Button(item.title) { app.section = item }
                        .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: [.command])
                }
            }
        }


        Settings {
            SettingsView()
                .environment(app)
        }
    }
}

/// 主窗口：记住它和打开它的办法，Dock 图标被点时用
@MainActor
enum MainWindow {
    static let id = "main"
    static var started = false
    static weak var window: NSWindow?
    static var open: OpenWindowAction?
    private static var media: SystemMediaControls?
    /// 退出前要把这首的收听记进库
    static weak var model: AppModel?

    static func installMediaControls(_ app: AppModel) {
        model = app
        guard media == nil else { return }
        media = SystemMediaControls(player: app.player) { [weak app] track in
            guard let client = app?.client else { return nil }
            return (try? await client.resolveMedia(track.coverPath)) ?? nil
        }
    }

    /// 主窗口在就拿到前面（最小化了就还原），不在就重新开
    static func show() {
        if let window, window.isVisible || window.isMiniaturized {
            if window.isMiniaturized { window.deminiaturize(nil) }
            window.makeKeyAndOrderFront(nil)
        } else {
            open?(id: id)
        }
        NSApplication.shared.activate(ignoringOtherApps: true)
    }
}

/// 记生命周期（冒烟模式）；Dock 图标被点时把主窗口开回来
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// 主窗口被 ⌘W 关掉以后点 Dock 图标：开回主窗口。全屏歌词还开着（在另一个桌面）时系统认为
    /// 「有窗口」，不会自己开，所以这里看的是主窗口本身
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        guard MainWindow.window?.isVisible != true else { return true }
        MainWindow.show()
        return false
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard SmokeRun.isEnabled else { return }
        SmokeRun.trace("didFinishLaunching，窗口数 \(NSApplication.shared.windows.count)")
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if SmokeRun.isEnabled {
            // 冒烟也走下面真正的退出流程（先写播放历史再退）：应用内更新就是从这条路卡死的，
            // 以前冒烟在这里直接放行，CI 看不出来。退干净了 willTerminate 会留下 quit.txt
            let visible = sender.windows.filter { $0.isVisible }.count
            SmokeRun.trace("shouldTerminate，窗口数 \(sender.windows.count)，可见 \(visible)")
        }
        guard let model = MainWindow.model else { return .terminateNow }
        // 正在听的这首还没记进播放历史：写完再退（最多等 2 秒，写入卡住也不拦着退出）
        let gate = TerminateGate(sender)
        Task { @MainActor in
            await model.player.finishHistory()
            gate.reply()
        }
        // 兜底用 run loop 定时器，不用 Task.sleep：terminate 要是在主队列上的一个 Task 里调的，
        // 系统等回复时转的是嵌套 run loop，主队列被那个 Task 占着，排在主线程上的 Task 一个都轮不到，
        // 历史写不完、2 秒兜底也不响，程序就永远卡在「正在退出」（应用内更新卡死、⌘Q 也退不了就是这样）。
        // common 模式的定时器在那个嵌套 run loop 里照样会响
        let fallback = Timer(timeInterval: 2, repeats: false) { _ in MainActor.assumeIsolated { gate.reply() } }
        RunLoop.main.add(fallback, forMode: .common)
        return .terminateLater
    }

    func applicationWillTerminate(_ notification: Notification) {
        if SmokeRun.isEnabled {
            SmokeRun.trace("willTerminate")
            SmokeRun.markQuit()
        }
    }
}

/// 从 async 代码（Task 里）退出程序。
///
/// 不能在 Task 里直接调 `terminate`：它在当前这个主队列任务里同步转嵌套 run loop 等
/// `applicationShouldTerminate` 的回复，而回复要靠别的主线程 Task，主队列被占着，永远等不到。
/// 交给 run loop 的定时器去调，`terminate` 就不在任何主队列任务里，等回复时主队列照常运转
@MainActor
enum AppQuit {
    static func request() {
        let timer = Timer(timeInterval: 0, repeats: false) { _ in
            MainActor.assumeIsolated { NSApplication.shared.terminate(nil) }
        }
        RunLoop.main.add(timer, forMode: .common)
    }
}

/// 退出时只回复系统一次：写完历史和 2 秒兜底，谁先到算谁
@MainActor
private final class TerminateGate {
    private let app: NSApplication
    private var replied = false
    init(_ app: NSApplication) { self.app = app }
    func reply() {
        guard !replied else { return }
        replied = true
        app.reply(toApplicationShouldTerminate: true)
    }
}
