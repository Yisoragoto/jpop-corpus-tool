import AppKit
import CoreText
import JPCorpusModel
import Observation
import SwiftUI

/// 整个界面的字体和大小。字体空串 = 系统字体（San Francisco / 苹方），大小 100% = 系统字号，两样都默认时和以前一模一样。
///
/// SwiftUI 没有「把系统字体整体换掉」的开关：`.font(.caption)` 这类写法永远是系统字体。
/// 所以界面里的字号一律写成 `.app(.caption)` / `.appSystem(size:)`，这里按设置决定用系统字体还是选中的字体。
/// `family` 是 Observable：视图在 body 里经过这里读过一次，换字体时就会跟着重画，不用重启。
/// 不绑主线程：Canvas 的绘制闭包里也会画字。实际只在主线程上读写（设置页改、视图重画时读）
@Observable
final class AppFont: @unchecked Sendable {
    static let shared = AppFont()
    static let key = "ui.fontFamily"
    static let scaleKey = "ui.scale"
    /// 界面大小的范围（百分比）和快捷键一格走多少，和 Windows 版一样
    static let scaleRange: ClosedRange<Double> = 30...300
    static let scaleStep = 10.0

    /// 界面大小，百分比。文字（连同跟着文字走的图标）按比例放大缩小，控件换成相近的尺寸档
    var scale: Double = AppFont.clampScale(UserDefaults.standard.object(forKey: AppFont.scaleKey) as? Double ?? 100) {
        // 写入的地方（滑块、快捷键）已经夹好范围，这里只管存
        didSet { UserDefaults.standard.set(scale, forKey: Self.scaleKey) }
    }
    var factor: CGFloat { CGFloat(scale / 100) }

    static func clampScale(_ value: Double) -> Double {
        guard value.isFinite else { return 100 }
        return min(scaleRange.upperBound, max(scaleRange.lowerBound, value.rounded()))
    }

    /// 快捷键：按一格走；dir 为 0 时回到 100%
    func step(_ dir: Int) {
        scale = dir == 0 ? 100 : Self.clampScale((scale / Self.scaleStep).rounded() * Self.scaleStep + Double(dir) * Self.scaleStep)
    }

    /// 控件跟着界面大小换尺寸档：SwiftUI 的按钮、开关、滑块没法按任意比例缩放，只能取最接近的一档
    var controlSize: ControlSize {
        switch scale {
        case ..<60: .mini
        case ..<90: .small
        case ..<120: .regular
        case ..<170: .large
        default: .extraLarge
        }
    }

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
        let ui = AppFont.shared, family = ui.resolved, factor = ui.factor
        if family == nil && factor == 1 { return .system(style) }
        let size = AppFont.size(style) * factor
        let font = family.map { Font.custom($0, size: size, relativeTo: style) } ?? .system(size: size)
        return style == .headline ? font.weight(.semibold) : font
    }

    /// 界面文字：跟着「界面字体」设置走的 `.system(size:weight:)`。带 design（等宽、圆体）的保持系统字体
    static func appSystem(size: CGFloat, weight: Font.Weight = .regular, design: Font.Design? = nil) -> Font {
        let ui = AppFont.shared, scaled = size * ui.factor
        guard design == nil, let family = ui.resolved else { return .system(size: scaled, weight: weight, design: design) }
        return Font.custom(family, size: scaled).weight(weight)
    }
}

/// 挂在每个窗口的根上：没写字号的文字（按钮、输入框、列表）都用界面字体。
/// 写成 ViewModifier，body 里读一次 AppFont，换字体时整窗跟着重画
struct AppFontRoot: ViewModifier {
    func body(content: Content) -> some View {
        content
            .font(.app(.body))
            .controlSize(AppFont.shared.controlSize)
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
