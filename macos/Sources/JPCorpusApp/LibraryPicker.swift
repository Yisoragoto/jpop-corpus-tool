import JPCorpusModel
import SwiftUI
import UniformTypeIdentifiers

/// 设置窗口里「换一个…」用的 fileImporter 类型：文件夹，或者任意文件。
/// 不按 .db / .sqlite 扩展名去造 UTType：没有程序认领这些扩展名时，系统可能把 corpus.db 显示成灰的、点不了。
/// 选了什么由 `AppModel.libraryLocation(forPicked:)` 检查，不是数据库文件会给出说明
let libraryPickerTypes: [UTType] = [.folder, .data]

/// 主窗口里唯一的「打开语料库」入口。文件菜单（⌘O）、欢迎页、导入页、侧栏底部的库名都只是把
/// `app.showLibraryPicker` 设成 true，这里弹系统的打开面板。
/// 选了一个没有数据库的文件夹时，在这里问要不要新建
@MainActor
struct LibraryPickerHost: ViewModifier {
    @Environment(AppModel.self) private var app

    func body(content: Content) -> some View {
        content
            // 用系统的打开面板（见 OpenPanels）：几处入口共用，不会和页面里的 fileImporter 互相抢
            .onChange(of: app.showLibraryPicker) { _, show in
                guard show else { return }
                Task {
                    let url = await OpenPanels.chooseLibrary()
                    app.showLibraryPicker = false
                    if let url { await app.selectLibrary(picked: url) }
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
}
