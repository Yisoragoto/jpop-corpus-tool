import AppKit
import CoreText
import JPCorpusModel
import JPCore
import SwiftUI
import UniformTypeIdentifiers

private enum SettingsCategory: String, CaseIterable, Identifiable {
    case appearance, lyrics, wordcloud, dict, mining, library, system, about

    var id: String { rawValue }
    var title: String {
        switch self {
        case .appearance: return "外观"
        case .lyrics: return "歌词"
        case .wordcloud: return "词云"
        case .dict: return "词典"
        case .mining: return "制卡"
        case .library: return "曲库维护"
        case .system: return "更新与系统"
        case .about: return "关于"
        }
    }
    var icon: String {
        switch self {
        case .appearance: return "macwindow"
        case .lyrics: return "text.alignleft"
        case .wordcloud: return "textformat"
        case .dict: return "character.book.closed"
        case .mining: return "rectangle.on.rectangle"
        case .library: return "externaldrive"
        case .system: return "arrow.triangle.2.circlepath"
        case .about: return "info.circle"
        }
    }
    var description: String {
        switch self {
        case .appearance: return "全屏歌词的背景、播放条、首页的收听目标"
        case .lyrics: return "振假名、字体、字号和行距"
        case .wordcloud: return "资料库里词云的字体，以及哪些词不进词云"
        case .dict: return "导入、启用、排序 Yomitan 词典"
        case .mining: return "发到 Anki 的牌组、笔记类型和字段"
        case .library: return "音频目录、新歌检查、补全与迁移"
        case .system: return "检查更新、诊断报告"
        case .about: return "语料库在哪、程序的运行状态"
        }
    }
}

@MainActor
struct SettingsView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    var embedded = false
    @AppStorage(NavStyle.key) private var navStyle: NavStyle = NavStyle.defaultStyle
    @AppStorage("settings.category") private var selectedCategory = SettingsCategory.appearance.rawValue
    private enum PickKind { case library, audio }
    @State private var pick = FilePickState<PickKind>()
    /// 一个视图只挂一个 sheet（SourceGuards）：用哪个由这里决定
    private enum Sheet: String, Identifiable { case lyrics, maintenance; var id: String { rawValue } }
    @State private var sheet: Sheet?

    private var category: SettingsCategory { SettingsCategory(rawValue: selectedCategory) ?? .appearance }

    var body: some View {
        HStack(spacing: 0) {
            categoryList
            Divider()
            categoryContent
        }
        .controlSize(.regular)
        .glass(.sidebar)
        .frame(width: embedded ? nil : 880, height: embedded ? nil : 640)
        .sheet(item: $sheet) { which in
            switch which {
            case .lyrics: LyricsLibraryView()
            case .maintenance: LibraryMaintenanceView()
            }
        }
        .fileImporter(isPresented: $pick.isPresented, allowedContentTypes: pick.intent == .library
                      ? libraryPickerTypes
                      : [.folder]) { result in
            let kind = pick.finish()
            switch result {
            case .failure(let error):
                app.errorMessage = error.localizedDescription
            case .success(let url):
                switch kind {
                case .library:
                    Task { await app.selectLibrary(picked: url) }
                case .audio:
                    if !app.audioRoots.contains(url.path) { app.audioRoots.append(url.path) }
                case nil:
                    app.errorMessage = "文件选择的用途已丢失，请重新选择"
                }
            }
        }
        .alert("新建语料库？", isPresented: Binding(
            get: { app.pendingCreationURL != nil },
            set: { if !$0 { app.cancelCreation() } }
        )) {
            if let folder = app.pendingCreationURL {
                Button("新建") { Task { await app.confirmCreation(in: folder) } }
                Button("取消", role: .cancel) { app.cancelCreation() }
            }
        } message: {
            Text("这里没有 corpus.db，要在这里新建语料库吗？\n\(app.pendingCreationURL?.path ?? "")")
        }
    }

    private var categoryList: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 4) {
                Text("设置")
                    .font(.title2.weight(.semibold))
                    .padding(.horizontal, 10)
                    .padding(.bottom, 16)
                ForEach(SettingsCategory.allCases) { item in
                    Button { selectedCategory = item.rawValue } label: {
                        Label(item.title, systemImage: item.icon)
                            .font(.body.weight(category == item ? .semibold : .regular))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 10)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(category == item ? Color.accentColor : Color.primary)
                    .background(category == item ? Color.accentColor.opacity(0.12) : Color.clear,
                                in: RoundedRectangle(cornerRadius: 7))
                    .accessibilityAddTraits(category == item ? [.isSelected] : [])
                }
            }
            .padding(12)
            .padding(.top, 10)
        }
        .frame(width: 172)
        .accessibilityLabel("设置分类")
    }

    private var categoryContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                HStack(spacing: 12) {
                    Image(systemName: category.icon)
                        .font(.title2)
                        .foregroundStyle(.tint)
                        .frame(width: 44, height: 44)
                        .background(Color.accentColor.opacity(0.1), in: RoundedRectangle(cornerRadius: 10))
                    VStack(alignment: .leading, spacing: 4) {
                        Text(category.title).font(.title2.weight(.semibold)).accessibilityAddTraits(.isHeader)
                        Text(category.description).font(.callout).foregroundStyle(.secondary)
                    }
                }
                selectedContent
                if let error = app.errorMessage {
                    Text(error).font(.caption).foregroundStyle(.orange).textSelection(.enabled)
                }
            }
            .frame(maxWidth: 900, alignment: .leading)
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
        .id(category)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder private var selectedContent: some View {
        switch category {
        case .appearance: appearanceSettings
        case .lyrics: LyricsPreferencesView()
        case .wordcloud: CloudPreferencesView()
        case .dict: DictionaryPreferencesView { navigate(to: .dictionaries) }
        case .mining: MineSettingsView()
        case .library: librarySettings
        case .system:
            UpdateSettingsView()
            DiagnosticsSettingsView()
        case .about:
            libraryLocation
            HealthSettingsView()
        }
    }

    private var appearanceSettings: some View {
        VStack(alignment: .leading, spacing: 24) {
            languageSettings
            stageSettings
        }
    }

    /// 界面语言。当场换：根视图重建，文字重新翻一遍；菜单栏和系统提示框重启后跟着换
    private var languageSettings: some View {
        SettingGroup("语言与导航") {
            SettingRow(icon: "globe", title: "界面语言", description: "菜单栏和系统对话框在重新启动后切换") {
                Picker("界面语言", selection: Binding(
                    get: { UserDefaults.standard.string(forKey: L10n.languageKey) ?? "" },
                    set: { value in
                        L10n.choose(value.isEmpty ? nil : value)
                        app.languageRevision += 1
                    }
                )) {
                    Text("跟随系统").tag("")
                    Text(verbatim: "中文").tag("zh")
                    Text(verbatim: "English").tag("en")
                }.frame(width: 160)
            }
            SettingDivider()
            SettingRow(icon: "sidebar.left", title: "导航栏位置", description: "工具栏左上角的按钮也能切换") {
                Picker("导航栏位置", selection: $navStyle) {
                    Text("侧边栏").tag(NavStyle.sidebar)
                    Text("顶部栏").tag(NavStyle.top)
                }
                .pickerStyle(.segmented)
                .frame(width: 180)
            }
        }
    }

    private var stageSettings: some View {
        @Bindable var app = app
        @Bindable var player = app.player
        return SettingGroup("全屏与播放") {
            SettingRow(icon: "macwindow", title: "全屏歌词的背景用封面虚化", description: "没有封面时自动回到纯色") {
                Toggle("全屏背景用封面虚化", isOn: $app.stageCoverBlur).toggleStyle(.switch)
            }
            SettingDivider()
            SettingRow(icon: "drop.halffull", title: "虚化强度", description: "越大越糊、歌词越清楚") {
                SettingSlider(title: "虚化强度", value: $app.stageBlurRadius, range: 20...120)
            }.disabled(!app.stageCoverBlur)
            SettingDivider()
            SettingRow(icon: "waveform", title: "播放条上的音频可视化", description: "播放条上随音乐跳动的频谱条") {
                Toggle("显示音频可视化", isOn: $player.spectrumEnabled).toggleStyle(.switch)
            }
            SettingDivider()
            SettingRow(icon: "clock", title: "每天的收听目标", description: "首页「今日」那个环按这个算") {
                SettingSlider(title: "每天的收听目标", value: $app.dailyGoalMinutes, range: 5...180, unit: " 分钟")
            }
        }
    }

    private var librarySettings: some View {
        VStack(alignment: .leading, spacing: 24) {
            maintenanceSettings
            audioSettings
        }
    }

    private var maintenanceSettings: some View {
        @Bindable var app = app
        return SettingGroup("补齐与修复") {
            SettingRow(icon: "music.note.list", title: "启动时检查新歌", description: "发现未导入的音频时提示，不会自动导入") {
                Toggle("启动时检查新歌", isOn: $app.autoCheckNewSongs).toggleStyle(.switch)
            }
            SettingDivider()
            SettingRow(icon: "externaldrive", title: "资料库管理", description: "歌曲、音频、成员、维护与迁移") {
                Button("管理…") { sheet = .maintenance }
            }.disabled(app.core == nil)
            SettingDivider()
            SettingRow(icon: "text.alignleft", title: "歌词来源与补全", description: "歌词来源、批量补全与逐首预览") {
                Button("管理…") { sheet = .lyrics }
            }.disabled(app.core == nil)
        }
    }

    private var audioSettings: some View {
        VStack(alignment: .leading, spacing: 8) {
            SettingGroup("音频目录（路径映射）") {
                SettingRow(icon: "folder.badge.plus", title: "音频目录", description: "原路径不存在时，到这些文件夹里按文件名查找") {
                    Button("添加文件夹…") { pick.present(.audio) }
                }
                ForEach(app.audioRoots, id: \.self) { root in
                    SettingDivider()
                    SettingRow(icon: "folder", title: URL(fileURLWithPath: root).lastPathComponent,
                               description: root, selectableDescription: true) {
                        Button(role: .destructive) { app.audioRoots.removeAll { $0 == root } } label: {
                            Image(systemName: "minus.circle")
                        }
                        .buttonStyle(.borderless)
                        .help("移除音频目录")
                        .accessibilityLabel("移除音频目录 \(root)")
                    }
                }
                SettingDivider()
                SettingRow(icon: "square.and.arrow.down", title: "导入新歌", description: "音频目录只用于查找已在库里的曲目") {
                    Button("去导入") { navigate(to: .importer) }.disabled(app.client == nil)
                }
            }
            if app.audioRoots.isEmpty { SettingNote("尚未添加音频目录。库中原来的音频路径仍然有效。") }
        }
    }

    private var libraryLocation: some View {
        VStack(alignment: .leading, spacing: 8) {
            SettingGroup("语料库") {
                SettingRow(icon: "externaldrive", title: app.isDemo ? "示例语料库" : "当前语料库",
                           description: app.libraryURL?.appendingPathComponent(app.databaseName).path ?? "未选择", selectableDescription: true) {
                    Button("换一个…") { pick.present(.library) }
                }
                SettingDivider()
                SettingRow(icon: "music.note", title: "示例语料库", description: "先看看示例歌曲与歌词") {
                    Button("打开示例") { Task { await app.openDemo() } }
                }
                SettingDivider()
                SettingRow(icon: "folder", title: "语料库文件夹", description: "\(app.tracks.count) 首") {
                    Button("在 Finder 中显示") {
                        if let url = app.libraryURL { NSWorkspace.shared.activateFileViewerSelecting([url.appendingPathComponent(app.databaseName)]) }
                    }.disabled(app.libraryURL == nil)
                }
            }
            SettingNote("与 Windows 共用同一种库，整个文件夹拷过来就能打开。旧库自动补齐结构；比本程序新的库会拒绝打开。")
        }
    }

    private func navigate(to section: SidebarItem) {
        app.section = section
        if !embedded { dismiss() }
    }
}

@MainActor
private struct LyricsPreferencesView: View {
    @Environment(AppModel.self) private var app
    @State private var importing = false
    @State private var files: [LyricsFontFile] = []
    @State private var problems: [String] = []
    /// 字体菜单里的选项（自带 / 导入的 + 常用的 + 本机全部）。算好存着，不在每次重画时现拼
    @State private var families: [FontChoice] = []

    struct FontChoice: Hashable {
        let family: String
        let label: String
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            annotationSettings
            playbackSettings
            fontSettings
            layoutSettings
            if !problems.isEmpty {
                ForEach(problems, id: \.self) { Text($0).font(.caption).foregroundStyle(.orange).textSelection(.enabled) }
            }
        }
        .task(id: app.libraryURL) {
            if families.isEmpty { families = buildFamilies() }
            await refresh()
        }
        .fileImporter(isPresented: $importing,
                      allowedContentTypes: ["ttf", "otf", "ttc", "zip"].map {
                          UTType(filenameExtension: $0) ?? UTType(importedAs: "org.jpcorpus.font.\($0)")
                      }, allowsMultipleSelection: true) { result in
            switch result {
            case .failure(let error): app.errorMessage = error.localizedDescription
            case .success(let urls): Task { await importFonts(urls) }
            }
        }
    }

    private var annotationSettings: some View {
        @Bindable var app = app
        return SettingGroup("振假名") {
            SettingRow(icon: "character.phonetic", title: "默认显示振假名", description: "也可以在歌词右上角随时开关") {
                Toggle("默认显示振假名", isOn: $app.showFurigana).toggleStyle(.switch)
            }
            SettingDivider()
            SettingRow(icon: "character.cursor.ibeam", title: "注音方式", description: "只注汉字或为整个词注音") {
                Picker("注音方式", selection: Binding(get: { app.furiganaMode }, set: {
                    app.furiganaMode = $0
                    app.showFurigana = true
                })) {
                    Text("只注汉字").tag(FuriganaMode.kanji)
                    Text("整词注音").tag(FuriganaMode.word)
                }.frame(width: 190)
            }
        }
    }

    private var playbackSettings: some View {
        @Bindable var app = app
        return SettingGroup("播放") {
            SettingRow(icon: "text.alignleft", title: "逐字点亮当前行", description: "有真实逐字或逐词时间时逐字点亮") {
                Toggle("逐字点亮当前行", isOn: $app.karaokeLyrics).toggleStyle(.switch)
            }
        }
    }

    private var fontSettings: some View {
        @Bindable var app = app
        return SettingGroup("字体") {
            SettingRow(icon: "textformat", title: "歌词字体", description: "本机字体、自带字体与导入的字体") {
                fontPicker("歌词字体", selection: $app.lyricsDisplay.fontFamily)
            }
            SettingDivider()
            SettingRow(icon: "textformat.alt", title: "备用字体", description: "主字体里没有的字用这个显示") {
                fontPicker("备用字体", selection: $app.lyricsDisplay.fallbackFamily)
            }
            SettingDivider()
            SettingRow(icon: "square.and.arrow.down", title: "导入字体", description: "ttf / otf / ttc，或装着字体的 zip") {
                Button("导入字体…") { importing = true }
            }
            if !brokenFonts.isEmpty {
                SettingDivider()
                SettingRow(icon: "exclamationmark.triangle", title: "读不到的字体", description: "清除失效的导入记录") {
                    Button("清除 \(brokenFonts.count) 个读不到的") {
                        let broken = brokenFonts
                        app.importedLyricFonts.removeAll { broken.contains($0) }
                        Task { await refresh() }
                    }
                }
            }
        }
    }

    private var layoutSettings: some View {
        @Bindable var app = app
        return VStack(alignment: .leading, spacing: 8) {
            SettingGroup("字体与排版") {
                SettingRow(icon: "textformat.size", title: "字号", description: "歌词正文的文字大小") {
                    SettingSlider(title: "字号", value: $app.lyricsDisplay.fontSize, range: 12...30)
                }
                SettingDivider()
                SettingRow(icon: "textformat.superscript", title: "振假名字号", description: "歌词上方注音的文字大小") {
                    SettingSlider(title: "振假名字号", value: $app.lyricsDisplay.rubySize, range: 6...18)
                }
                SettingDivider()
                SettingRow(icon: "line.3.horizontal", title: "行距", description: "歌词行之间的距离") {
                    SettingSlider(title: "行距", value: $app.lyricsDisplay.lineGap, range: 0...36)
                }
                SettingDivider()
                SettingRow(icon: "textformat.abc", title: "字距", description: "歌词文字之间的距离") {
                    SettingSlider(title: "字距", value: $app.lyricsDisplay.letterSpacing, range: 0...12)
                }
                SettingDivider()
                SettingRow(icon: "arrow.counterclockwise", title: "恢复默认", description: "重置字体、字号、间距和注音方式") {
                    Button("恢复默认") { app.lyricsDisplay = LyricsDisplay(); app.furiganaMode = .kanji }
                }
            }
            Text("忘れたい自分に缶コーヒーを買った")
                .font(lyricFont(app.lyricsDisplay, size: app.lyricsDisplay.fontSize))
                .kerning(app.lyricsDisplay.letterSpacing)
                .padding(16)
                .accessibilityLabel("歌词字体预览")
            SettingNote("在歌词右上角的「显示」面板里改也是同一份。")
        }
    }

    private func buildFamilies() -> [FontChoice] {
        var names = files.map(\.family)
        let installed = InstalledFonts.families
        let preferred = ["Klee One", "LXGW WenKai", "Meiryo", "Noto Sans CJK SC", "Noto Sans CJK JP", "Source Han Sans SC", "Source Han Sans JP", "BIZ UDPGothic"]
        let installedSet = Set(installed)
        names += preferred.filter { installedSet.contains($0) }
        names += installed
        var seen = Set<String>()
        let kinds = Dictionary(files.map { ($0.family, $0.bundled) }, uniquingKeysWith: { first, _ in first })
        return names.filter { seen.insert($0).inserted }.map { name in
            FontChoice(family: name, label: name + (kinds[name].map { L10n.t($0 ? "（自带）" : "（导入）") } ?? ""))
        }
    }

    private var brokenFonts: [String] {
        app.importedLyricFonts.filter { path in problems.contains { $0.contains(path) } }
    }

    private func fontPicker(_ title: String, selection: Binding<String>) -> some View {
        Picker(title, selection: selection) {
            Text("默认").tag("")
            if !selection.wrappedValue.isEmpty && !families.contains(where: { $0.family == selection.wrappedValue }) {
                Text("\(selection.wrappedValue)（本机没有）").tag(selection.wrappedValue)
            }
            ForEach(families, id: \.self) { choice in
                Text(verbatim: choice.label).tag(choice.family)
            }
        }.frame(width: 190)
    }

    private func refresh() async {
        guard let client = app.core else { return }
        do {
            let catalog = try await client.lyricsFontCatalog(imported: app.importedLyricFonts)
            files = catalog.files
            problems = catalog.problems
            families = buildFamilies()
            for path in Set(files.map(\.path)) {
                CTFontManagerRegisterFontsForURL(URL(fileURLWithPath: path) as CFURL, .process, nil)
            }
            resetLyricFontCache()
            let display = app.lyricsDisplay
            app.lyricsDisplay = display
        } catch { app.errorMessage = error.localizedDescription }
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
        } catch { app.errorMessage = error.localizedDescription }
    }
}
