import JPCorpusModel
import AppKit
import Foundation
import Darwin
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

    private static var started = false

    static func run(_ app: AppModel) async {
        guard let out = outputDirectory, !started else { return }
        started = true
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        var log: [String] = []
        func note(_ line: String) {
            log.append(line)
            trace(line)
            // Persist each step: a crash or timeout must leave useful diagnostics.
            try? (log.joined(separator: newline) + newline).write(to: out.appendingPathComponent("smoke.txt"), atomically: true, encoding: .utf8)
        }
        func require(_ condition: Bool, _ name: String, _ values: String) throws {
            note("[step] \(name) \(values) status=\(condition ? "pass" : "fail")")
            if !condition { throw SmokeFailure.step(name) }
        }

        do {
            note("[step] launch windows=\(NSApplication.shared.windows.count)")
            guard let audioPath = ProcessInfo.processInfo.environment["JPCORPUS_SMOKE_AUDIO"] else {
                throw SmokeFailure.step("missing-JPCORPUS_SMOKE_AUDIO")
            }
            app.audioRoots = [audioPath]
            await app.openDemo()
            try require(app.client != nil && app.errorMessage == nil, "open-demo", "tracks=\(app.tracks.count)")
            await settle(2)
            shot("01-start", into: out, note: note)

            app.search("夜", field: .lemma)
            await settle(2)
            shot("02-kwic", into: out, note: note)
            guard let client = app.client else { throw SmokeFailure.step("missing-client") }
            let hits = try await client.kwic(KwicQuery(keywords: ["夜"], field: .lemma))
            try require(!hits.isEmpty, "kwic", "hits=\(hits.count)")
            if let hit = hits.first {
                await app.player.audition(hit.utteranceId)
                await settle(1.5)
                let p = app.player
                try require(p.isPlaying && p.lastError == nil, "audition", "playing=\(p.isPlaying) song=\(p.track?.id ?? "none")")
                shot("03-audition", into: out, note: note)
                let wait = max(0, (hit.endSec ?? 0) - p.position) + 1
                await settle(min(wait, 12))
                try require(!p.isPlaying, "audition-stopped", "playing=\(p.isPlaying)")
            }

            for (name, section) in [("04-library", SidebarItem.library), ("05-analysis", .analysis), ("06-import", .importer)] {
                app.section = section
                await settle(2)
                shot(name, into: out, note: note)
            }

            let folder = URL(fileURLWithPath: audioPath, isDirectory: true).appendingPathComponent("import", isDirectory: true)
            let files = try FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)
                .filter { $0.pathExtension.lowercased() == "wav" }.sorted { $0.path < $1.path }
            guard let first = files.first else { throw SmokeFailure.step("missing-import-fixture") }
            // This is the same model instance and completion route used by ImportView.
            let model = app.importer
            await model.handleSelection(.success([first]), kind: .files, client: app.client)
            try require(model.errorMessage == nil && model.plan?.summary.total == 1, "single-file", "plan.total=\(model.plan?.summary.total ?? -1)")
            await settle(0.5)
            shot("07-single-file-plan", into: out, note: note)

            await model.handleSelection(.success([folder]), kind: .folders, client: app.client)
            let planned = model.plan?.summary.new ?? 0
            try require(model.errorMessage == nil && planned > 0, "folder", "plan.new=\(planned)")
            await settle(0.5)
            shot("08-folder-plan", into: out, note: note)
            let before = app.tracks.count
            model.tokenize = false // Dictionary/tokenization has a separate real-FFI test.
            await model.runImport(app: app)
            let added = app.tracks.count - before
            try require(model.errorMessage == nil && app.errorMessage == nil && added == planned && added > 0 && model.lastResult?.failed == 0,
                        "import", "tracks.before=\(before) tracks.after=\(app.tracks.count) added=\(added)")
            app.section = .library
            await settle(2)
            shot("09-imported-library", into: out, note: note)
            let screenshot = FileManager.default.fileExists(atPath: out.appendingPathComponent("09-imported-library.png").path)
            try require(screenshot, "imported-library", "screenshot=\(screenshot)")
            note("[step] complete status=pass")
            NSApplication.shared.terminate(nil)
        } catch {
            note("[step] complete status=fail error=\(error.localizedDescription.replacingOccurrences(of: newline, with: " "))")
            exit(EXIT_FAILURE)
        }
    }

    private enum SmokeFailure: LocalizedError {
        case step(String)
        var errorDescription: String? {
            switch self { case .step(let name): return "Smoke assertion failed: \(name)" }
        }
    }

    private static func settle(_ seconds: Double) async {
        try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
    }

    /// 每个可见窗口画一张 PNG。画整个窗框（含工具栏），不只是内容区
    private static func shot(_ name: String, into dir: URL, note: (String) -> Void) {
        let windows = NSApplication.shared.windows.filter { $0.isVisible && $0.contentView != nil }
        if windows.isEmpty { note("[step] screenshot name=\(name) visible=0") }
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
            var gotComposite = false
            do {
                try capture.run()
                capture.waitUntilExit()
                gotComposite = capture.terminationStatus == 0 && FileManager.default.fileExists(atPath: composited.path)
            } catch { note("[step] screenshot-fallback name=\(base)") }

            view.cacheDisplay(in: view.bounds, to: rep)
            if let png = rep.representation(using: .png, properties: [:]) {
                try? png.write(to: dir.appendingPathComponent(gotComposite ? "\(base)-self.png" : "\(base).png"))
            }
            note("[step] screenshot name=\(base) width=\(Int(view.bounds.width)) height=\(Int(view.bounds.height)) composite=\(gotComposite)")
        }
    }
}
