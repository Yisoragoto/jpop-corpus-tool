import AppKit
import JPCore
import SwiftUI

@main
struct JPCorpusApp: App {
    @State private var app = AppModel()

    init() {
        // `swift run` 起来的是裸可执行文件，不是 .app：不设这一句窗口不会到前台、菜单栏也不出来
        NSApplication.shared.setActivationPolicy(.regular)
    }

    var body: some Scene {
        WindowGroup("JPOP Corpus") {
            ContentView()
                .environment(app)
                .frame(minWidth: 960, minHeight: 600)
                .task { await app.restore() }
                .onAppear { NSApplication.shared.activate(ignoringOtherApps: true) }
        }
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
