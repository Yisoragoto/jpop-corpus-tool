import JPCorpusModel
import AppKit
import Foundation
import Darwin
import JPCore

/// 冒烟模式：没有 Mac 的人也能看到真实界面。
///
/// 设了环境变量 `JPCORPUS_SMOKE=<目录>` 启动时，应用不读上次的库，而是：
/// 打开示例语料库 → 检索 → KWIC → 试听第一条 → 振假名 → 全屏歌词 → 首页 → 曲库（歌曲 / 专辑 / 歌手 / 详情）→ ⌘K
/// → 分析（含存档、质量）→ 词典 → 刮削 → 导入（扫描单文件、扫描文件夹并真导入），
/// 每一步把**自己的窗口**画成 PNG；即时写 `smoke.txt`，断言失败以非零退出。
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
    static var playerBarHeight: CGFloat = 0

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
        func requirePlayerBarHeight() throws {
            let height = playerBarHeight
            try require(height.isFinite && height > 0 && height <= 120, "player-bar", "height=\(height)")
        }

        do {
            note("[step] launch windows=\(NSApplication.shared.windows.count)")
            guard let audioPath = ProcessInfo.processInfo.environment["JPCORPUS_SMOKE_AUDIO"] else {
                throw SmokeFailure.step("missing-JPCORPUS_SMOKE_AUDIO")
            }
            app.audioRoots = [audioPath]
            await app.openDemo()
            try require(app.client != nil && app.errorMessage == nil, "open-demo", "tracks=\(app.tracks.count)")
            guard let manager = app.library.client, let original = app.tracks.first else { throw SmokeFailure.step("library-edit-missing-client") }
            var edited = SongEdit(original); edited.title += "（管理验证）"
            let saved = await app.library.save(original.id, edit: edited)
            let readback = try await app.core?.track(original.id)
            try require(saved && readback?.title == edited.title, "library-edit", "readback=true")
            _ = try await manager.libraryEditSong(original.id, edit: SongEdit(original))
            await app.reloadTracks()
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
                try requirePlayerBarHeight()
                shot("03-audition", into: out, note: note)
                let wait = max(0, (hit.endSec ?? 0) - p.position) + 1
                await settle(min(wait, 12))
                try require(!p.isPlaying, "audition-stopped", "playing=\(p.isPlaying)")
            }

            // 真 FLAC 经 TimePitch 播放：调整时不重载、不跳回起点。
            if let track = app.player.track {
                let player = app.player
                await player.open(track, at: 0, autoplay: true)
                player.setPitch(2)
                player.setRate(1.25)
                let before = player.position
                await settle(1.2)
                try require(player.isPlaying && player.pitch == 2 && player.rate == 1.25 && player.position > before + 0.5 && player.lastError == nil,
                            "playback-tuning", "pitch=\(player.pitch) rate=\(player.rate) advanced=\(player.position > before + 0.5)")
                player.pause()
                player.resetTuning()
            } else { throw SmokeFailure.step("playback-tuning-missing-track") }

            // Exercise the compact layout with the lyrics inspector still open, and without spectrum.
            if let window = NSApplication.shared.windows.first(where: { $0.isVisible && $0.contentView != nil }) {
                let originalFrame = window.frame
                window.setContentSize(NSSize(width: 960, height: 600))
                await settle(0.8)
                try requirePlayerBarHeight()
                shot("03a-compact-player", into: out, note: note)
                app.player.spectrumEnabled = false
                await settle(0.3)
                try requirePlayerBarHeight()
                app.player.spectrumEnabled = true
                window.setFrame(originalFrame, display: true)
                await settle(0.8)
            }

            // 振假名要分词词典：CI 把缓存的那份目录传进来，装进示例库（和导入页「选已有的词典」同一条路）
            if let sudachi = ProcessInfo.processInfo.environment["JPCORPUS_SMOKE_SUDACHI"], !sudachi.isEmpty, let client = app.core {
                do {
                    _ = try await client.tokenizerInstall(from: sudachi)
                    note("分词词典：装好了")
                } catch {
                    note("分词词典装不上：\(error.localizedDescription)")
                }
            }
            app.showFurigana = true
            await settle(2)
            let rubyCount = app.furigana.values.map { $0.count }.reduce(0, +)
            note("振假名：\(rubyCount) 段，错误：\(app.furiganaError ?? "无")")
            shot("03b-furigana", into: out, note: note)

            // 全屏歌词
            app.showStage = true
            await settle(2)
            shot("03c-stage", into: out, note: note)
            app.showStage = false
            await settle(1)

            // 首页：刚才那次试听到行尾自动暂停，播放记录里有了，「继续听」应该是这首
            app.section = .home
            await settle(2)
            try requirePlayerBarHeight()
            shot("04a-home", into: out, note: note)

            // 曲库三种看法
            app.section = .library
            for (name, mode) in [("04b-library-songs", "歌曲"), ("04c-library-albums", "专辑"), ("04d-library-artists", "歌手")] {
                app.libraryMode = LibraryMode(rawValue: mode) ?? .songs
                await settle(1.5)
                shot(name, into: out, note: note)
            }
            app.libraryMode = .graph
            let lyricsWereVisible = app.showLyrics; app.showLyrics = false
            for _ in 0..<60 { if !app.visualizations.graph.nodes.isEmpty && !app.visualizations.graphBusy { break }; await settle(0.2) }
            try require(!app.visualizations.graph.nodes.isEmpty && !app.visualizations.graphBusy,"graph","nodes=\(app.visualizations.graph.nodes.count) links=\(app.visualizations.graph.links.count)")
            await settle(0.5); shot("04g-graph",into: out,note: note)
            app.libraryMode = .timeline
            await settle(1)
            try require(!app.visualizations.timeline.rows.isEmpty,"timeline","rows=\(app.visualizations.timeline.rows.count) releases=\(app.visualizations.releases.count)")
            shot("04h-timeline",into: out,note: note)
            app.libraryMode = .cloud
            for _ in 0..<60 { if !app.visualizations.words.isEmpty && !app.visualizations.placed.isEmpty && !app.visualizations.cloudBusy { break }; await settle(0.2) }
            try require(!app.visualizations.words.isEmpty && !app.visualizations.placed.isEmpty,"cloud","words=\(app.visualizations.words.count) placed=\(app.visualizations.placed.count)")
            await settle(0.5); shot("04i-cloud",into: out,note: note)
            app.libraryMode = .songs; app.showLyrics = lyricsWereVisible

            // 歌手详情、歌曲详情
            if let person = try? await app.core?.peopleDirectory().first(where: { $0.performer > 0 }) {
                app.show(.person(person.id))
                await settle(2)
                shot("04e-person", into: out, note: note)
            }
            app.libraryPath = []
            app.show(.song("004"))
            await settle(2)
            try requirePlayerBarHeight()
            shot("04f-song", into: out, note: note)

            // ⌘K 全库搜索
            app.quickSearchSeed = "夜"
            app.showQuickSearch = true
            await settle(2)
            shot("04g-quick-search", into: out, note: note)
            app.showQuickSearch = false
            await settle(0.8)

            // 分析：词频 → 存一条档并重跑 → 存档页 → 语料质量
            app.section = .analysis
            UserDefaults.standard.set(0, forKey: "analysisTab")
            await settle(2)
            shot("05a-analysis", into: out, note: note)
            do {
                let q = FrequencyQuery(scope: AnalysisScope(unit: .distinctLines, jpOnly: true), pos: "NOUN", limit: 50)
                if let run = try await app.core?.archiveFrequency(q, label: "冒烟 · 名词") {
                    let cmp = try await app.core?.analysisRerun(run.id)
                    note("存档 #\(run.id)，重跑结果相同：\(cmp?.resultIdentical == true)")
                }
            } catch {
                note("存档失败：\(error.localizedDescription)")
            }
            UserDefaults.standard.set(2, forKey: "analysisTab")
            await settle(1.5)
            shot("05b-archive", into: out, note: note)
            UserDefaults.standard.set(3, forKey: "analysisTab")
            await settle(1.5)
            shot("05c-quality", into: out, note: note)
            UserDefaults.standard.set(0, forKey: "analysisTab")

            guard let exportClient = app.core else { throw SmokeFailure.step("stats-export-client") }
            let report = try await exportClient.statsReport(StatsFilter())
            let exportURL = FileManager.default.temporaryDirectory.appendingPathComponent("stats-smoke-\(UUID().uuidString).txt")
            defer { try? FileManager.default.removeItem(at: exportURL) }
            try await exportClient.exportText(path: exportURL.path, content: report.text)
            let exported = try Data(contentsOf: exportURL)
            try require(!exported.isEmpty && String(data: exported, encoding: .utf8) == report.text, "stats-export", "bytes=\(exported.count)")

            app.section = .anki
            for _ in 0..<100 {
                if app.anki.status != nil { break }
                await settle(0.1)
            }
            await settle(0.5)
            shot("05d-anki", into: out, note: note)
            let ankiStatus = app.anki.status
            let ankiShot = FileManager.default.fileExists(atPath: out.appendingPathComponent("05d-anki.png").path)
            try require(ankiShot && ankiStatus?.connected == false && !(ankiStatus?.message.isEmpty ?? true), "anki-page", "connected=\(ankiStatus?.connected == true) screenshot=\(ankiShot)")

            // 词典：现做一本只有一个词条的 Yomitan 词典（解开的目录），经界面那条路导入，再试查「夜」
            app.section = .dictionaries
            do {
                let dict = FileManager.default.temporaryDirectory.appendingPathComponent("JPCorpusSmokeDict", isDirectory: true)
                try? FileManager.default.removeItem(at: dict)
                try FileManager.default.createDirectory(at: dict, withIntermediateDirectories: true)
                try #"{"title":"冒烟辞書","revision":"1","format":3,"author":"smoke"}"#.write(to: dict.appendingPathComponent("index.json"), atomically: true, encoding: .utf8)
                try #"[["夜","よる","n","",0,["日が沈んでから昇るまでの間"],1,""]]"#.write(to: dict.appendingPathComponent("term_bank_1.json"), atomically: true, encoding: .utf8)
                if let client = app.core {
                    try await client.dictImport([dict.path])
                    var progress = try await client.dictImportProgress()
                    while !progress.finished {
                        await settle(0.3)
                        progress = try await client.dictImportProgress()
                    }
                    var lines: [String] = []
                    for r in progress.result?.results ?? [] {
                        if let summary = r.summary {
                            lines.append("\(summary.title) \(summary.terms) 条")
                        } else {
                            lines.append(r.error ?? "?")
                        }
                    }
                    note("词典导入：" + lines.joined(separator: "；") + "，错误：" + (progress.error ?? "无"))
                    let anki = try await client.ankiStatus()
                    note("Anki：\(anki.connected ? "连上了" : "连不上（\(anki.message)）")")
                }
            } catch {
                note("词典失败：\(error.localizedDescription)")
            }
            app.lookupSeed = "夜に"
            app.dictionaryVersion += 1
            await settle(2.5)
            shot("06a-dictionaries", into: out, note: note)

            // 刮削页：只打开看看，不点（CI 上别去打 MusicBrainz）
            app.section = .scrape
            await settle(2)
            shot("06b-scrape", into: out, note: note)

            app.section = .importer
            await settle(2)
            shot("06c-import", into: out, note: note)

            // 导入之后看曲库列表，不要停在上面推进去的详情页
            app.libraryPath = []

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
