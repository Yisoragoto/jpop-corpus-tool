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

// 下面这些控件的 String 标题默认走 LocalizedStringKey → 系统的 Localizable.strings，跟的是系统语言，
// 应用里切成英文时它们还是中文（导入页的「语料库文件夹」「选择音乐文件夹」就是这样漏的）。
// 一律改走 L10n。转成 Substring 是为了调到 SwiftUI 的 StringProtocol 版本，而不是又回到这里。

extension DisclosureGroup where Label == Text {
    init(_ title: String, @ViewBuilder content: @escaping () -> Content) {
        self.init(Substring(L10n.t(title)), content: content)
    }
    init(_ title: String, isExpanded: Binding<Bool>, @ViewBuilder content: @escaping () -> Content) {
        self.init(Substring(L10n.t(title)), isExpanded: isExpanded, content: content)
    }
}

extension ContentUnavailableView where Label == SwiftUI.Label<Text, Image>, Description == Text?, Actions == EmptyView {
    init(_ title: String, systemImage: String, description: Text? = nil) {
        self.init(Substring(L10n.t(title)), systemImage: systemImage, description: description)
    }
}

extension TextField where Label == Text {
    init(_ title: String, text: Binding<String>) {
        self.init(Substring(L10n.t(title)), text: text)
    }
}

extension SecureField where Label == Text {
    init(_ title: String, text: Binding<String>) {
        self.init(Substring(L10n.t(title)), text: text)
    }
}

extension Picker where Label == Text {
    init(_ title: String, selection: Binding<SelectionValue>, @ViewBuilder content: () -> Content) {
        self.init(Substring(L10n.t(title)), selection: selection, content: content)
    }
}

extension Stepper where Label == Text {
    init<V: Strideable>(_ title: String, value: Binding<V>, in bounds: ClosedRange<V>) {
        self.init(Substring(L10n.t(title)), value: value, in: bounds)
    }
}

extension ProgressView where Label == Text, CurrentValueLabel == EmptyView {
    init(_ title: String) {
        self.init(Substring(L10n.t(title)))
    }
}

extension CommandMenu {
    init(_ name: String, @ViewBuilder content: () -> Content) {
        self.init(Substring(L10n.t(name)), content: content)
    }
}

extension View {
    func alert<A: View>(_ title: String, isPresented: Binding<Bool>, @ViewBuilder actions: () -> A) -> some View {
        alert(Text(verbatim: L10n.t(title)), isPresented: isPresented, actions: actions)
    }
    func alert<A: View, M: View>(_ title: String, isPresented: Binding<Bool>, @ViewBuilder actions: () -> A, @ViewBuilder message: () -> M) -> some View {
        alert(Text(verbatim: L10n.t(title)), isPresented: isPresented, actions: actions, message: message)
    }
    func accessibilityLabel(_ label: String) -> some View {
        accessibilityLabel(Text(verbatim: L10n.t(label)))
    }
}
