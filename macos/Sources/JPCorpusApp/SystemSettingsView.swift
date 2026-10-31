import AppKit
import Foundation
import JPCore
import JPCorpusModel
import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct UpdateSettingsView: View {
    @Environment(AppModel.self) private var app
    var body: some View {
        @Bindable var updates = app.updates
        VStack(alignment: .leading, spacing: 8) {
            SettingGroup("更新") {
                preferenceRows
                automaticUpdateRow
                updateActions
            }
            updateFeedback
        }
        .task(id: app.libraryURL) { await updates.load() }
        .alert("切换到 debug？", isPresented: $updates.confirmDebug) {
            Button("切换") { Task { await updates.setChannel(.debug) } }
            Button("取消", role: .cancel) {}
        } message: { Text("会跟随每次提交更新，可能不稳定；不会自动回到正式版。") }
        .sheet(isPresented: $updates.showChangelog) { UpdateChangelogView() }
    }

    @ViewBuilder private var preferenceRows: some View {
        let updates = app.updates
        if let prefs = updates.preferences {
            SettingRow(icon: "app.badge", title: "当前版本", description: "\(prefs.current) · \(prefs.buildId)", selectableDescription: true) {
                EmptyView()
            }
            SettingDivider()
            SettingRow(icon: "arrow.triangle.branch", title: "更新轨道", description: "正式版或跟随每次提交的调试版") {
                Picker("更新轨道", selection: Binding(get: { prefs.channel }, set: { channel in
                    if channel == .debug { updates.confirmDebug = true }
                    else { Task { await updates.setChannel(channel) } }
                })) {
                    Text("正式版（stable）").tag(UpdateChannel.stable)
                    Text("调试版（debug）").tag(UpdateChannel.debug)
                }.frame(width: 180)
            }
            let skipped = prefs.channel == .debug ? prefs.skippedDebug : prefs.skippedStable
            if !skipped.isEmpty {
                SettingDivider()
                SettingRow(icon: "bell.slash", title: "已跳过 \(skipped)", description: "恢复这个版本的更新提醒") {
                    Button("恢复提醒") { Task { await updates.clearSkipped() } }
                }
            }
            SettingDivider()
        }
    }

    @ViewBuilder private var automaticUpdateRow: some View {
        @Bindable var updates = app.updates
        SettingRow(icon: "clock.arrow.circlepath", title: "启动时检查更新", description: "有新版会先显示说明，由你决定装不装") {
            Toggle("启动时检查更新", isOn: $updates.autoCheck).toggleStyle(.switch)
        }
        SettingDivider()
    }

    @ViewBuilder private var updateActions: some View {
        let updates = app.updates
        SettingRow(icon: "arrow.triangle.2.circlepath", title: "检查更新", description: "检查当前轨道有没有新版本") {
            Button(updates.busy ? "检查中…" : "检查更新") { Task { await updates.check() } }
        }.disabled(controlsDisabled)
        SettingDivider()
        SettingRow(icon: "list.bullet.rectangle", title: "更新日志", description: "查看每个版本的更新说明") {
            Button("查看更新日志") { Task { await updates.changelog() } }
        }.disabled(controlsDisabled)
        if updates.status?.updateAvailable == true {
            SettingDivider()
            SettingRow(icon: "sparkles", title: "新版本", description: updates.status?.latest?.identity ?? "") {
                Button("查看新版本…") { updates.offerSkip = false; updates.showDialog = true }
            }.disabled(controlsDisabled)
        }
        if updates.canInstallInApp {
            SettingDivider()
            SettingRow(icon: "arrow.down.circle", title: "安装更新", description: "下载并校验后替换应用，完成后重新打开") {
                Button(installLabel(updates.status?.latest)) { Task { await installUpdate(app: app) } }
                    .buttonStyle(.borderedProminent)
            }.disabled(controlsDisabled)
        }
    }

    @ViewBuilder private var updateFeedback: some View {
        let updates = app.updates
        if updates.downloading || updates.installing {
            UpdateDownloadProgress(fraction: updates.downloadFraction, installing: updates.installing)
                .padding(.horizontal, 16)
        }
        if let at = updates.lastCheckedAt {
            let when = at.formatted(date: .abbreviated, time: .shortened)
            Text(updates.lastCheckError.map { "上次检查 \(when) 失败：\($0)" } ?? "上次检查 \(when)")
                .font(.caption)
                .foregroundStyle(updates.lastCheckError == nil ? Color.secondary : Color.orange)
                .textSelection(.enabled)
        }
        if let status = updates.status {
            let text = status.message.isEmpty ? (status.updateAvailable ? "有新版本 \(status.latest?.identity ?? "")" : "已经是最新版（\(status.current)）") : status.message
            // 这次检查失败了：下面是更早那次成功检查的结果，标出来，别和这次的失败混在一起
            let prefix = updates.statusIsStale
                ? "上次成功检查（\(updates.statusCheckedAt?.formatted(date: .abbreviated, time: .shortened) ?? "更早")）的结果："
                : ""
            Text(prefix + text).font(.caption).foregroundStyle(updates.statusIsStale ? Color.secondary : Color.primary)
        }
        SettingNote("从本项目的 GitHub 发布下载并校验 SHA-256；安装时会退出应用。也可前往发布页手动下载。")
        if let error = updates.errorMessage { Text(error).foregroundStyle(.orange).textSelection(.enabled) }
    }

    private var controlsDisabled: Bool {
        app.updates.busy || app.updates.downloading || app.updates.installing || app.core == nil
    }
}

@MainActor
struct UpdatePresentation: View {
    @Environment(AppModel.self) private var app
    var body: some View {
        Color.clear.frame(width: 0, height: 0)
            .task(id: app.libraryURL) {
                if !SmokeRun.isEnabled { await app.updates.startup() }
            }
            .sheet(isPresented: Binding(get: { app.updates.showDialog }, set: { app.updates.showDialog = $0 })) { UpdateDialogView() }
    }
}

@MainActor
private struct UpdateDialogView: View {
    @Environment(AppModel.self) private var app
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if let release = app.updates.status?.latest {
                Text("有新版本 \(release.identity)").font(.title2.bold())
                Text("当前 \(app.updates.status?.current ?? "") · \(release.publishedAt)").foregroundStyle(.secondary)
                ScrollView { Text(release.notes).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                if app.updates.downloading || app.updates.installing {
                    UpdateDownloadProgress(fraction: app.updates.downloadFraction, installing: app.updates.installing)
                }
                HStack {
                    Button("前往发布页") { Task { await openRelease(release.url, app: app) } }
                    Spacer()
                    if app.updates.offerSkip { Button("跳过这个版本") { Task { await app.updates.skip() } } }
                    Button("稍后") { app.updates.showDialog = false }.keyboardShortcut(.cancelAction)
                    if app.updates.canInstallInApp {
                        Button(installLabel(release)) { Task { await installUpdate(app: app) } }
                            .keyboardShortcut(.defaultAction)
                            .buttonStyle(.borderedProminent)
                    }
                }.disabled(app.updates.busy || app.updates.downloading || app.updates.installing)
            }
            if let error = app.updates.errorMessage { Text(error).foregroundStyle(.orange) }
        }.padding(24).frame(width: 580, height: 440)
    }
}

@MainActor
private struct UpdateChangelogView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(alignment: .leading) {
            Text("更新日志").font(.title2.bold())
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    ForEach(app.updates.releases) { release in
                        Text(release.name).font(.headline)
                        Text(release.publishedAt).font(.caption).foregroundStyle(.secondary)
                        Text(release.notes).textSelection(.enabled)
                        Button("前往发布页") { Task { await openRelease(release.url, app: app) } }
                        Divider()
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            if let error = app.updates.errorMessage { Text(error).foregroundStyle(.orange) }
            Button("关闭") { dismiss() }.keyboardShortcut(.cancelAction)
        }.padding(24).frame(width: 620, height: 500)
    }
}

/// 「下载并安装（34 MB）」
private func installLabel(_ release: UpdateRelease?) -> String {
    guard let size = release?.installer?.size, size > 0 else { return "下载并安装" }
    return "下载并安装（\(ByteCountFormatter.string(fromByteCount: size, countStyle: .file))）"
}

/// 后台还有导入、刮削之类在跑就不装：替换前要退出程序，会把它们打断
@MainActor
private func installUpdate(app: AppModel) async {
    let jobs = await app.runningJobs()
    guard jobs.isEmpty else {
        app.updates.errorMessage = "\(jobs.joined(separator: "、"))还在进行，等它结束再更新"
        return
    }
    await app.updates.downloadAndInstall(appPath: Bundle.main.bundlePath) { AppQuit.request() }
}

private struct UpdateDownloadProgress: View {
    let fraction: Double?
    var installing = false
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if installing {
                ProgressView().progressViewStyle(.linear)
                Text("下载好了，正在退出并替换为新版…").font(.caption).foregroundStyle(.secondary)
            } else {
                if let fraction { ProgressView(value: fraction) } else { ProgressView().progressViewStyle(.linear) }
                Text(fraction.map { "正在下载 \(Int($0 * 100))%" } ?? "正在下载…").font(.caption).foregroundStyle(.secondary).monospacedDigit()
            }
        }
    }
}

@MainActor
private func openRelease(_ address: String, app: AppModel) async {
    guard let client = app.core else { return }
    do {
        let official = try await client.updateReleasePage(address)
        guard let url = URL(string: official), NSWorkspace.shared.open(url) else { throw CoreError.open("无法打开发布页") }
    } catch { app.updates.errorMessage = error.localizedDescription }
}

@MainActor
struct DiagnosticsSettingsView: View {
    @Environment(AppModel.self) private var app
    @State private var busy = false
    @State private var note = ""
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            SettingGroup("诊断") {
                SettingRow(icon: "doc.text.magnifyingglass", title: "诊断报告", description: "版本、目录、运行状态与最近 200 行日志") {
                    Button("复制") { Task { await copy() } }
                    Button("存成文件…") { Task { await save() } }
                }.disabled(busy || app.core == nil)
            }
            SettingNote("不联网、不自动上报；不含歌名和歌词，但路径里有你的用户名。")
            if !note.isEmpty { SettingNote(note) }
        }
    }
    private func copy() async {
        guard let client = app.core else { return }; busy = true; defer { busy = false }
        do {
            let text = try await client.diagnosticsReport(audioAvailable: app.player.audioAvailable)
            NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string)
            note = "复制好了，\(text.components(separatedBy: "\n").count) 行"
        } catch { app.errorMessage = error.localizedDescription }
    }
    private func save() async {
        guard let client = app.core else { return }
        let panel = NSSavePanel(); panel.title = L10n.t("保存诊断信息"); panel.nameFieldStringValue = "jpop-corpus-diagnostics.txt"; panel.allowedContentTypes = [.plainText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        busy = true; defer { busy = false }
        do { note = "存好了：" + (try await client.diagnosticsSave(path: url.path, audioAvailable: app.player.audioAvailable)) }
        catch { app.errorMessage = error.localizedDescription }
    }
}

@MainActor
struct CloudPreferencesView: View {
    @AppStorage("cloud.count") private var count = 120
    @AppStorage("cloud.hideFunction") private var hideFunction = true
    @AppStorage("cloud.colorByPos") private var colorByPos = false
    @AppStorage("cloud.font") private var family = "Klee One"
    @AppStorage("cloud.hidden") private var hiddenJSON = "[]"
    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            CloudSettings(count: $count, hideFunction: $hideFunction, colorByPos: $colorByPos, family: $family, inline: true)
            let hidden = (try? JSONDecoder().decode([String].self, from: Data(hiddenJSON.utf8))) ?? []
            VStack(alignment: .leading, spacing: 8) {
                SettingGroup("自己隐藏的词") {
                    SettingRow(icon: "eye.slash", title: "自己隐藏的词", description: hidden.isEmpty ? "没有隐藏的词" : "已隐藏 \(hidden.count) 个词") {
                        Button("全部恢复") { hiddenJSON = "[]" }.disabled(hidden.isEmpty)
                    }
                }
                if !hidden.isEmpty { SettingNote(hidden.joined(separator: "、")) }
            }
        }
    }
}

@MainActor
struct DictionaryPreferencesView: View {
    var onManage: () -> Void
    @AppStorage("dict.collapsed") private var collapsedJSON = "[]"

    var body: some View {
        SettingGroup("查词结果") {
            SettingRow(icon: "character.book.closed", title: "管理词典", description: "导入、启用、排序 Yomitan 词典") {
                Button("管理词典", action: onManage)
            }
            SettingDivider()
            SettingRow(icon: "rectangle.compress.vertical", title: "折叠记忆", description: "查词结果里折叠的词典，下次查词时保持折叠") {
                Button("清除词典折叠记忆") { collapsedJSON = "[]" }.disabled(collapsedJSON == "[]")
            }
        }
    }
}

@MainActor
struct HealthSettingsView: View {
    @Environment(AppModel.self) private var app
    @State private var health: HealthReport?
    var body: some View {
        SettingGroup("运行状态") {
            if let health {
                SettingRow(icon: "externaldrive", title: "数据库", description: health.dbPath, selectableDescription: true) { EmptyView() }
                SettingDivider()
                SettingRow(icon: "music.note.list", title: "曲目 / 歌词行", description: "语料库中的歌曲与歌词行数") {
                    Text("\(health.tracks) 首 · \(health.lyricLines) 行").foregroundStyle(.secondary).monospacedDigit()
                }
                SettingDivider()
                SettingRow(icon: "character.cursor.ibeam", title: "分词器", description: "振假名与歌词分词") {
                    Text(health.tokenizerReady ? "可用" : "没有分词词典").foregroundStyle(.secondary)
                }
                SettingDivider()
                SettingRow(icon: "speaker.wave.2", title: "音频输出", description: "播放与变调使用的输出设备") {
                    Text(health.audioReady ? "可用" : "没有输出设备").foregroundStyle(.secondary)
                }
            } else {
                SettingRow(icon: "info.circle", title: "运行状态", description: app.core == nil ? "打开语料库后查看运行状态" : "正在读取…") { EmptyView() }
            }
        }.task(id: app.libraryURL) {
            health = nil
            do { health = try await app.core?.health(audioAvailable: app.player.audioAvailable) }
            catch { app.errorMessage = error.localizedDescription }
        }
    }
}
