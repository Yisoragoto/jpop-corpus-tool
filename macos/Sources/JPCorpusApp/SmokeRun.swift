import AppKit
import Foundation
import JPCore

/// 冒烟模式：没有 Mac 的人也能看到真实界面。
///
/// 设了环境变量 `JPCORPUS_SMOKE=<目录>` 启动时，应用不读上次的库，而是：
/// 打开示例语料库 → 检索 → KWIC → 试听第一条 → 首页 → 曲库（歌曲 / 专辑 / 歌手 / 详情）→ ⌘K → 分析（含存档、质量）→ 导入，
/// 每一步把**自己的窗口**画成 PNG 存进那个目录，最后写一份 `smoke.txt` 记录状态，然后退出。
///
/// 截图用 `cacheDisplay` 让窗口自己渲染，不需要「屏幕录制」权限，所以在 CI 的 macOS 机器上也能跑
/// （`.github/workflows/macos.yml` 把截图作为 artifact 传上去）。
/// `JPCORPUS_SMOKE_AUDIO=<目录>` 指一个音频文件夹，示例库的歌按文件名（001.flac…）去那里找，播放那条路也走一遍。
@MainActor
enum SmokeRun {
    nonisolated static var outputDirectory: URL? {
        ProcessInfo.processInfo.environment["JPCORPUS_SMOKE"].map { URL(fileURLWithPath: $0, isDirectory: true) }
    }

    nonisolated static var isEnabled: Bool { outputDirectory != nil }

    /// 换行符。不写成转义字面量：这份文件是在 Windows 上用脚本改的，转义序列在路上被吃过一次
    nonisolated static let newline = String(UnicodeScalar(10))

    /// 直接写 stderr（不缓冲）：stdout 接到管道时 print 是整块缓冲的，进程一退出就看不到了
    nonisolated static func trace(_ line: String) {
        FileHandle.standardError.write(Data(("[smoke] " + line + newline).utf8))
    }

    /// 应用一启动就调：留个标记文件，CI 上能分清「根本没起来」和「起来了但没开窗口」
    static func markLaunched() {
        guard let out = outputDirectory else { return }
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        try? ("launched \(Date())" + newline).write(to: out.appendingPathComponent("launched.txt"), atomically: true, encoding: .utf8)
        trace("init，输出到 \(out.path)")
    }

    static func run(_ app: AppModel) async {
        guard let out = outputDirectory else { return }
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        var log: [String] = []
        func note(_ line: String) {
            log.append(line)
            trace(line)
        }
        note("窗口任务开始，窗口数 \(NSApplication.shared.windows.count)")

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

        // 首页：刚才那次试听到行尾自动暂停，播放记录里有了，「继续听」应该是这首
        app.section = .home
        await settle(2)
        shot("04-home", into: out, note: note)

        // 曲库三种看法
        app.section = .library
        for (name, mode) in [("05-library-songs", "歌曲"), ("06-library-albums", "专辑"), ("07-library-artists", "歌手")] {
            UserDefaults.standard.set(mode, forKey: "libraryMode")
            await settle(1.5)
            shot(name, into: out, note: note)
        }
        UserDefaults.standard.set("歌曲", forKey: "libraryMode")

        // 歌手详情、歌曲详情
        if let person = try? await app.client?.peopleDirectory().first(where: { $0.performer > 0 }) {
            app.show(.person(person.id))
            await settle(2)
            shot("08-person", into: out, note: note)
        }
        app.libraryPath = []
        app.show(.song("004"))
        await settle(2)
        shot("09-song", into: out, note: note)

        // ⌘K 全库搜索
        app.quickSearchSeed = "夜"
        app.showQuickSearch = true
        await settle(2)
        shot("10-quick-search", into: out, note: note)
        app.showQuickSearch = false
        await settle(0.8)

        // 分析：词频 → 存一条档并重跑 → 存档页 → 语料质量
        app.section = .analysis
        UserDefaults.standard.set(0, forKey: "analysisTab")
        await settle(2)
        shot("11-analysis", into: out, note: note)
        do {
            let q = FrequencyQuery(scope: AnalysisScope(unit: .distinctLines, jpOnly: true), pos: "NOUN", limit: 50)
            if let run = try await app.client?.archiveFrequency(q, label: "冒烟 · 名词") {
                let cmp = try await app.client?.analysisRerun(run.id)
                note("存档 #\(run.id)，重跑结果相同：\(cmp?.resultIdentical == true)")
            }
        } catch {
            note("存档失败：\(error.localizedDescription)")
        }
        UserDefaults.standard.set(2, forKey: "analysisTab")
        await settle(1.5)
        shot("12-archive", into: out, note: note)
        UserDefaults.standard.set(3, forKey: "analysisTab")
        await settle(1.5)
        shot("13-quality", into: out, note: note)
        UserDefaults.standard.set(0, forKey: "analysisTab")

        app.section = .importer
        await settle(2)
        shot("14-import", into: out, note: note)

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
            let base = windows.count == 1 ? name : "\(name)-\(i)"
            // 首选系统截图：窗口服务器合成后的真实画面，毛玻璃（透出桌面的模糊）只有这样才截得到。
            // 需要「屏幕录制」权限；没有的话退回让窗口自己渲染（cacheDisplay 画不出 behindWindow 的模糊）
            let composited = dir.appendingPathComponent("\(base).png")
            let capture = Process()
            capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
            capture.arguments = ["-x", "-o", "-l", String(window.windowNumber), composited.path]
            try? capture.run()
            capture.waitUntilExit()
            let gotComposite = capture.terminationStatus == 0 && FileManager.default.fileExists(atPath: composited.path)

            view.cacheDisplay(in: view.bounds, to: rep)
            if let png = rep.representation(using: .png, properties: [:]) {
                try? png.write(to: dir.appendingPathComponent(gotComposite ? "\(base)-self.png" : "\(base).png"))
            }
            note("\(name)：\(Int(view.bounds.width))×\(Int(view.bounds.height))，系统截图 \(gotComposite ? "有" : "没有（退回自绘）")")
        }
    }
}
