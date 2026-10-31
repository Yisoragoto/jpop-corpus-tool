import AppKit
import JPCorpusModel
import SwiftUI

/// 二级页面（歌、专辑、音乐人详情，时间树）顶上的返回。
/// 以前只在工具栏里放一个小箭头，和标题栏混在一起很难找；这里放在正文左上角，
/// 快捷键：⌘[、Esc（输入框里按 Esc 不算），鼠标侧键「后退」也行
@MainActor
struct BackBar: View {
    @Environment(AppModel.self) private var app
    var title = "返回"
    let action: () -> Void

    var body: some View {
        HStack {
            Button(action: action) {
                Label(title, systemImage: "chevron.left")
                    .font(.callout.weight(.semibold))
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .background(.regularMaterial, in: Capsule())
                    .overlay(Capsule().strokeBorder(.white.opacity(0.15), lineWidth: 0.5))
                    .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .keyboardShortcut("[", modifiers: [.command])
            .help("返回（⌘[ 或 Esc）")
            .accessibilityIdentifier("back")
            Spacer()
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 8)
        .background(BackKeys(action: action, active: { [app] in !app.showStage }))
    }
}

/// Esc 和鼠标侧键。输入框正在编辑时 Esc 留给输入框；弹出面板时不管
@MainActor
struct BackKeys: NSViewRepresentable {
    let action: () -> Void
    /// 全屏歌词盖在上面时 Esc 是它的
    let active: () -> Bool
    func makeCoordinator() -> Coordinator { Coordinator(action, active) }
    func makeNSView(context: Context) -> NSView { let view = NSView(); context.coordinator.view = view; context.coordinator.install(); return view }
    func updateNSView(_ view: NSView, context: Context) { context.coordinator.action = action; context.coordinator.active = active }
    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) { coordinator.remove() }

    @MainActor final class Coordinator {
        weak var view: NSView?
        var action: () -> Void
        var active: () -> Bool
        private var monitor: Any?
        init(_ action: @escaping () -> Void, _ active: @escaping () -> Bool) { self.action = action; self.active = active }

        func install() {
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .otherMouseDown]) { [weak self] event in
                MainActor.assumeIsolated {
                    guard let self, let window = self.view?.window, event.window === window, window.attachedSheet == nil, self.active() else { return event }
                    switch event.type {
                    case .keyDown where event.keyCode == 53 && event.modifierFlags.intersection(.deviceIndependentFlagsMask).isEmpty:
                        // 光标在输入框里：Esc 是输入框的（取消输入、收起候选）
                        if window.firstResponder is NSTextView { return event }
                        self.action()
                        return nil
                    case .otherMouseDown where event.buttonNumber == 3:
                        self.action()
                        return nil
                    default:
                        return event
                    }
                }
            }
        }

        func remove() {
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil
        }
    }
}
