import JPCore
import JPCorpusModel
import SwiftUI
import UniformTypeIdentifiers

/// 各入口共用一个面板，每个 View 只持有一种 presentation。
@MainActor
struct LyricsToolsButton: View {
    let songId: String
    var timing = false
    var onDismiss: (() -> Void)? = nil
    @State private var showing = false
    var body: some View {
        Button(timing ? "打轴" : "歌词管理") { showing = true }
            .sheet(isPresented: $showing, onDismiss: { onDismiss?() }) {
                LyricsManagementView(songId: songId, timing: timing)
                    .id(songId)
            }
    }
}

@MainActor
struct LyricsManagementView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var model: LyricsModel
    @State private var tab: Int
    @State private var importing = false
    @State private var confirmClose = false
    @FocusState private var timingFocused: Bool

    init(songId: String, timing: Bool = false) {
        _model = State(initialValue: LyricsModel(songId: songId))
        _tab = State(initialValue: timing ? 3 : 0)
    }

    var body: some View {
        @Bindable var model = model
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("歌词管理").font(.title2.bold())
                if model.dirty { Text("有未保存的改动").font(.caption).foregroundStyle(.orange) }
                Spacer()
                if model.busy { ProgressView().controlSize(.small) }
                Button("关闭") { if model.dirty { confirmClose = true } else { dismiss() } }
                    .disabled(model.busy)
            }
            Picker("功能", selection: $tab) {
                Text("逐行编辑").tag(0)
                Text("来源与版本").tag(1)
                Text("在线歌词").tag(2)
                Text("卡拉 OK 打轴").tag(3)
            }
            .pickerStyle(.segmented)
            .disabled(model.dirty || model.busy)
            if model.dirty {
                HStack {
                    Text("保存或放弃草稿后可切换功能。旧版保留在版本列表中。").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("放弃草稿") { Task { if let client = app.core { await model.load(client) } } }
                        .disabled(model.busy)
                }
            }
            if let error = model.errorMessage { Text(error).foregroundStyle(.orange).textSelection(.enabled) }
            if let message = model.message { Text(message).font(.callout).foregroundStyle(.secondary) }
            Group {
                switch tab {
                case 0: lineEditor
                case 1: versions
                case 2: online
                default: timingEditor
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            if tab == 0 || tab == 3 {
                HStack {
                    TextField("修订说明（可选）", text: $model.note)
                    Button(tab == 0 ? "保存歌词" : "保存打轴") {
                        Task {
                            guard let client = app.core else { return }
                            if tab == 0 { await model.saveLines(client) } else { await model.saveTiming(client) }
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut("s", modifiers: .command)
                    .disabled(model.busy || !model.dirty)
                }
            }
        }
        .padding(20)
        .frame(minWidth: 760, idealWidth: 900, minHeight: 580, idealHeight: 720)
        .interactiveDismissDisabled(model.dirty || model.busy)
        .alert("放弃未保存的改动？", isPresented: $confirmClose) {
            Button("继续编辑", role: .cancel) {}
            Button("放弃并关闭", role: .destructive) { dismiss() }
        } message: { Text("库内已保存的版本不受影响。") }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.init(filenameExtension: "lrc") ?? .plainText, .plainText], allowsMultipleSelection: false) { result in
            switch result {
            case .failure(let error): model.errorMessage = error.localizedDescription
            case .success(let urls):
                Task {
                    guard let client = app.core else { model.errorMessage = ImportModel.noLibrary; return }
                    await model.importSelection(.success(urls), client: client)
                }
            }
        }
        .task {
            guard let client = app.core else { model.errorMessage = ImportModel.noLibrary; return }
            await model.load(client)
            while !Task.isCancelled {
                do { try await Task.sleep(for: .milliseconds(300)) }
                catch { break }
                await model.poll(client)
            }
        }
        .onChange(of: model.revision) { _, _ in
            Task {
                await app.reloadTracks()
                await app.player.refreshLyrics(songId: model.songId)
                await app.loadFurigana()
                app.homeVersion += 1
            }
        }
    }

    private var lineEditor: some View {
        @Bindable var model = model
        return VStack(alignment: .leading) {
            HStack {
                Text("时间可填秒数或 分:秒.毫秒；留空表示纯文本。").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("导入 .lrc 文件") { importing = true }.disabled(model.busy || model.dirty)
                Button("添加一行") { model.edits.append(LyricDraft()) }.disabled(model.busy)
            }
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach($model.edits) { $line in
                        HStack {
                            TextField("时间", text: $line.time).frame(width: 100).monospacedDigit()
                            TextField("歌词正文", text: $line.text)
                            Button(role: .destructive) { model.edits.removeAll { $0.id == line.id } } label: { Image(systemName: "minus.circle") }
                        }
                    }
                }.padding(4)
            }
            .disabled(model.busy)
        }
    }

    private var versions: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Button("导入 .lrc 文件") { importing = true }
                Button("重新校验") { Task { if let client = app.core { await model.revalidate(client) } } }
            }.disabled(model.busy)
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(model.versions) { version in
                        HStack(alignment: .top) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text("第 \(version.versionNo) 版\(version.isCurrent ? " · 当前" : "")").bold()
                                Text("\(version.sourceType) · \(version.sourceName) · \(version.lineCount) 行 · \(version.importedAt)").font(.caption).foregroundStyle(.secondary)
                                if !version.note.isEmpty { Text(version.note).font(.caption) }
                            }
                            Spacer()
                            Button("原文") { Task { if let client = app.core { await model.preview(version.id, client: client) } } }
                            Button("恢复为新版") { Task { if let client = app.core { await model.restore(version.id, client: client) } } }
                                .disabled(version.isCurrent || model.busy)
                        }
                        Divider()
                    }
                    if let raw = model.rawPreview { Text(raw).font(.body.monospaced()).textSelection(.enabled) }
                }
            }
        }
    }

    private var online: some View {
        @Bindable var model = model
        return ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text("批量处理保留人工修改；已有歌词可搜索候选后逐首采用。没有真实逐字数据时保持逐行。").font(.callout).foregroundStyle(.secondary)
                HStack {
                    Toggle("网易", isOn: $model.sources.netease)
                    Toggle("QQ", isOn: $model.sources.qq)
                    Toggle("酷狗", isOn: $model.sources.kugou)
                    Toggle("AMLL", isOn: $model.sources.amll)
                    Toggle("LRCLIB", isOn: $model.sources.lrclib)
                    Button("保存来源") { Task { if let client = app.core { await model.saveSources(client) } } }
                }.disabled(model.busy || model.running)
                HStack {
                    Button("补齐本首") { start(all: false) }
                        .disabled(!model.missing.contains { $0.songId == model.songId })
                    Button("批量补齐缺失（\(model.missing.count) 首）") { start(all: true) }
                    Button("全库升级逐字歌词") { start(all: true, upgrade: true) }
                }.disabled(model.running || model.busy)
                if let job = model.job, job.running || job.finished {
                    HStack {
                        if job.running { ProgressView(value: job.fraction ?? 0).frame(width: 160) }
                        Text("\(job.done) / \(job.total) · \(job.stage)")
                        Spacer()
                        if job.running {
                            Button("取消任务") { Task { if let client = app.core { await model.cancel(client) } } }
                        } else {
                            Text(job.cancelled ? "已取消" : "已结束").foregroundStyle(.secondary)
                        }
                    }
                    if let result = job.result {
                        Text("升级 \(result.upgraded) · 补逐行 \(result.lineFilled) · 跳过 \(result.skipped) · 未找到 \(result.notFound) · 失败 \(result.failed)").font(.caption)
                        ForEach(Array(result.results.enumerated()), id: \.offset) { _, item in
                            Text("\(item.title) · \(statusLabel(item.status)) \(item.message)").font(.caption).textSelection(.enabled)
                        }
                    }
                }
                Divider()
                Button("搜索本首时间歌词（逐行 / 逐字）") { Task { if let client = app.core { await model.search(client) } } }
                    .disabled(model.busy)
                if let result = model.searchResult {
                    if result.candidates.isEmpty { Text("没有找到可采用的候选").foregroundStyle(.secondary) }
                    ForEach(result.warnings, id: \.self) { warning in
                        Text("\(warning.source)：\(warning.message)").font(.caption).foregroundStyle(.orange)
                    }
                    ForEach(result.candidates) { candidate in
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                Text("\(candidate.source) · \(candidate.format) · \(candidate.timedLineCount) / \(candidate.lineCount) 行逐字").bold()
                                Spacer()
                                Button("采用") { Task { if let client = app.core { await model.apply(candidate.id, client: client) } } }.disabled(model.busy)
                            }
                            Text("\(candidate.title) · \(candidate.artist)")
                            Text(candidate.matchNote).font(.caption).foregroundStyle(.secondary)
                            Text(candidate.preview.joined(separator: "\n")).textSelection(.enabled)
                        }.padding(12).background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
                    }
                }
            }.padding(4)
        }
    }

    private func start(all: Bool, upgrade: Bool = false) {
        Task { if let client = app.core { await model.start(client, all: all, upgrade: upgrade) } }
    }

    private var timingEditor: some View {
        @Bindable var model = model
        let ready = app.player.track?.id == model.songId
        return VStack(alignment: .leading, spacing: 12) {
            HStack {
                Button(ready && app.player.isPlaying ? "暂停" : "播放本首") {
                    Task {
                        if ready { app.player.toggle() } else { await app.play(songId: model.songId) }
                    }
                }
                Text(formatTime(app.player.position)).monospacedDigit()
                Picker("打点", selection: $model.tapMode) {
                    Text("按住").tag(TimingTapMode.hold)
                    Text("点按").tag(TimingTapMode.tap)
                }.frame(width: 130)
                Text("提前量（毫秒）")
                TextField("150", value: $model.advanceMilliseconds, format: .number).frame(width: 70)
                Spacer()
                Button("撤销") { model.undo() }.disabled(!model.canUndo)
                Button("重做") { model.redo() }.disabled(!model.canRedo)
            }
            Text("选单位后用空格打点。按住记起止；点按时前一个单位唱到下一次按下，行尾按住记结尾。拆开后需重新打点。").font(.caption).foregroundStyle(.secondary)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 10) {
                    ForEach(Array(model.timing.enumerated()), id: \.offset) { lineIndex, line in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text("第 \(lineIndex + 1) 行").font(.caption).foregroundStyle(.secondary)
                                Button("试听") { Task { await app.player.seek(to: max(0, (line.timeSec ?? 0) - 1.5)); app.player.play() } }.disabled(!ready)
                            }
                            ScrollView(.horizontal) {
                                HStack(spacing: 4) {
                                    ForEach(Array(line.units.enumerated()), id: \.offset) { unitIndex, unit in
                                        Button {
                                            model.select(line: lineIndex, unit: unitIndex)
                                            timingFocused = true
                                        } label: {
                                            VStack(spacing: 3) {
                                                Text(LyricsModel.unitText(line.text, unit: unit)).font(.title3)
                                                Text(unit.start.map { String(format: "%.2f", $0) } ?? "未打").font(.caption2.monospacedDigit())
                                                Text(unit.end.map { String(format: "%.2f", $0) } ?? "—").font(.caption2.monospacedDigit())
                                            }.padding(6)
                                        }
                                        .tint(lineIndex == model.selectedLine && unitIndex == model.selectedUnit ? Color.accentColor : Color.secondary)
                                    }
                                }
                            }
                        }
                    }
                }.padding(4)
            }
            HStack {
                Button("记开始") { model.stampStart(at: app.player.timingPosition) }
                Button("记结束并前进") { model.stampEnd(at: app.player.timingPosition) }
                Button("拆开") { model.splitUnit() }
                Button("和后一个合并") { model.mergeUnit() }
                Spacer()
                Button("起点校准") { calibrate(clear: false) }
                Button("清除校准") { calibrate(clear: true) }
            }.disabled(!ready || !model.validCursor || model.busy)
            HStack {
                TextField("开始时间", text: $model.startText).frame(width: 120)
                TextField("结束时间", text: $model.endText).frame(width: 120)
                Button("应用起止") { model.applyFields() }
                Text("可保存未完成的打轴；播放时由核心校验是否能逐字点亮。").font(.caption).foregroundStyle(.secondary)
            }.disabled(!model.validCursor || model.busy)
        }
        .focusable()
        .focused($timingFocused)
        .onKeyPress(.space, phases: [.down, .up, .repeat]) { press in
            guard ready, model.validCursor, !model.busy else { return .ignored }
            if press.phase == .down { model.keyDown(at: app.player.timingPosition) }
            else if press.phase == .up { model.keyUp(at: app.player.timingPosition) }
            return .handled
        }
    }

    private func calibrate(clear: Bool) {
        Task { if let client = app.core { await model.calibrate(client, position: app.player.timingPosition, clear: clear) } }
    }
    private func statusLabel(_ status: String) -> String {
        switch status {
        case "upgraded": return "已升级"
        case "lineFilled": return "已补逐行"
        case "notFound": return "未找到"
        case "failed": return "失败"
        case "protected": return "保留人工修改"
        case "complete": return "逐字已完整"
        case "kept": return "保留现有版本"
        case "cancelled": return "已取消"
        default: return "待逐首复核"
        }
    }
}
