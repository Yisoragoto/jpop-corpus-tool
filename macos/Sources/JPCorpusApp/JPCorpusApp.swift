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

    init() {
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
                .environment(app)
                .frame(minWidth: 960, minHeight: 600)
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
                    // 平时由主窗口开全屏歌词窗口；主窗口被关掉时自己开
                    if app.showStage, MainWindow.window?.isVisible != true { MainWindow.open?(id: "stage") }
                }
                    .keyboardShortcut("f", modifiers: [.command, .shift])
                    .disabled(app.player.track == nil)
                Toggle("振假名", isOn: Binding(get: { app.showFurigana }, set: { app.showFurigana = $0 }))
                    .keyboardShortcut("r", modifiers: [.command, .shift])
                Button("窗口全屏") { NSApplication.shared.keyWindow?.toggleFullScreen(nil) }
                    .keyboardShortcut(KeyEquivalent("\u{F70E}"), modifiers: [])
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

        // 全屏歌词：没有标题栏的独立窗口，打开后切进原生全屏
        Window("全屏歌词", id: "stage") {
            LyricsStage()
                .environment(app)
                .frame(minWidth: 720, minHeight: 480)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1280, height: 800)

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
            let visible = sender.windows.filter { $0.isVisible }.count
            SmokeRun.trace("shouldTerminate，窗口数 \(sender.windows.count)，可见 \(visible)")
            return .terminateNow
        }
        guard let model = MainWindow.model else { return .terminateNow }
        // 正在听的这首还没记进播放历史：写完再退（最多等 2 秒，写入卡住也不拦着退出）
        let gate = TerminateGate(sender)
        Task { @MainActor in
            await model.player.finishHistory()
            gate.reply()
        }
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(2))
            gate.reply()
        }
        return .terminateLater
    }

    func applicationWillTerminate(_ notification: Notification) {
        if SmokeRun.isEnabled { SmokeRun.trace("willTerminate") }
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
