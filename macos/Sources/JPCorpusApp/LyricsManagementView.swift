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

    init(songId: String, timing: Bool = false, initialTab: Int? = nil) {
        _model = State(initialValue: LyricsModel(songId: songId))
        _tab = State(initialValue: initialTab ?? (timing ? 3 : 0))
    }

    var body: some View {
        @Bindable var model = model
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("歌词管理").font(.title2.bold())
                if model.dirty { Text("有未保存的改动").font(.caption).foregroundStyle(.orange) }
                Spacer()
                if model.busy { ProgressView().controlSize(.small) }
                Button("关闭", action: requestClose)
                    .disabled(model.busy)
            }
            Picker("功能", selection: $tab) {
                Text("逐行编辑").tag(0)
                Text("来源与版本").tag(1)
                Text("在线歌词").tag(2)
                Text("卡拉 OK 打轴").tag(3)
            }
            .pickerStyle(.segmented)
            .disabled(model.dirty || model.busy || model.songId.isEmpty)
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
                    .disabled(model.busy || (tab == 0 ? model.editSummary == "没有改动" || !model.invalidEditRows.isEmpty : !model.dirty))
                }
            }
        }
        .padding(20)
        .frame(minWidth: 760, idealWidth: 900, minHeight: 580, idealHeight: 720)
        .interactiveDismissDisabled(model.dirty || model.busy)
        .onExitCommand(perform: requestClose)
        .alert("放弃未保存的改动？", isPresented: $confirmClose) {
            Button("继续编辑", role: .cancel) {}
            Button("保存并返回") {
                Task {
                    guard let client = app.core else { return }
                    if tab == 3 { await model.saveTiming(client) } else { await model.saveLines(client) }
                    if !model.dirty { dismiss() }
                }
            }
            Button("放弃并关闭", role: .destructive) { dismiss() }
        } message: { Text("库内已保存的版本不受影响。") }
        .fileImporter(isPresented: $importing, allowedContentTypes: ["lrc", "txt", "yrc", "qrc", "krc", "ttml"].map { UTType(filenameExtension: $0) ?? UTType(importedAs: "org.jpcorpus.lyrics.\($0)") }, allowsMultipleSelection: false) { result in
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
                Text("时间写成 1:02.50；留空表示无时间，正文空白的行保存时去掉。").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("导入 .lrc 文件") { importing = true }.disabled(model.busy || model.dirty)
                Button("添加一行") { model.edits.append(LyricDraft()) }.disabled(model.busy)
            }
            Text(model.editSummary).font(.caption).foregroundStyle(.secondary)
            if !model.invalidEditRows.isEmpty {
                Text("第 \(model.invalidEditRows.map { String($0 + 1) }.joined(separator: "、")) 行的时间认不出来").foregroundStyle(.orange)
            }
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach($model.edits) { $line in
                        HStack {
                            TextField("时间", text: $line.time).frame(width: 100).monospacedDigit()
                            TextField("歌词正文", text: $line.text)
                            Button("＋") { if let index = model.edits.firstIndex(where: { $0.id == line.id }) { model.insertAfter(index) } }
                                .help("在下面插一行")
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
                    LyricsLineageDetails(model: model)
                    ForEach(model.versions) { version in
                        HStack(alignment: .top) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text("第 \(version.versionNo) 版\(version.isCurrent ? " · 当前" : "")").bold()
                                Text("\(version.sourceType) · \(version.sourceName) · \(version.lineCount) 行 · \(version.importedAt)").font(.caption).foregroundStyle(.secondary)
                                Text("完整度 \(version.completeness.map { String(format: "%.0f%%", $0 * 100) } ?? "—")").font(.caption)
                                if !version.note.isEmpty { Text(version.note).font(.caption) }
                            }
                            Spacer()
                            Button(model.previewingVersion == version.id ? "收起原文" : "原文") { Task { if let client = app.core { await model.preview(version.id, client: client) } } }
                                .disabled(!version.hasRawText || model.busy)
                            Button("恢复为新版") { Task { if let client = app.core { await model.restore(version.id, client: client) } } }
                                .disabled(version.isCurrent || !version.hasRawText || model.busy)
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
                appleSettings
                HStack {
                    Button("补齐本首") { Task { if let client = app.core { await model.fillOne(client) } } }
                        .disabled(model.songId.isEmpty || !model.timing.isEmpty)
                    Button("批量补齐缺失（\(model.missing.count) 首）") { start(all: true) }
                    Button("全库升级逐字歌词") { start(all: true, upgrade: true) }
                }.disabled(model.running || model.busy)
                if let job = model.job, job.running || job.finished {
                    HStack {
                        if job.running { ProgressView(value: job.fraction ?? 0).frame(width: 160) }
                        Text("\(job.done) / \(job.total) · \(job.stage)")
                        Spacer()
                        if job.running {
                            Button(job.cancelled ? "正在停止…" : "取消任务") { Task { if let client = app.core { await model.cancel(client) } } }
                                .disabled(job.cancelled)
                        } else {
                            Text(job.cancelled ? "已取消" : "已结束").foregroundStyle(.secondary)
                        }
                    }
                    if let error = job.error { Text(error).foregroundStyle(.orange) }
                    if let result = job.result {
                        if !result.error.isEmpty { Text(result.error).foregroundStyle(.orange) }
                        Text("升级 \(result.upgraded) · 补逐行 \(result.lineFilled) · 跳过 \(result.skipped) · 未找到 \(result.notFound) · 失败 \(result.failed)").font(.caption)
                        ForEach(Array(result.results.enumerated()), id: \.offset) { _, item in
                            Text("\(item.title) · \(statusLabel(item.status)) \(item.message)").font(.caption).textSelection(.enabled)
                        }
                    }
                }
                Divider()
                Button("搜索本首时间歌词（逐行 / 逐字）") { Task { if let client = app.core { await model.search(client) } } }
                    .disabled(model.busy || model.songId.isEmpty)
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
                            Text(candidate.precisionLabel).font(.caption)
                            if candidate.precisionLabel != "含真实逐字时间" { Text("此候选没有真实逐字时间；采用后按逐行或纯文本显示。").font(.caption).foregroundStyle(.secondary) }
                            Text(candidate.matchNote).font(.caption).foregroundStyle(.secondary)
                            Text(candidate.preview.prefix(5).joined(separator: "\n")).textSelection(.enabled)
                        }.padding(12).background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
                    }
                }
            }.padding(4)
        }
    }

    private var appleSettings: some View {
        @Bindable var model = model
        return DisclosureGroup("Apple Music 来源设置") {
            VStack(alignment: .leading, spacing: 8) {
                Toggle("Apple Music（默认关闭）", isOn: $model.sources.apple)
                Text("Apple Music 官方 API 不提供歌词正文或逐字时间；此开关仅用于普通元数据刮削，不作为歌词来源。").font(.caption).foregroundStyle(.secondary)
                TextField("商店地区（jp / cn / us）", text: $model.sources.appleStorefront)
                Text("Developer Token · \(model.clearDeveloperToken ? "保存时清除" : model.sources.appleDeveloperTokenSet ? "已设置" : "未设置")").font(.caption)
                SecureField("Developer Token（留空保留）", text: $model.developerToken)
                    .onChange(of: model.developerToken) { _, value in if !value.isEmpty { model.clearDeveloperToken = false } }
                Button("清除 Developer Token") { model.developerToken = ""; model.clearDeveloperToken = true }
                Text("Music User Token · \(model.clearMusicUserToken ? "保存时清除" : model.sources.appleMusicUserTokenSet ? "已设置" : "未设置")").font(.caption)
                SecureField("Music User Token（留空保留）", text: $model.musicUserToken)
                    .onChange(of: model.musicUserToken) { _, value in if !value.isEmpty { model.clearMusicUserToken = false } }
                Button("清除 Music User Token") { model.musicUserToken = ""; model.clearMusicUserToken = true }
                Text("令牌仅保存在后端内存，重启后需重新配置；清除需保存，输入框不回显。").font(.caption).foregroundStyle(.secondary)
                Button("保存来源设置") { Task { if let client = app.core { await model.saveSources(client) } } }
            }.padding(8).disabled(model.busy || model.running)
        }
    }

    private func start(all: Bool, upgrade: Bool = false) {
        Task { if let client = app.core { await model.start(client, all: all, upgrade: upgrade) } }
    }

    private var timingEditor: some View {
        LyricsTimingEditor(model: model, leave: requestClose)
    }
    private func requestClose() {
        if model.dirty { confirmClose = true } else { dismiss() }
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
