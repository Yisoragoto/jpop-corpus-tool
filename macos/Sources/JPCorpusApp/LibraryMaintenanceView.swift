import SwiftUI
import UniformTypeIdentifiers
import JPCore
import JPCorpusModel

@MainActor
struct LibraryManageButton: View {
    @State private var open = false
    var body: some View {
        Button { open = true } label: { Image(systemName: "slider.horizontal.3") }.buttonStyle(PlayerGlyphStyle()).help("资料库管理")
            .sheet(isPresented: $open) { LibraryMaintenanceView() }
    }
}

@MainActor
struct LibraryMaintenanceView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var tab = 0
    private var model: LibraryModel { app.library }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack { Text("资料库管理").font(.title2.bold()); Spacer(); Button("完成") { dismiss() } }
            Picker("", selection: $tab) {
                Text("找回音频").tag(0); Text("新歌与文件夹").tag(1); Text("维护与迁移").tag(2); Text("收听记录").tag(3)
            }.pickerStyle(.segmented)
            Group {
                switch tab {
                case 0: AudioRepairView()
                case 1: WatchedFoldersView()
                case 2: MaintenanceJobsView()
                default: ListeningHistoryView()
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
            if !model.note.isEmpty { Text(model.note).font(.caption).textSelection(.enabled) }
            if let error = model.errorMessage { Text(error).foregroundStyle(.orange).textSelection(.enabled) }
        }.padding(24).frame(width: 760, height: 580).glass(.sidebar)
    }
}

@MainActor
struct AudioRepairView: View {
    @Environment(AppModel.self) private var app
    @State private var pick = FilePickState<RepairPick>()
    @State private var isFolder = false
    private enum RepairPick { case folder, song(String) }
    private var model: LibraryModel { app.library }
    var body: some View {
        VStack(alignment: .leading) {
            HStack {
                Text(model.missing.isEmpty ? "所有曲目的音频文件都在。" : "\(model.missing.count) 首找不到音频。选择新路径，确认后才写库。")
                Spacer(); Button("重新检查") { Task { await model.loadMissing() } }
            }
            Button("扫描文件夹自动配对…") { isFolder = true; pick.present(.folder) }
            List(model.missing, id: \.songId) { song in
                VStack(alignment: .leading, spacing: 5) {
                    HStack { Text("\(song.title) · \(song.artist)").bold(); Spacer(); Button("选择文件…") { isFolder = false; pick.present(.song(song.songId)) } }
                    Text("原路径：\(song.audioPath)").font(.caption).foregroundStyle(.secondary)
                    if let path = model.picks[song.songId] {
                        HStack { Text("→ \(path)").font(.caption); Spacer(); Button("不改") { model.picks.removeValue(forKey: song.songId) } }
                    }
                    if let error = model.repairErrors[song.songId] { Text(error).font(.caption).foregroundStyle(.orange) }
                }.textSelection(.enabled)
            }.scrollContentBackground(.hidden)
            Button("确认更新 \(model.picks.count) 首") { Task { if await model.applyRelinks() { await app.reloadTracks() } } }.disabled(model.picks.isEmpty)
        }.disabled(model.busy)
            .task { await model.loadMissing() }
            .fileImporter(isPresented: $pick.isPresented, allowedContentTypes: isFolder ? [.folder] : LibraryAdmin.audioExtensions.compactMap { UTType(filenameExtension: $0) }) { result in
                let kind = pick.finish()
                switch result {
                case .failure(let error): model.errorMessage = error.localizedDescription
                case .success(let url):
                    switch kind {
                    case .folder: Task { await model.suggest(url.path) }
                    case .song(let id): model.picks[id] = url.path
                    case nil: model.errorMessage = "文件选择的用途已丢失，请重新选择"
                    }
                }
            }
    }
}

@MainActor
struct WatchedFoldersView: View {
    @Environment(AppModel.self) private var app
    private var model: LibraryModel { app.library }
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                NewSongsNotice()
                Button(model.checkingNewAudio ? "正在查…" : "重新检查新歌") { Task { await model.checkNewAudio() } }.disabled(model.busy || model.checkingNewAudio)
                if let found = model.found {
                    let unknown = found.scan.items.filter { $0.action.kind != .alreadyImported }
                    if !unknown.isEmpty {
                        Button("查看这 \(unknown.count) 个文件的计划") {
                            Task {
                                guard let client = app.client else { return }
                                await app.importer.scan(unknown.map { URL(fileURLWithPath: $0.path) }, client: client)
                                app.section = .importer
                            }
                        }.disabled(app.importer.importing || app.importer.scanning)
                    }
                    ForEach(found.folders, id: \.path) { folder in
                        HStack {
                            VStack(alignment: .leading) {
                                Text(folder.path).textSelection(.enabled)
                                Text("\(why(folder.reason)) · 深度 \(folder.depth) · 库中 \(folder.songs) 首 · 未知 \(folder.unknown) 个 · 新歌 \(folder.new) 首").font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer(); Button("别再看") { Task { await model.ignoreFolder(folder.path) } }
                        }
                    }
                }
                ForEach(model.ignores.folders, id: \.self) { path in
                    HStack { Text(path).foregroundStyle(.secondary); Spacer(); Button("恢复检查") { Task { await model.restoreFolder(path) } } }
                }
                if !model.ignores.files.isEmpty {
                    Text("已忽略 \(model.ignores.files.count) 个文件").font(.caption)
                    Button("清除文件忽略清单") { model.ignores.files = []; Task { await model.checkNewAudio() } }
                }
            }
        }
    }
    private func why(_ reason: String) -> String { ["library": "语料库目录", "libraryAudio": "库中音频所在目录", "parent": "共同上级目录"][reason] ?? reason }
}

@MainActor
struct NewSongsNotice: View {
    @Environment(AppModel.self) private var app
    @State private var dismissed = false
    @State private var summary = ""
    @State private var missingLyrics: [String] = []
    @State private var filling = false
    @State private var lyricsNote = ""
    private var model: LibraryModel { app.library }
    private var items: [PlanItem] { model.found.map { NewSongs.newItems($0.scan) } ?? [] }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !items.isEmpty && !dismissed {
                Text("发现 \(items.count) 首新歌").font(.headline)
                Text(NewSongs.describeNewSongs(items)).font(.callout)
                if let found = model.found { Text(NewSongs.foldersOf(found, items).joined(separator: "、")).font(.caption).foregroundStyle(.secondary) }
                if let reason = NewSongs.reviewReason(items) { Text(reason).font(.caption).foregroundStyle(.secondary) }
                HStack {
                    Button("查看计划") { Task { await review() } }
                    if NewSongs.reviewReason(items) == nil {
                        Button("导入") { Task { await directImport() } }
                    }
                    Button("别再提") { model.ignoreFiles(items.map(\.path)) }
                    Button("稍后") { dismissed = true }
                }.disabled(model.busy || app.importer.importing || app.importer.scanning)
            }
            if !summary.isEmpty { HStack { Text(summary).font(.callout); Button("关闭") { summary = ""; missingLyrics = []; lyricsNote = "" } }.disabled(filling) }
            if !missingLyrics.isEmpty && lyricsNote.isEmpty {
                Button("在线补缺失歌词") { Task { await fillLyrics() } }.disabled(filling)
            }
            if !lyricsNote.isEmpty { Text(lyricsNote).font(.caption) }
            if let error = app.importer.errorMessage { Text(error).font(.caption).foregroundStyle(.orange) }
        }
        .onChange(of: model.found?.scan.summary.new) { _, _ in dismissed = false }
    }
    private func review() async {
        guard let client = app.client else { return }
        await app.importer.scan(items.map { URL(fileURLWithPath: $0.path) }, client: client)
        app.section = .importer
    }
    private func directImport() async {
        guard let client = app.client else { return }
        let planned = items
        await app.importer.scan(planned.map { URL(fileURLWithPath: $0.path) }, client: client)
        guard app.importer.errorMessage == nil, let scan = app.importer.plan else { return }
        let fresh = NewSongs.newItems(scan)
        guard !fresh.isEmpty else { model.errorMessage = "这几个文件现在已经不算新歌了，没有再导一遍"; dismissed = true; return }
        app.importer.tokenize = true
        await app.importer.runImport(app: app)
        if let report = app.importer.lastResult {
            let r = NewSongs.summarizeImport(report, fresh)
            missingLyrics = r.withoutLyrics
            summary = "导入 \(r.imported) 首 · \(r.artists.joined(separator: "、"))" + (r.cancelled ? " · 已停止" : "") + r.failed.map { "\n\($0.title)：\($0.error)" }.joined()
        }
        await model.checkNewAudio()
    }
    private func fillLyrics() async {
        guard let client = app.core else { return }
        filling = true; lyricsNote = "正在补歌词…"
        defer { filling = false }
        do {
            try await client.lyricsFillStart(songIds: missingLyrics)
            while !Task.isCancelled {
                let job = try await client.lyricsFillProgress()
                if job.finished {
                    if let error = job.error { model.errorMessage = error }
                    if let r = job.result { lyricsNote = "补上 \(r.upgraded + r.lineFilled) 首，\(r.notFound) 首没找到，\(r.failed) 首失败" }
                    else { lyricsNote = "补歌词已结束" }
                    missingLyrics = []; await app.reloadTracks(); app.library.version += 1; return
                }
                try await Task.sleep(for: .milliseconds(350))
            }
        } catch { model.errorMessage = error.localizedDescription; lyricsNote = "" }
    }
}

@MainActor
struct MaintenanceJobsView: View {
    @Environment(AppModel.self) private var app
    @State private var picking = false
    private var model: LibraryModel { app.library }
    var body: some View {
        @Bindable var m = model
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack {
                    Button("回填缺失时长") { Task { _ = await model.backfillDurations() } }
                    Button("补专辑封面") { Task { _ = await model.fillAlbumArtwork() } }
                }
                Text("只回填空时长；专辑封面使用库里已有的曲目图片。").font(.caption).foregroundStyle(.secondary)
                Divider()
                Text("从另一个库迁移").font(.headline)
                Button("选择源语料库…") { picking = true }
                if let plan = model.migrationPlan {
                    Text(model.migrationSource).textSelection(.enabled)
                    Text("源库 \(plan.sourceSongs) 首 · 新增 \(plan.newSongs) 首 · 重复 \(plan.duplicateSongs) 首\n\(plan.lyricLines) 行歌词 · \(plan.tokens) 词 · \(plan.people) 人 · \(plan.albums) 张专辑\n\(plan.plays) 条播放 · \(plan.favorites) 条收藏 · \(plan.corrections) 条校正 · \(plan.dictTerms) 条词典词条")
                    ForEach(plan.samples, id: \.self) { Text($0).font(.caption) }
                    Toggle("迁移收听记录和收藏", isOn: $m.migrationOptions.history)
                    Toggle("迁移词典", isOn: $m.migrationOptions.dictionaries)
                    if plan.targetHasDictionaries { Text("当前库已有词典表，不会覆盖。").font(.caption) }
                    Text("音频文件不复制，仍指向原处。封面、照片、歌词和分词词典会复制；已有曲目跳过。").font(.caption).foregroundStyle(.secondary)
                    Button("确认迁移") { Task { _ = await model.migrateConfirmed() } }
                }
                if let job = model.job, job.running { ProgressView(); Text(job.stage) }
            }
        }.disabled(model.busy || model.job?.running == true)
            .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
                switch result {
                case .failure(let error): model.errorMessage = error.localizedDescription
                case .success(let url): Task { await model.previewMigration(url.path) }
                }
            }
            .onChange(of: model.version) { _, _ in Task { await app.reloadTracks(); app.homeVersion += 1 } }
            .task {
                repeat {
                    await model.pollJob()
                    try? await Task.sleep(for: .milliseconds(350))
                } while !Task.isCancelled
            }
    }
}

@MainActor
struct ListeningHistoryView: View {
    @Environment(AppModel.self) private var app
    @State private var most = false
    @State private var tracks: [PlayedTrack] = []
    var body: some View {
        VStack {
            Picker("收听", selection: $most) { Text("最近播放").tag(false); Text("播放最多").tag(true) }.pickerStyle(.segmented)
            List(tracks) { t in
                Button { app.show(.song(t.songId)) } label: {
                    HStack { Text("\(t.title) · \(t.artist)"); Spacer(); Text("\(t.playCount) 次 · \(InterfaceLogic.formatRelative(t.lastPlayed))").foregroundStyle(.secondary) }
                }.buttonStyle(.plain)
            }
        }.task(id: most) {
            do {
                if most { tracks = try await app.library.client?.mostPlayed(minListenedSec: 30, limit: 20) ?? [] }
                else { tracks = try await app.library.client?.recentlyPlayed(limit: 20) ?? [] }
            }
            catch { app.library.errorMessage = error.localizedDescription }
        }
    }
}
