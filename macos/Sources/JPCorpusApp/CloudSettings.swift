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
    @State private var open = false
    @State private var importing = false
    @State private var files: [LyricsFontFile] = []
    var body: some View {
        Button { open.toggle() } label: { Image(systemName: "slider.horizontal.3") }.buttonStyle(.borderless).help("词云设置")
            .popover(isPresented: $open) {
                VStack(alignment: .leading,spacing: 14) {
                    Text("词云设置").font(.headline)
                    Picker("字体",selection: $family) {
                        Text("界面字体").tag("")
                        ForEach(Array(Set(files.map(\.family)+NSFontManager.shared.availableFontFamilies+[family])).filter { !$0.isEmpty }.sorted(),id: \.self) { Text($0).tag($0) }
                    }
                    Button("导入字体…") { importing = true }
                    HStack { Text("词数"); Slider(value: Binding(get: { Double(count) }, set: { count = Int($0) }), in: 30...250, step: 1); Text(String(count)).monospacedDigit() }
                    Toggle("按词性上色",isOn: $colorByPos); Toggle("去虚词",isOn: $hideFunction)
                }.padding(18).frame(width: 330)
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
