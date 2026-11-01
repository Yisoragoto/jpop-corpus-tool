import AppKit
import CoreText
import JPCorpusModel
import Observation
import SwiftUI

/// 整个界面的字体。空串 = 系统字体（San Francisco / 苹方），和以前一模一样。
///
/// SwiftUI 没有「把系统字体整体换掉」的开关：`.font(.caption)` 这类写法永远是系统字体。
/// 所以界面里的字号一律写成 `.app(.caption)` / `.appSystem(size:)`，这里按设置决定用系统字体还是选中的字体。
/// `family` 是 Observable：视图在 body 里经过这里读过一次，换字体时就会跟着重画，不用重启。
/// 不绑主线程：Canvas 的绘制闭包里也会画字。实际只在主线程上读写（设置页改、视图重画时读）
@Observable
final class AppFont: @unchecked Sendable {
    static let shared = AppFont()
    static let key = "ui.fontFamily"

    var family: String = UserDefaults.standard.string(forKey: AppFont.key) ?? "" {
        didSet { UserDefaults.standard.set(family, forKey: Self.key); refresh() }
    }

    /// 真正拿来用的字体名：选中的字体本机认得才用（导入、自带的字体要先注册），认不得就是 nil = 系统字体，
    /// 免得整个界面落到最后兜底的那个字体。每处文字都会读它，所以算好存着，不在每次重画时问 NSFontManager
    private(set) var resolved: String?

    private init() { refresh() }

    /// 字体注册完（自带、导入的字体）再认一遍
    func refresh() {
        let name = family.trimmingCharacters(in: .whitespaces)
        let known = !name.isEmpty && (NSFontManager.shared.availableMembers(ofFontFamily: name) != nil || NSFont(name: name, size: 13) != nil)
        let next = known ? name : nil
        if next != resolved { resolved = next }
    }

    /// macOS 各个文字样式的默认字号（和系统的 TextStyle 一致）
    static func size(_ style: Font.TextStyle) -> CGFloat {
        switch style {
        case .largeTitle: 26
        case .title: 22
        case .title2: 17
        case .title3: 15
        case .headline, .body: 13
        case .callout: 12
        case .subheadline: 11
        case .footnote, .caption, .caption2: 10
        @unknown default: 13
        }
    }
}

extension Font {
    /// 界面文字：跟着「界面字体」设置走的 `.caption`、`.title2` ……
    static func app(_ style: Font.TextStyle) -> Font {
        guard let family = AppFont.shared.resolved else { return .system(style) }
        let font = Font.custom(family, size: AppFont.size(style), relativeTo: style)
        return style == .headline ? font.weight(.semibold) : font
    }

    /// 界面文字：跟着「界面字体」设置走的 `.system(size:weight:)`。带 design（等宽、圆体）的保持系统字体
    static func appSystem(size: CGFloat, weight: Font.Weight = .regular, design: Font.Design? = nil) -> Font {
        guard design == nil, let family = AppFont.shared.resolved else { return .system(size: size, weight: weight, design: design) }
        return Font.custom(family, size: size).weight(weight)
    }
}

/// 挂在每个窗口的根上：没写字号的文字（按钮、输入框、列表）都用界面字体。
/// 写成 ViewModifier，body 里读一次 AppFont，换字体时整窗跟着重画
struct AppFontRoot: ViewModifier {
    func body(content: Content) -> some View {
        content.font(.app(.body))
    }
}

/// 界面字体是自带或导入的字体时，要先把字体文件注册进本进程，系统才认得这个名字
@MainActor
enum AppFontLoader {
    static func registerIfNeeded(_ app: AppModel) async {
        guard !AppFont.shared.family.isEmpty, AppFont.shared.resolved == nil, let client = app.core else { return }
        guard let catalog = try? await client.lyricsFontCatalog(imported: app.importedLyricFonts) else { return }
        for path in Set(catalog.files.filter { $0.family == AppFont.shared.family }.map(\.path)) {
            CTFontManagerRegisterFontsForURL(URL(fileURLWithPath: path) as CFURL, .process, nil)
        }
        AppFont.shared.refresh()
    }
}
