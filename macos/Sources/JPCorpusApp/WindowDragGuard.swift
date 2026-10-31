import AppKit
import SwiftUI

/// 主窗口开了「拖背景移动窗口」（Glass.swift），SwiftUI 的 Canvas 不是会吃掉鼠标的 NSView，
/// 于是在关系网上拖一个人、在时间轴上拖着平移，系统都当成拖窗口——整个页面跟着走。
/// 在这块区域里按下鼠标时先把窗口的这个开关关掉，松开再恢复。
extension View {
    func keepsWindowStill() -> some View { background(WindowDragGuard()) }
}

@MainActor
private struct WindowDragGuard: NSViewRepresentable {
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        context.coordinator.view = view
        context.coordinator.install()
        return view
    }
    func updateNSView(_ view: NSView, context: Context) {}
    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) { coordinator.remove() }

    @MainActor final class Coordinator {
        weak var view: NSView?
        private var monitor: Any?
        /// 按下时关掉过的窗口和它原来的值，松开时还原
        private var held: (window: NSWindow, movable: Bool)?

        func install() {
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseUp]) { [weak self] event in
                MainActor.assumeIsolated {
                    guard let self else { return event }
                    if event.type == .leftMouseUp {
                        self.restore()
                        return event
                    }
                    guard let view = self.view, let window = view.window, event.window === window else { return event }
                    let p = view.convert(event.locationInWindow, from: nil)
                    if view.bounds.contains(p), window.isMovableByWindowBackground {
                        self.held = (window, true)
                        window.isMovableByWindowBackground = false
                    }
                    return event
                }
            }
        }

        private func restore() {
            if let held { held.window.isMovableByWindowBackground = held.movable }
            held = nil
        }

        func remove() {
            restore()
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil
        }
    }
}
