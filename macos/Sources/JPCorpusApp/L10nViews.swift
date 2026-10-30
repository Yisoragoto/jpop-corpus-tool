import JPCore
import SwiftUI

// 英文界面的入口：给常用控件加一个收 String 的版本，显示前过一遍 `L10n.t`。
//
// 写成 `Text("检索")` 的字面量，Swift 优先选 String（字面量的默认类型）而不是 LocalizedStringKey，
// 所以界面里直接写的中文、拼出来的句子（"\(n) 首"）、Rust 返回的报错都从这里走，和 Windows 的 tr() 一样。
// 不是中文的（歌名、数字、路径）原样返回；中文界面下什么都不做。
// 没覆盖到的 LocalizedStringKey 接口（提示框标题、Section、菜单名）走 .app 里的 Localizable.strings。

extension Text {
    init(_ content: String) { self.init(verbatim: L10n.t(content)) }
}

extension Button where Label == Text {
    init(_ title: String, action: @escaping () -> Void) {
        self.init(action: action) { Text(verbatim: L10n.t(title)) }
    }

    init(_ title: String, role: ButtonRole?, action: @escaping () -> Void) {
        self.init(role: role, action: action) { Text(verbatim: L10n.t(title)) }
    }
}

extension Label where Title == Text, Icon == Image {
    init(_ title: String, systemImage: String) {
        self.init { Text(verbatim: L10n.t(title)) } icon: { Image(systemName: systemImage) }
    }
}

extension Toggle where Label == Text {
    init(_ title: String, isOn: Binding<Bool>) {
        self.init(isOn: isOn) { Text(verbatim: L10n.t(title)) }
    }
}

extension View {
    func help(_ text: String) -> some View { help(Text(verbatim: L10n.t(text))) }
    func navigationTitle(_ title: String) -> some View { navigationTitle(Text(verbatim: L10n.t(title))) }
    func navigationSubtitle(_ subtitle: String) -> some View { navigationSubtitle(Text(verbatim: L10n.t(subtitle))) }
}

extension Menu where Label == Text {
    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.init(content: content) { Text(verbatim: L10n.t(title)) }
    }
}
