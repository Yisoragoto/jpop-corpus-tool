import AppKit
import CoreText
import JPCore
import JPCorpusModel
import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct CloudSettings: View {
    @Environment(AppModel.self) private var app
    @Binding var count: Int; @Binding var hideFunction: Bool; @Binding var colorByPos: Bool; @Binding var family: String
    var inline = false
    @State private var open = false
    @State private var importing = false
    @State private var files: [LyricsFontFile] = []
    var body: some View {
        Group {
            if inline {
                preferences
            } else {
                Button { open.toggle() } label: { Image(systemName: "slider.horizontal.3") }
                    .buttonStyle(.borderless).help("词云设置").accessibilityLabel("词云设置")
                    .popover(isPresented: $open) {
                        preferences.padding(18).frame(width: 520)
                    }
            }
        }
            .task(id: app.libraryURL) { await refresh() }
            .fileImporter(isPresented: $importing,allowedContentTypes: ["ttf","otf","ttc","zip"].map { UTType(filenameExtension: $0) ?? .data },allowsMultipleSelection: true) { result in
                switch result {
                case .failure(let error): app.errorMessage = error.localizedDescription
                case .success(let urls): Task {
                    do {
                        guard let core = app.core else { return }; var imported: [String] = []
                        for url in urls {
                            let access = url.startAccessingSecurityScopedResource(); defer { if access { url.stopAccessingSecurityScopedResource() } }
                            if url.pathExtension.lowercased() == "zip" { imported += try await core.lyricsFontImportArchive(url.path) } else { imported.append(url.path) }
                        }
                        for path in imported where !app.importedLyricFonts.contains(path) { app.importedLyricFonts.append(path) }
                        await refresh(); if let file = files.first(where: { imported.contains($0.path) }) { family = file.family }
                    } catch { app.errorMessage = error.localizedDescription }
                }
                }
            }
    }

    private var preferences: some View {
        VStack(alignment: .leading, spacing: 24) {
            SettingGroup("字体") {
                SettingRow(icon: "textformat", title: "词云字体", description: "资料库里词云用的字体") {
                    Picker("词云字体", selection: $family) {
                        Text("界面字体").tag("")
                        ForEach(families, id: \.self) { Text($0).tag($0) }
                    }.frame(width: 190)
                }
                SettingDivider()
                SettingRow(icon: "square.and.arrow.down", title: "导入字体", description: "ttf / otf / ttc / zip，与歌词共用导入列表") {
                    Button("导入字体…") { importing = true }
                }
            }
            SettingGroup("哪些词进词云") {
                SettingRow(icon: "number", title: "词数", description: "词云最多显示多少个词") {
                    SettingSlider(title: "词数", value: Binding(get: { Double(count) }, set: { count = Int($0) }), range: 30...250)
                }
                SettingDivider()
                SettingRow(icon: "paintpalette", title: "按词性上色", description: "用不同颜色区分词性") {
                    Toggle("按词性上色", isOn: $colorByPos).toggleStyle(.switch)
                }
                SettingDivider()
                SettingRow(icon: "line.3.horizontal.decrease", title: "去掉虚词", description: "词频表和检索不受影响") {
                    Toggle("去掉虚词", isOn: $hideFunction).toggleStyle(.switch)
                }
            }
        }
    }

    private var families: [String] {
        Array(Set(files.map(\.family) + InstalledFonts.families + [family]))
            .filter { !$0.isEmpty }.sorted()
    }
    private func refresh() async {
        do {
            if let catalog = try await app.core?.lyricsFontCatalog(imported: app.importedLyricFonts) {
                files = catalog.files
                for path in Set(files.map(\.path)) { CTFontManagerRegisterFontsForURL(URL(fileURLWithPath: path) as CFURL,.process,nil) }
                if !catalog.problems.isEmpty { app.errorMessage = catalog.problems.joined(separator: "\n") }
            }
        } catch { app.errorMessage = error.localizedDescription }
    }
}
