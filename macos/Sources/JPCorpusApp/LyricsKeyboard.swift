import AppKit
import SwiftUI

/// 只处理所在窗口的键盘；文本输入保留系统编辑行为，按钮焦点不抢空格。
@MainActor
struct LyricsKeyboard: NSViewRepresentable {
    var action: (NSEvent) -> Bool
    func makeCoordinator() -> Coordinator { Coordinator(action) }
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        context.coordinator.view = view
        context.coordinator.install()
        return view
    }
    func updateNSView(_ nsView: NSView, context: Context) { context.coordinator.action = action }
    static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) { coordinator.remove() }

    @MainActor final class Coordinator {
        weak var view: NSView?
        var action: (NSEvent) -> Bool
        private var monitor: Any?
        init(_ action: @escaping (NSEvent) -> Bool) { self.action = action }
        func install() {
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] event in
                MainActor.assumeIsolated {
                    guard let self, let window = self.view?.window, event.window === window,
                          window.attachedSheet == nil else { return event }
                    if let input = window.firstResponder as? NSTextView, input.isEditable { return event }
                    return self.action(event) ? nil : event
                }
            }
        }
        func remove() { if let monitor { NSEvent.removeMonitor(monitor) }; monitor = nil }
    }
}
