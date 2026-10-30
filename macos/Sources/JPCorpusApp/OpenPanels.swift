import AppKit
import JPCore

/// 系统的打开面板。选语料库、加音乐文件夹用它，不用 SwiftUI 的 fileImporter：
/// - 同一个窗口里好几处都要选（文件菜单、欢迎页、导入页、侧栏、缺文件提示），fileImporter 挂多了会互相抢；
/// - 能同时允许选文件夹和任意文件，不会因为系统不认识 .db 扩展名把 corpus.db 显示成灰的
@MainActor
enum OpenPanels {
    /// 语料库：文件夹，或者里面的数据库文件
    static func chooseLibrary() async -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.message = L10n.t("选语料库文件夹，或直接选里面的 corpus.db（从 Windows 拷来时，整个 JPOP Corpus Tool 文件夹一起拷）")
        panel.prompt = L10n.t("打开")
        return await run(panel)
    }

    /// 一个文件夹（音乐文件夹之类）
    static func chooseFolder(message: String) async -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = L10n.t(message)
        panel.prompt = L10n.t("选择")
        return await run(panel)
    }

    private static func run(_ panel: NSOpenPanel) async -> URL? {
        await withCheckedContinuation { continuation in
            let done: (NSApplication.ModalResponse) -> Void = { response in
                continuation.resume(returning: response == .OK ? panel.url : nil)
            }
            // 有主窗口就作为它的表单弹出（不会跑到别的窗口后面）
            if let window = NSApp.keyWindow ?? NSApp.mainWindow {
                panel.beginSheetModal(for: window, completionHandler: done)
            } else {
                panel.begin(completionHandler: done)
            }
        }
    }
}
