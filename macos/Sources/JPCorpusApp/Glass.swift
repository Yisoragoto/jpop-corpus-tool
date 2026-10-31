import AppKit
import SwiftUI

/// 真毛玻璃：`NSVisualEffectView`，窗口**后面**的桌面透上来、被系统模糊（behindWindow），
/// 和 Finder 侧栏、控制中心是同一种材质。不是半透明颜色模仿的。
///
/// 状态固定成 `.active`：默认跟着窗口是否在前台走，切到后台整个窗口会变成一块灰板，
/// 对一个常开着边听边看的播放器来说太突兀。
@MainActor
struct GlassBackground: NSViewRepresentable {
    var material: NSVisualEffectView.Material = .underWindowBackground
    var blending: NSVisualEffectView.BlendingMode = .behindWindow

    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.state = .active
        view.material = material
        view.blendingMode = blending
        return view
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {
        view.material = material
        view.blendingMode = blending
    }
}

/// 把窗口本身调成适合毛玻璃的样子：标题栏透明、内容铺到标题栏下面、可以拖背景移动窗口。
///
/// 全屏时系统默认让工具栏一直显示，还给它垫一块不透明的灰底（toolbarBackground(.hidden) 管不到全屏），
/// 顶上就多出一条去不掉的灰带。全屏期间改成工具栏和菜单栏一起自动隐藏，鼠标移到顶上才滑出来
@MainActor
struct GlassWindowConfigurator: NSViewRepresentable {
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSView {
        let view = NSView(), coordinator = context.coordinator
        DispatchQueue.main.async {
            guard let window = view.window else { return }
            window.titlebarAppearsTransparent = true
            window.styleMask.insert(.fullSizeContentView)
            window.isMovableByWindowBackground = true
            coordinator.watch(window)
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {}

    static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) { coordinator.stop() }

    @MainActor final class Coordinator {
        private var observers: [NSObjectProtocol] = []

        func watch(_ window: NSWindow) {
            stop()
            let center = NotificationCenter.default
            observers.append(center.addObserver(forName: NSWindow.didEnterFullScreenNotification, object: window, queue: .main) { _ in
                MainActor.assumeIsolated { NSApp.presentationOptions = [.fullScreen, .autoHideToolbar, .autoHideMenuBar] }
            })
            observers.append(center.addObserver(forName: NSWindow.willExitFullScreenNotification, object: window, queue: .main) { _ in
                MainActor.assumeIsolated { NSApp.presentationOptions = [] }
            })
        }

        func stop() {
            observers.forEach(NotificationCenter.default.removeObserver)
            observers = []
        }
    }
}

extension View {
    /// 整块区域铺毛玻璃（铺满到窗口边缘，包括标题栏下面）
    func glass(_ material: NSVisualEffectView.Material = .underWindowBackground) -> some View {
        background(GlassBackground(material: material).ignoresSafeArea())
    }

    /// 表格、列表去掉自己的不透明底和斑马纹，让下面的毛玻璃透出来
    func clearScrollBackground() -> some View {
        scrollContentBackground(.hidden)
    }

    /// 浮在玻璃上的一张「卡片」：更浅一层的材质 + 圆角 + 细描边 + 轻阴影
    func glassCard(cornerRadius: CGFloat = 14) -> some View {
        background(.regularMaterial, in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .strokeBorder(.white.opacity(0.18), lineWidth: 0.5)
            )
            .shadow(color: .black.opacity(0.12), radius: 12, y: 4)
    }
}

/// 封面图（磁盘上的文件）。读不到就是 nil，调用方降级
func loadImage(atPath path: String?) -> NSImage? {
    guard let path, !path.isEmpty else { return nil }
    return NSImage(contentsOfFile: path)
}
