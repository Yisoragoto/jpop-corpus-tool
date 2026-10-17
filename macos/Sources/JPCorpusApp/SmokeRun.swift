import AppKit
import Foundation
import JPCore

/// 冒烟模式：没有 Mac 的人也能看到真实界面。
///
/// 设了环境变量 `JPCORPUS_SMOKE=<目录>` 启动时，应用不读上次的库，而是：
/// 打开示例语料库 → 检索 → KWIC → 试听第一条 → 曲库 → 分析 → 导入，
/// 每一步把**自己的窗口**画成 PNG 存进那个目录，最后写一份 `smoke.txt` 记录状态，然后退出。
///
/// 截图用 `cacheDisplay` 让窗口自己渲染，不需要「屏幕录制」权限，所以在 CI 的 macOS 机器上也能跑
/// （`.github/workflows/macos.yml` 把截图作为 artifact 传上去）。
/// `JPCORPUS_SMOKE_AUDIO=<目录>` 指一个音频文件夹，示例库的歌按文件名（001.flac…）去那里找，播放那条路也走一遍。
@MainActor
enum SmokeRun {
    static var outputDirectory: URL? {
        ProcessInfo.processInfo.environment["JPCORPUS_SMOKE"].map { URL(fileURLWithPath: $0, isDirectory: true) }
    }

    static var isEnabled: Bool { outputDirectory != nil }

    static func run(_ app: AppModel) async {
        guard let out = outputDirectory else { return }
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        var log: [String] = []
        func note(_ line: String) {
            log.append(line)
            print("[smoke] \(line)")
        }

        if let audio = ProcessInfo.processInfo.environment["JPCORPUS_SMOKE_AUDIO"] {
            app.audioRoots = [audio]
        }
        await app.openDemo()
        note("示例库：\(app.tracks.count) 首，错误：\(app.errorMessage ?? "无")")
        await settle(2)
        shot("01-start", into: out, note: note)

        // 检索 → KWIC
        app.search("夜", field: .lemma)
        await settle(2)
        shot("02-kwic", into: out, note: note)

        // 试听第一条：加载那首歌、从行首放、到行尾停
        if let hit = try? await app.client?.kwic(KwicQuery(keywords: ["夜"], field: .lemma)).first {
            await app.player.audition(hit.utteranceId)
            await settle(1.5)
            let p = app.player
            note("试听：歌 \(p.track?.id ?? "-")，在放 \(p.isPlaying)，位置 \(String(format: "%.2f", p.position))，"
                + "命中行 \(String(format: "%.2f", hit.timeSec ?? -1))–\(String(format: "%.2f", hit.endSec ?? -1))，"
                + "当前行 \(p.currentUtterance.map(String.init) ?? "-")（期望 \(hit.utteranceId)），错误：\(p.lastError ?? "无")")
            shot("03-audition", into: out, note: note)
            // 等过行尾，看是不是自己停了
            let wait = max(0, (hit.endSec ?? 0) - p.position) + 1
            await settle(min(wait, 12))
            note("行尾之后：在放 \(app.player.isPlaying)，位置 \(String(format: "%.2f", app.player.position))")
        } else {
            note("KWIC 没有结果")
        }

        for (name, section) in [("04-library", SidebarItem.library), ("05-analysis", .analysis), ("06-import", .importer)] {
            app.section = section
            await settle(2)
            shot(name, into: out, note: note)
        }

        try? log.joined(separator: "\n").write(to: out.appendingPathComponent("smoke.txt"), atomically: true, encoding: .utf8)
        NSApplication.shared.terminate(nil)
    }

    private static func settle(_ seconds: Double) async {
        try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
    }

    /// 每个可见窗口画一张 PNG。画整个窗框（含工具栏），不只是内容区
    private static func shot(_ name: String, into dir: URL, note: (String) -> Void) {
        let windows = NSApplication.shared.windows.filter { $0.isVisible && $0.contentView != nil }
        if windows.isEmpty { note("\(name)：没有可见窗口") }
        for (i, window) in windows.enumerated() {
            guard let view = window.contentView?.superview ?? window.contentView,
                  let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds)
            else { continue }
            view.cacheDisplay(in: view.bounds, to: rep)
            guard let png = rep.representation(using: .png, properties: [:]) else { continue }
            let file = dir.appendingPathComponent(windows.count == 1 ? "\(name).png" : "\(name)-\(i).png")
            try? png.write(to: file)
            note("\(name)：\(Int(view.bounds.width))×\(Int(view.bounds.height))")
        }
    }
}
