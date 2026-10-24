import AppKit
import JPCorpusModel
import SwiftUI

/// 菜单先于控件处理 keyEquivalent；主动转交输入键，覆盖设置和弹层窗口。
@MainActor
enum PlaybackInputProtection {
    private static var monitor: Any?
    private static weak var pointerResponder: NSResponder?
    static func install() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown]) { event in
            MainActor.assumeIsolated {
                guard let window = event.window else { return event }
                if event.type == .leftMouseDown {
                    DispatchQueue.main.async { [weak window] in pointerResponder = window?.firstResponder }
                    return event
                }
                if event.keyCode == 48 { pointerResponder = nil }
                if owns(event) { window.firstResponder?.keyDown(with: event); return nil }
                return event
            }
        }
    }
    static func owns(_ event: NSEvent) -> Bool {
        guard PlaybackKeyboard.isPlaybackKey(event), let responder = event.window?.firstResponder else { return false }
        let text = (responder as? NSTextView)?.isEditable == true || responder is NSTextField
        let arrows = event.keyCode != 49 && (responder is NSSlider || responder is NSTableView || responder is NSOutlineView)
        let button = event.keyCode == 49 && responder is NSButton && responder !== pointerResponder
        return text || arrows || button
    }
}

/// 先把输入键交还原生控件，再处理播放命令，避免菜单快捷键抢走文字。
@MainActor
struct PlaybackKeyboard: NSViewRepresentable {
    let app: AppModel
    var exit: (() -> Void)?
    func makeCoordinator() -> Coordinator { Coordinator(app: app, exit: exit) }
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        context.coordinator.view = view
        context.coordinator.install()
        return view
    }
    func updateNSView(_ view: NSView, context: Context) { context.coordinator.exit = exit }
    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) { coordinator.remove() }

    static func isPlaybackKey(_ event: NSEvent) -> Bool {
        event.modifierFlags.intersection([.command, .control, .option]).isEmpty && [49, 123, 124, 125, 126].contains(event.keyCode)
    }

    @MainActor
    final class Coordinator {
        weak var view: NSView?
        let app: AppModel
        var exit: (() -> Void)?
        var monitor: Any?
        init(app: AppModel, exit: (() -> Void)?) { self.app = app; self.exit = exit }
        func install() {
            monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                MainActor.assumeIsolated {
                    guard let self, let window = self.view?.window, event.window === window,
                          window.attachedSheet == nil else { return event }
                    if PlaybackInputProtection.owns(event) { return event }
                    if event.keyCode == 53, let exit = self.exit { exit(); return nil }
                    if event.keyCode == 103 { window.toggleFullScreen(nil); return nil }
                    guard PlaybackKeyboard.isPlaybackKey(event) else { return event }
                    let player = self.app.player
                    switch event.keyCode {
                    case 49: if !event.isARepeat { player.toggle() }
                    case 123: Task { await player.nudge(-5) }
                    case 124: Task { await player.nudge(5) }
                    case 126: player.setVolume(player.volume + 0.05)
                    case 125: player.setVolume(player.volume - 0.05)
                    default: return event
                    }
                    return nil
                }
            }
        }
        func remove() { if let monitor { NSEvent.removeMonitor(monitor) }; monitor = nil }
    }
}
