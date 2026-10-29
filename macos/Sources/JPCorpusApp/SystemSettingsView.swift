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
        Section("更新") {
            if let prefs = updates.preferences {
                LabeledContent("当前版本", value: "\(prefs.current) · \(prefs.buildId)")
                Picker("更新轨道", selection: Binding(get: { prefs.channel }, set: { channel in
                    if channel == .debug { updates.confirmDebug = true }
                    else { Task { await updates.setChannel(channel) } }
                })) { Text("正式版（stable）").tag(UpdateChannel.stable); Text("调试版（debug）").tag(UpdateChannel.debug) }
                let skipped = prefs.channel == .debug ? prefs.skippedDebug : prefs.skippedStable
                if !skipped.isEmpty { HStack { Text("已跳过 \(skipped)"); Button("恢复提醒") { Task { await updates.clearSkipped() } } } }
            }
            Toggle("启动时检查更新", isOn: $updates.autoCheck)
            HStack {
                Button(updates.busy ? "检查中…" : "检查更新") { Task { await updates.check() } }
                Button("查看更新日志") { Task { await updates.changelog() } }
                if updates.status?.updateAvailable == true {
                    Button("查看新版本…") { updates.offerSkip = false; updates.showDialog = true }
                }
            }.disabled(updates.busy || app.core == nil)
            if let at = updates.lastCheckedAt {
                let when = at.formatted(date: .abbreviated, time: .shortened)
                Text(updates.lastCheckError.map { "上次检查 \(when) 失败：\($0)" } ?? "上次检查 \(when)")
                    .font(.caption)
                    .foregroundStyle(updates.lastCheckError == nil ? Color.secondary : Color.orange)
                    .textSelection(.enabled)
            }
            if let status = updates.status {
                Text(status.message.isEmpty ? (status.updateAvailable ? "有新版本 \(status.latest?.identity ?? "")" : "已经是最新版（\(status.current)）") : status.message).font(.caption)
            }
            Text("有新版会先显示说明，由你选择前往官方发布页、稍后或跳过；在发布页下载新的 .dmg 覆盖安装，程序不会自动下载或执行文件。")
                .font(.caption).foregroundStyle(.secondary)
            if let error = updates.errorMessage { Text(error).foregroundStyle(.orange).textSelection(.enabled) }
        }
        .task(id: app.libraryURL) { await updates.load() }
        .alert("切换到 debug？", isPresented: $updates.confirmDebug) {
            Button("切换") { Task { await updates.setChannel(.debug) } }
            Button("取消", role: .cancel) {}
        } message: { Text("会跟随每次提交更新，可能不稳定；不会自动回到正式版。") }
        .sheet(isPresented: $updates.showChangelog) { UpdateChangelogView() }
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
                HStack {
                    Button("前往发布页") { Task { await openRelease(release.url, app: app) } }
                    Spacer()
                    if app.updates.offerSkip { Button("跳过这个版本") { Task { await app.updates.skip() } } }
                    Button("稍后") { app.updates.showDialog = false }.keyboardShortcut(.cancelAction)
                }.disabled(app.updates.busy)
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
        Section("诊断") {
            Text("版本、语料库目录、曲目数、分词词典和音频状态、最近 200 行日志。不联网、不自动上报；不含歌名和歌词，但路径里有你的用户名。")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("复制") { Task { await copy() } }
                Button("存成文件…") { Task { await save() } }
            }.disabled(busy || app.core == nil)
            if !note.isEmpty { Text(note).font(.caption).textSelection(.enabled) }
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
        let panel = NSSavePanel(); panel.title = "保存诊断信息"; panel.nameFieldStringValue = "jpop-corpus-diagnostics.txt"; panel.allowedContentTypes = [.plainText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        busy = true; defer { busy = false }
        do { note = "存好了：" + (try await client.diagnosticsSave(path: url.path, audioAvailable: app.player.audioAvailable)) }
        catch { app.errorMessage = error.localizedDescription }
    }
}

@MainActor
struct CloudPreferencesView: View {
    @Environment(AppModel.self) private var app
    @AppStorage("cloud.count") private var count = 120
    @AppStorage("cloud.hideFunction") private var hideFunction = true
    @AppStorage("cloud.colorByPos") private var colorByPos = false
    @AppStorage("cloud.font") private var family = "Klee One"
    @AppStorage("cloud.hidden") private var hiddenJSON = "[]"
    @AppStorage("dict.collapsed") private var collapsedJSON = "[]"
    var body: some View {
        Section("词云") {
            HStack { Text("字体、词数、词性颜色与虚词过滤"); Spacer(); CloudSettings(count: $count, hideFunction: $hideFunction, colorByPos: $colorByPos, family: $family) }
            let hidden = (try? JSONDecoder().decode([String].self, from: Data(hiddenJSON.utf8))) ?? []
            if !hidden.isEmpty {
                Text("已隐藏：" + hidden.joined(separator: "、")).font(.caption)
                Button("恢复全部隐藏词") { hiddenJSON = "[]" }
            }
        }
        Section("词典") {
            Button("管理词典") { app.section = .dictionaries }
            Button("清除词典折叠记忆") { collapsedJSON = "[]" }.disabled(collapsedJSON == "[]")
        }
    }
}

@MainActor
struct HealthSettingsView: View {
    @Environment(AppModel.self) private var app
    @State private var health: HealthReport?
    var body: some View {
        Section("运行状态") {
            if let health {
                LabeledContent("数据库", value: health.dbPath)
                LabeledContent("曲目 / 歌词行", value: "\(health.tracks) / \(health.lyricLines)")
                LabeledContent("分词器", value: health.tokenizerReady ? "可用" : "没有分词词典")
                LabeledContent("音频输出", value: health.audioReady ? "可用" : "没有输出设备")
            }
        }.task(id: app.libraryURL) {
            do { health = try await app.core?.health(audioAvailable: app.player.audioAvailable) }
            catch { app.errorMessage = error.localizedDescription }
        }
    }
}
