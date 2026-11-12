import JPCore
import JPCorpusModel
import SwiftUI
import UniformTypeIdentifiers

/// 来源文件夹（#60），和 Windows 的 LibrarySources 一样：曲库从哪些文件夹读歌。
/// 扫描只对账，确认了才写库；移出的歌在下面「已移出的歌」里恢复或彻底删除。确认都在行内，不弹窗
@MainActor
struct LibrarySourcesView: View {
    @Environment(AppModel.self) private var app
    /// 正在确认移除哪个文件夹；`hard` 是第二步「彻底删除」
    @State private var removing: (id: Int64, hard: Bool)?
    @State private var confirmPurge = false
    @State private var dropping = false
    private var model: LibrarySourcesModel { app.sources }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("来源文件夹").font(.app(.title3).bold())
                    Text("扫描只对账，确认了才写库").font(.app(.caption)).foregroundStyle(.secondary)
                }
                if model.sources.isEmpty {
                    if model.loaded { emptyState }
                } else {
                    ForEach(model.sources) { source in sourceRow(source) }
                    Button("添加文件夹…") { Task { await pickFolders() } }
                        .help("也可以把文件夹拖进窗口")
                        .disabled(model.busy != nil)
                }
                if !model.note.isEmpty {
                    Text(model.note).font(.app(.caption)).textSelection(.enabled)
                }
                if let error = model.errorMessage {
                    Text(error).font(.app(.caption)).foregroundStyle(.orange).textSelection(.enabled)
                }
                if !model.removed.isEmpty {
                    Divider()
                    removedSection
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, 4)
        }
        .overlay {
            if dropping {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(Color.accentColor, style: StrokeStyle(lineWidth: 2, dash: [6]))
                    .allowsHitTesting(false)
            }
        }
        .dropDestination(for: URL.self) { urls, _ in
            let folders = urls.filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
            guard !folders.isEmpty else { return false }
            Task { await model.add(folders.map { SourceFolderInput(path: $0.path) }) }
            return true
        } isTargeted: { dropping = $0 }
        .task(id: app.library.version) { await model.reload() }
    }

    // MARK: - 还没有来源

    @ViewBuilder private var emptyState: some View {
        if let suggestions = model.suggestions, !suggestions.folders.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text("按库里的歌所在的位置，建议加这几个：")
                ForEach(suggestions.folders, id: \.path) { folder in
                    Toggle(isOn: Binding(
                        get: { model.pickedSuggestions.contains(folder.path) },
                        set: { on in
                            if on { model.pickedSuggestions.insert(folder.path) } else { model.pickedSuggestions.remove(folder.path) }
                        }
                    )) {
                        VStack(alignment: .leading, spacing: 1) {
                            Text(verbatim: folder.path).textSelection(.enabled)
                            Text(verbatim: L10n.t("\(folder.songs) 首") + " · " + L10n.t(folder.recursive ? "含子文件夹" : "只看这一层"))
                                .font(.app(.caption)).foregroundStyle(.secondary)
                        }
                    }
                    .toggleStyle(.checkbox)
                }
                if suggestions.looseSongs > 0 {
                    Text("另有 \(suggestions.looseSongs) 首不在这些文件夹里，照常在库里。")
                        .font(.app(.caption)).foregroundStyle(.secondary)
                }
                HStack {
                    Button("加这 \(model.pickedSuggestions.count) 个") { Task { await model.addSuggested() } }
                        .buttonStyle(.borderedProminent)
                        .disabled(model.pickedSuggestions.isEmpty || model.busy != nil)
                    Button("选别的文件夹…") { Task { await pickFolders() } }
                        .disabled(model.busy != nil)
                }
            }
        } else {
            Button { Task { await pickFolders() } } label: {
                VStack(spacing: 4) {
                    Image(systemName: "folder.badge.plus").font(.appSystem(size: 26))
                    Text("添加来源文件夹").font(.app(.headline))
                    Text("点这里选，或者把文件夹拖进窗口").font(.app(.caption)).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 22)
                .background(RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(.tertiary, style: StrokeStyle(lineWidth: 1.5, dash: [6])))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(model.busy != nil)
        }
    }

    // MARK: - 文件夹一行

    private func sourceRow(_ source: LibrarySource) -> some View {
        let scanning = model.busy == .scanning(source.id)
        return VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: source.rootPath).textSelection(.enabled)
                    Text(verbatim: LibrarySourcesModel.status(source)).font(.app(.caption)).foregroundStyle(.secondary)
                    if let error = source.lastScanError, !error.isEmpty {
                        Text(error).font(.app(.caption)).foregroundStyle(.red).textSelection(.enabled)
                    }
                }
                .opacity(source.enabled && source.present ? 1 : 0.55)
                Spacer(minLength: 8)
                Button(scanning ? "正在扫描…" : "扫描") { Task { await model.scan(source) } }
                    .disabled(model.busy != nil || !source.present)
                Button(source.enabled ? "停用" : "启用") { Task { await model.toggleEnabled(source) } }
                    .buttonStyle(.link)
                Button("移除…") { removing = (source.id, false) }
                    .buttonStyle(.link)
                    .disabled(model.busy != nil)
            }
            if let removing, removing.id == source.id { removeConfirm(source, hard: removing.hard) }
            if let review = model.review, review.sourceId == source.id { reviewBlock(review) }
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(.quaternary.opacity(0.5)))
    }

    @ViewBuilder private func removeConfirm(_ source: LibrarySource, hard: Bool) -> some View {
        let all = source.songs + source.removed
        VStack(alignment: .leading, spacing: 6) {
            if hard {
                Text("彻底删除这 \(all) 首：歌词、分词、收听记录都删掉，不能恢复（分词校正保留，音频文件不动）。")
                HStack {
                    Button("彻底删除", role: .destructive) { Task { await remove(source, mode: .delete) } }
                        .disabled(model.busy != nil)
                    Button("返回") { removing = (source.id, false) }
                }
            } else {
                Text(source.songs > 0
                     ? "它下面的 \(source.songs) 首会移出语料库，可以在「已移出的歌」里恢复。音频文件不动。"
                     : "移除这个文件夹？库里没有歌属于它。")
                HStack {
                    Button("移除") { Task { await remove(source, mode: .moveOut) } }
                        .buttonStyle(.borderedProminent)
                        .disabled(model.busy != nil)
                    if all > 0 {
                        Button("改为彻底删除…") { removing = (source.id, true) }
                            .buttonStyle(.link)
                            .foregroundStyle(.red)
                    }
                    Button("取消") { removing = nil }
                }
            }
        }
        .font(.app(.callout))
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.orange.opacity(0.1)))
    }

    // MARK: - 对账

    private func reviewBlock(_ review: SourceReview) -> some View {
        let plan = review.plan
        let applying = model.busy == .applying
        return VStack(alignment: .leading, spacing: 10) {
            if plan.nothingFound {
                Text("一个音频都没扫到。盘换了或挂错了的话，别确认移出。").foregroundStyle(.red)
            }
            if !plan.newFiles.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(plan.newFiles.count) 个新文件").bold()
                    Text(plan.newSongs > 0 ? "其中 \(plan.newSongs) 首是新歌，其余疑似重复或读不出曲名"
                         : "都不算新歌（疑似重复、格式放不了或者读不出曲名）")
                        .font(.app(.caption)).foregroundStyle(.secondary)
                    Button("去添加音乐里复核") { Task { await sendToImport(plan.newFiles) } }
                        .buttonStyle(.link)
                        .disabled(app.importer.importing || app.importer.scanning)
                }
            }
            group(.changed, review, title: "\(plan.changed.count) 首文件变了",
                  note: "刷新文件信息和内嵌歌词；你改过的、刮削来的不动",
                  rows: plan.changed.map { SourceGroupRow(title: $0.title, artist: $0.artist, sub: $0.path) })
            group(.lyrics, review, title: "\(plan.lyricsChanged.count) 首歌词文件变了",
                  note: "挂成新的一版，旧版留着；你改过的不动",
                  rows: plan.lyricsChanged.map { SourceGroupRow(title: $0.title, artist: $0.artist, sub: $0.path) })
            group(.moved, review, title: "\(plan.moved.count) 首挪了位置", note: "改到新路径",
                  rows: plan.moved.map { SourceGroupRow(title: $0.title, artist: $0.artist, sub: "\($0.to)（\($0.reason)）") })
            group(.returned, review, title: "\(plan.returned.count) 首回来了", note: "移出过，文件又在了：恢复",
                  rows: plan.returned.map { SourceGroupRow(title: $0.title, artist: $0.artist, sub: $0.path) })
            group(.gone, review, title: "\(plan.gone.count) 首不见了", note: "移出语料库，可以恢复",
                  rows: plan.gone.map { SourceGroupRow(title: $0.title, artist: $0.artist, sub: $0.path) })
            HStack(spacing: 10) {
                if review.pending > 0 {
                    Button(applying ? "正在写…" : "确认这 \(review.pending) 项") { Task { await model.apply() } }
                        .buttonStyle(.borderedProminent)
                        .disabled(model.busy != nil)
                }
                Button(review.pending > 0 ? "取消" : "关闭") { model.review = nil }
                    .buttonStyle(.link)
                    .disabled(applying)
                if review.nothingToDo {
                    Text("\(plan.unchanged) 首都没变，没有要做的").font(.app(.caption)).foregroundStyle(.secondary)
                } else if review.unchangedShown > 0 {
                    Text("另有 \(review.unchangedShown) 首没变").font(.app(.caption)).foregroundStyle(.secondary)
                }
            }
        }
        .padding(.top, 4)
    }

    @ViewBuilder
    private func group(_ which: SourcePlanGroup, _ review: SourceReview, title: String, note: String,
                       rows: [SourceGroupRow]) -> some View {
        if !rows.isEmpty {
            SourceGroupBlock(
                on: Binding(get: { review.on.contains(which) }, set: { model.setGroup(which, on: $0) }),
                title: title, note: note, rows: rows
            )
        }
    }

    // MARK: - 已移出的歌

    private var removedSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            VStack(alignment: .leading, spacing: 2) {
                Text("已移出的歌").font(.app(.title3).bold())
                Text("不算进语料，数据都在，随时可以恢复").font(.app(.caption)).foregroundStyle(.secondary)
            }
            ForEach(model.removed) { song in
                Toggle(isOn: Binding(
                    get: { model.selectedRemoved.contains(song.id) },
                    set: { on in if on { model.selectedRemoved.insert(song.id) } else { model.selectedRemoved.remove(song.id) } }
                )) {
                    VStack(alignment: .leading, spacing: 1) {
                        Text(verbatim: [song.title, song.artist].filter { !$0.isEmpty }.joined(separator: " · "))
                        if song.audioPresent {
                            Text(verbatim: song.audioPath).font(.app(.caption)).foregroundStyle(.secondary)
                        } else {
                            Text("音频文件找不到了").font(.app(.caption)).foregroundStyle(.orange)
                        }
                    }
                }
                .toggleStyle(.checkbox)
            }
            let picked = model.selectedRemoved.count
            HStack(spacing: 10) {
                Button(picked > 0 ? "恢复选中的 \(picked) 首" : "全部恢复") { Task { await model.restore() } }
                    .disabled(model.busy != nil)
                if picked > 0 {
                    Button("彻底删除选中的…") { confirmPurge = true }
                        .buttonStyle(.link)
                        .foregroundStyle(.red)
                }
            }
            if confirmPurge, picked > 0 {
                VStack(alignment: .leading, spacing: 6) {
                    Text("彻底删除这 \(picked) 首：歌词、分词、收听记录都删掉，不能恢复（分词校正保留，音频文件不动）。")
                    HStack {
                        Button("彻底删除", role: .destructive) {
                            Task {
                                await model.purgeSelected()
                                confirmPurge = false
                            }
                        }
                        .disabled(model.busy != nil)
                        Button("取消") { confirmPurge = false }
                    }
                }
                .font(.app(.callout))
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.orange.opacity(0.1)))
            }
        }
    }

    // MARK: - 动作

    private func pickFolders() async {
        let urls = await OpenPanels.chooseFolders(message: "选放音乐的文件夹")
        await model.add(urls.map { SourceFolderInput(path: $0.path) })
    }

    /// 移除前：正在放的歌在这个文件夹里就先停下（歌要移出或删掉，收听记录别写到删掉的歌上）
    private func remove(_ source: LibrarySource, mode: SourceRemoveMode) async {
        if let track = app.player.track, let client = app.client,
           let path = try? await client.resolveMedia(track.audioPath),
           LibrarySourcesView.contains(folder: source.rootPath, path: path) {
            await app.player.prepareDeletion(track.id)
        }
        await model.remove(source, mode: mode)
        removing = nil
    }

    static func contains(folder: String, path: String) -> Bool {
        let root = folder.hasSuffix("/") ? folder : folder + "/"
        return path.hasPrefix(root)
    }

    /// 新文件不在这里导入：拿去导入页走一遍复核
    private func sendToImport(_ newFiles: [String]) async {
        guard let client = app.client else { return }
        await app.importer.scan(newFiles.map { URL(fileURLWithPath: $0) }, client: client)
        app.section = .importer
    }
}

private struct SourceGroupRow {
    let title: String
    let artist: String
    let sub: String
}

/// 一组对账结果：一个勾、一个标题、前几首（和 Windows 一样一组一个勾，不逐首勾）
@MainActor
private struct SourceGroupBlock: View {
    @Binding var on: Bool
    let title: String
    let note: String
    let rows: [SourceGroupRow]
    @State private var expanded = false
    private static let preview = 6

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Toggle(isOn: $on) {
                VStack(alignment: .leading, spacing: 1) {
                    Text(title).bold()
                    Text(note).font(.app(.caption)).foregroundStyle(.secondary)
                }
            }
            .toggleStyle(.checkbox)
            let shown = expanded ? rows : Array(rows.prefix(Self.preview))
            ForEach(Array(shown.enumerated()), id: \.offset) { _, row in
                VStack(alignment: .leading, spacing: 0) {
                    Text(verbatim: [row.title, row.artist].filter { !$0.isEmpty }.joined(separator: " · ")).font(.app(.callout))
                    Text(verbatim: row.sub).font(.app(.caption)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                }
                .padding(.leading, 22)
            }
            if rows.count > Self.preview {
                Button(expanded ? "收起" : "还有 \(rows.count - Self.preview) 首") { expanded.toggle() }
                    .buttonStyle(.link)
                    .font(.app(.caption))
                    .padding(.leading, 22)
            }
        }
    }
}
