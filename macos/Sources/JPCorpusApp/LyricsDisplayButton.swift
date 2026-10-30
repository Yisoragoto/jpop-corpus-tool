import AppKit
import CoreText
import JPCore
import JPCorpusModel
import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct LyricsDisplayButton: View {
    @Environment(AppModel.self) private var app
    @State private var open = false
    @State private var importing = false
    @State private var files: [LyricsFontFile] = []
    @State private var problems: [String] = []
    @State private var error: String?
    private var families: [String] {
        var names = files.map(\.family)
        let installed = InstalledFonts.families
        let preferred = ["Klee One", "LXGW WenKai", "Meiryo", "Noto Sans CJK SC", "Noto Sans CJK JP", "Source Han Sans SC", "Source Han Sans JP", "BIZ UDPGothic"]
        names += preferred.filter { installed.contains($0) }; names += installed
        var seen = Set<String>()
        return names.filter { seen.insert($0).inserted }
    }
    var body: some View {
        Button { open.toggle() } label: { Image(systemName: "textformat.size") }
            .buttonStyle(PlayerGlyphStyle())
            .help("歌词显示：字体、字号与间距").accessibilityLabel("歌词显示")
            .popover(isPresented: $open, arrowEdge: .bottom) { controls.padding(14).frame(width: 420) }
            .task(id: app.libraryURL) { await refresh() }
            .fileImporter(isPresented: $importing, allowedContentTypes: ["ttf", "otf", "ttc", "zip"].map { UTType(filenameExtension: $0) ?? UTType(importedAs: "org.jpcorpus.font.\($0)") }, allowsMultipleSelection: true) { result in
                switch result {
                case .failure(let failure): error = failure.localizedDescription
                case .success(let urls): Task { await importFonts(urls) }
                }
            }
    }
    private var controls: some View {
        @Bindable var app = app
        return VStack(alignment: .leading, spacing: 10) {
            Text("歌词显示").font(.headline)
            fontPicker("主字体", selection: $app.lyricsDisplay.fontFamily)
            fontPicker("备用字体", selection: $app.lyricsDisplay.fallbackFamily)
            HStack {
                Button("导入字体…") { importing = true }
                let broken = app.importedLyricFonts.filter { path in problems.contains { $0.contains(path) } }
                if !broken.isEmpty { Button("清除 \(broken.count) 个读不到的") { app.importedLyricFonts.removeAll { broken.contains($0) }; Task { await refresh() } } }
            }
            Text("忘れたい自分に缶コーヒーを買った")
                .font(lyricFont(app.lyricsDisplay, size: app.lyricsDisplay.fontSize))
                .kerning(app.lyricsDisplay.letterSpacing)
            slider("字号", value: $app.lyricsDisplay.fontSize, range: 12...30)
            slider("振假名字号", value: $app.lyricsDisplay.rubySize, range: 6...18)
            slider("行距", value: $app.lyricsDisplay.lineGap, range: 0...36)
            slider("字距", value: $app.lyricsDisplay.letterSpacing, range: 0...12)
            Picker("注音方式", selection: Binding(get: { app.furiganaMode }, set: { app.furiganaMode = $0; app.showFurigana = true })) {
                Text("只注汉字").tag(FuriganaMode.kanji); Text("整词注音").tag(FuriganaMode.word)
            }
            FuriganaDictionaryNotice(compact: true)
            Button("恢复默认") { app.lyricsDisplay = LyricsDisplay(); app.furiganaMode = .kanji }
            ForEach(problems, id: \.self) { Text($0).font(.caption).foregroundStyle(.orange) }
            if let error { Text(error).font(.caption).foregroundStyle(.orange) }
        }.buttonStyle(.automatic)
    }
    private func fontPicker(_ title: String, selection: Binding<String>) -> some View {
        Picker(title, selection: selection) {
            Text("默认").tag("")
            if !selection.wrappedValue.isEmpty && !families.contains(selection.wrappedValue) { Text("\(selection.wrappedValue)（本机没有）").tag(selection.wrappedValue) }
            ForEach(families, id: \.self) { name in
                Text(name + (files.first { $0.family == name }.map { $0.bundled ? "（自带）" : "（导入）" } ?? "")).tag(name)
            }
        }
    }
    private func slider(_ name: String, value: Binding<Double>, range: ClosedRange<Double>) -> some View {
        HStack { Text(name).frame(width: 80, alignment: .leading); Slider(value: value, in: range, step: 1); Text("\(Int(value.wrappedValue))").monospacedDigit().frame(width: 26) }
    }
    private func refresh() async {
        guard let client = app.core else { return }
        do {
            let catalog = try await client.lyricsFontCatalog(imported: app.importedLyricFonts)
            files = catalog.files; problems = catalog.problems
            for path in Set(files.map(\.path)) { CTFontManagerRegisterFontsForURL(URL(fileURLWithPath: path) as CFURL, .process, nil) }
            resetLyricFontCache()
            let display = app.lyricsDisplay
            app.lyricsDisplay = display
        } catch { self.error = error.localizedDescription }
    }
    private func importFonts(_ urls: [URL]) async {
        guard let client = app.core else { return }
        var imported: [String] = []
        do {
            for url in urls {
                let access = url.startAccessingSecurityScopedResource()
                defer { if access { url.stopAccessingSecurityScopedResource() } }
                if url.pathExtension.lowercased() == "zip" { imported += try await client.lyricsFontImportArchive(url.path) }
                else { imported.append(url.path) }
            }
            for path in imported where !app.importedLyricFonts.contains(path) { app.importedLyricFonts.append(path) }
            await refresh()
            if let first = files.first(where: { imported.contains($0.path) }) { app.lyricsDisplay.fontFamily = first.family }
        } catch { self.error = error.localizedDescription }
    }
}

/// 歌词每个字都要一份字体：按字体、备用字体、字号、字重缓存。不缓存时一行几十个字、
/// 每次重算都去 NSFontManager 找一遍，是歌词视图的主要 CPU 开销之一
@MainActor private var lyricFontCache: [String: Font] = [:]

/// 注册了新字体后要清掉：同名字族可能刚变成能用
@MainActor func resetLyricFontCache() { lyricFontCache.removeAll() }

@MainActor
func lyricFont(_ display: LyricsDisplay, size: Double, weight: NSFont.Weight = .bold) -> Font {
    let key = [display.fontFamily, display.fallbackFamily, "\(size)", "\(weight.rawValue)"].joined(separator: "|")
    if let hit = lyricFontCache[key] { return hit }
    if lyricFontCache.count > 256 { lyricFontCache.removeAll() }
    let font = makeLyricFont(display, size: size, weight: weight)
    lyricFontCache[key] = font
    return font
}

@MainActor
private func makeLyricFont(_ display: LyricsDisplay, size: Double, weight: NSFont.Weight) -> Font {
    func chosen(_ family: String) -> NSFont? {
        guard !family.isEmpty else { return nil }
        return NSFont(name: family, size: size) ?? NSFontManager.shared.font(withFamily: family, traits: [], weight: 5, size: size)
    }
    let base = chosen(display.fontFamily) ?? NSFont.systemFont(ofSize: size, weight: weight)
    guard let fallback = chosen(display.fallbackFamily) else { return Font(base) }
    let descriptor = base.fontDescriptor.addingAttributes([.cascadeList: [fallback.fontDescriptor]])
    return Font(NSFont(descriptor: descriptor, size: size) ?? base)
}

/// 本机装的字体家族，排好序。系统里有几百个，每次重画都去问 NSFontManager 再排序，设置页会明显卡；
/// 一次运行只问一次（导入的字体不在这里，各处另外并进去）
@MainActor
enum InstalledFonts {
    static let families: [String] = NSFontManager.shared.availableFontFamilies.sorted()
}
