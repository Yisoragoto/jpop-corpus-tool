import AppKit
import JPCore
import JPCorpusModel
import SwiftUI

/// 打开库后，库里记的音频、封面、照片在这台电脑上找不到时，正文顶上给一条说明。
///
/// 典型情况：只把 Windows 上的 corpus.db 拷到了 Mac。库里记的是 `E:\cloud music\…`、
/// `C:\Users\…\JPOP Corpus Tool\raw\covers\…` 这种路径，播放没声音、封面全空，以前界面上什么都不说。
/// 这里说清楚缺什么、原来在哪、怎么补：封面和照片跟着整个库文件夹走，音乐要另外拷过来再加成音乐文件夹
@MainActor
struct MediaHealthBanner: View {
    @Environment(AppModel.self) private var app
    @State private var status: MediaStatus?
    @State private var busy = false
    @State private var result = ""
    /// 关掉过的库（按库路径记），下次打开同一个库不再提示；缺的数量变了也不再提示，设置页里随时能看
    @AppStorage("media.banner.dismissed") private var dismissed = ""

    private var libraryKey: String { app.libraryURL?.path ?? "" }

    var body: some View {
        Group {
            if let status, status.anythingMissing, dismissed != libraryKey, !app.isDemo {
                content(status)
            }
        }
        .task(id: "\(libraryKey)|\(app.audioRoots.joined(separator: "|"))|\(app.library.version)") {
            guard let core = app.core, !app.isDemo else { status = nil; return }
            status = try? await core.mediaStatus()
        }
    }

    private func content(_ status: MediaStatus) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Image(systemName: "externaldrive.badge.questionmark").foregroundStyle(.orange)
                Text("这个库里有些文件在这台电脑上找不到").font(.callout.weight(.semibold))
                Spacer()
                Button { dismissed = libraryKey } label: { Image(systemName: "xmark") }
                    .buttonStyle(.borderless)
                    .help("不再提示这个库")
            }
            if status.audioMissing > 0 {
                Text("\(status.audioMissing) / \(status.songs) 首歌找不到音频，原来在：")
                    .font(.callout)
                ForEach(status.missingAudioFolders, id: \.folder) { folder in
                    Text(verbatim: "· \(folder.folder)（\(folder.songs)）")
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                }
            }
            let images = status.coversMissing + status.albumsMissing + status.artistsMissing
            if images > 0 {
                Text("\(status.coversMissing) 张歌曲封面、\(status.albumsMissing) 张专辑封面、\(status.artistsMissing) 张音乐人照片找不到。")
                    .font(.callout)
            }
            Text("封面和照片在库文件夹的 raw 子文件夹里：把 Windows 上整个「JPOP Corpus Tool」文件夹拷过来就能找到。音乐文件拷到这台 Mac 后，点「添加音乐文件夹」，按文件名自动找，不用改库。")
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 10) {
                if status.audioMissing > 0 {
                    Button("添加音乐文件夹…") { Task { await addMusicFolder() } }
                        .buttonStyle(.borderedProminent)
                }
                if images > 0 {
                    Button("在线补齐封面") { Task { await backfillCovers() } }
                        .help("按刮削记录重新下载找不到的歌曲封面（要联网）。库文件夹拷全了的话不用这一步")
                }
                Button("在 Finder 中显示库文件夹") {
                    if let url = app.libraryURL { NSWorkspace.shared.activateFileViewerSelecting([url]) }
                }
                if busy { ProgressView().controlSize(.small) }
                if !result.isEmpty { Text(result).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
            }
            .disabled(busy)
        }
        .padding(12)
        .glassCard(cornerRadius: 12)
        .padding(.horizontal, 12)
        .padding(.top, 6)
    }

    private func addMusicFolder() async {
        guard let url = await OpenPanels.chooseFolder(message: "选拷到这台 Mac 上的音乐文件夹（里面可以有子文件夹）") else { return }
        if !app.audioRoots.contains(url.path) { app.audioRoots.append(url.path) }
        busy = true
        defer { busy = false }
        // 先等音乐文件夹真的设进 Rust，再让界面（封面、播放、这条提示）重新找文件
        try? await app.core?.setAudioRoots(app.audioRoots)
        app.library.version += 1
    }

    private func backfillCovers() async {
        guard let core = app.core else { return }
        busy = true
        defer { busy = false }
        do {
            let report = try await core.scrapeBackfillCovers()
            result = L10n.t("补上 \(report.filled) 张；没有刮削记录 \(report.noCandidate) 首，下载失败 \(report.failed) 首")
            app.library.version += 1
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }
}
