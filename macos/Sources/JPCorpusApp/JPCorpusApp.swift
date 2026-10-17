import AppKit
import JPCore
import SwiftUI

@main
@MainActor
struct JPCorpusApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var app = AppModel()

    init() {
        // `swift run` 起来的是裸可执行文件，不是 .app：不设这一句窗口不会到前台、菜单栏也不出来
        NSApplication.shared.setActivationPolicy(.regular)
        if SmokeRun.isEnabled { SmokeRun.markLaunched() }
    }

    var body: some Scene {
        WindowGroup("JPOP Corpus") {
            ContentView()
                .environment(app)
                .frame(minWidth: 960, minHeight: 600)
                .task {
                    if SmokeRun.isEnabled {
                        await SmokeRun.run(app)
                    } else {
                        await app.restore()
                    }
                }
                .onAppear { NSApplication.shared.activate(ignoringOtherApps: true) }
        }
        .defaultSize(width: 1280, height: 800)
        .commands {
            CommandMenu("播放") {
                Button(app.player.isPlaying ? "暂停" : "播放") { app.player.toggle() }
                    .keyboardShortcut(.space, modifiers: [.option])
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
                Button(app.showStage ? "退出全屏歌词" : "全屏歌词") { app.showStage.toggle() }
                    .keyboardShortcut("f", modifiers: [.command, .shift])
                    .disabled(app.player.track == nil)
                Toggle("振假名", isOn: Binding(get: { app.showFurigana }, set: { app.showFurigana = $0 }))
                    .keyboardShortcut("r", modifiers: [.command, .shift])
            }
            CommandGroup(after: .textEditing) {
                Button("全库搜索…") { app.showQuickSearch = true }
                    .keyboardShortcut("k", modifiers: [.command])
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

/// 只为冒烟模式记生命周期：CI 上应用一闪而过时，要知道是没开出窗口、还是被谁要求退出
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        guard SmokeRun.isEnabled else { return }
        SmokeRun.trace("didFinishLaunching，窗口数 \(NSApplication.shared.windows.count)")
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if SmokeRun.isEnabled {
            let visible = sender.windows.filter { $0.isVisible }.count
            SmokeRun.trace("shouldTerminate，窗口数 \(sender.windows.count)，可见 \(visible)")
        }
        return .terminateNow
    }

    func applicationWillTerminate(_ notification: Notification) {
        if SmokeRun.isEnabled { SmokeRun.trace("willTerminate") }
    }
}
