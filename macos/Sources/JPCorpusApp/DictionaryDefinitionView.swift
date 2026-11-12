import AppKit
import Foundation
import JPCore
import SwiftUI
import WebKit

/// 一本词典的释义。和 Windows 的 `Clamp`（TermEntry.tsx）一样：释义按内容高度铺开，**自己不滚**，
/// 滚轮交给外面的查词栏；太长的截到 `clampHeight`、底部渐隐，给「展开全文」。
///
/// 以前是限高 260 的 WKWebView，自带滚动条：指针在释义上时滚轮只滚词条里面，
/// 想翻到下一本词典只能把指针挪到很窄的标题上（#62）。
@MainActor
struct DictionaryDefinitionView: View {
    let definition: LookupDefinition
    let client: CorpusClient
    @Environment(\.colorScheme) private var colorScheme
    @State private var html: String?
    @State private var failure: String?
    /// 网页里量出来的内容高度；0 表示还没量到
    @State private var contentHeight: CGFloat = 0
    @State private var expanded = false
    private static let clampHeight: CGFloat = 260
    /// 只超出一两行就不截了，截了反而更难读（和 Windows 一样留 48）
    private static let slack: CGFloat = 48

    private var overflows: Bool { contentHeight > Self.clampHeight + Self.slack }
    private var clamped: Bool { overflows && !expanded }
    private var shownHeight: CGFloat {
        if contentHeight == 0 { return 60 }
        return clamped ? Self.clampHeight : contentHeight
    }

    var body: some View {
        Group {
            if let html {
                VStack(alignment: .leading, spacing: 2) {
                    DefinitionWebView(html: html, core: client.core, dark: colorScheme == .dark) { height in
                        if abs(height - contentHeight) >= 1 { contentHeight = height }
                    }
                    .frame(height: shownHeight)
                    .mask {
                        LinearGradient(stops: [.init(color: .black, location: 0), .init(color: .black, location: clamped ? 0.8 : 1),
                                               .init(color: clamped ? .clear : .black, location: 1)],
                                       startPoint: .top, endPoint: .bottom)
                    }
                    if overflows {
                        Button(expanded ? "收起" : "展开全文") { expanded.toggle() }
                            .buttonStyle(.link).font(.app(.caption))
                    }
                }
            }
            else { ForEach(Array(definition.text.enumerated()), id: \.offset) { Text($0.element).textSelection(.enabled) } }
            if let failure { Text(failure).font(.app(.caption)).foregroundStyle(.orange) }
        }.task(id: definition) {
            expanded = false
            contentHeight = 0
            guard definition.entries != nil else { return }
            do { html = try await client.dictDefinitionHtml(definition) }
            catch { failure = error.localizedDescription }
        }
    }
}

/// 不接滚轮的 WKWebView：滚轮事件原样交给上一层（查词栏的 ScrollView）。
/// 内容已经按高度铺开，它自己本来也没什么可滚的。
private final class PassThroughWebView: WKWebView {
    override func scrollWheel(with event: NSEvent) {
        nextResponder?.scrollWheel(with: event)
    }
}

/// 注入到应用自己的脚本世界（不受页面的 CSP 和「禁用页面脚本」影响）：
/// 关掉网页自己的滚动条，内容高度一变就报给 Swift。`flow-root` 让子元素的外边距算进 body 的高度。
private let measureScript = """
(() => {
  const style = document.createElement('style');
  style.textContent = 'html, body { overflow: hidden !important; } body { display: flow-root; }';
  (document.head || document.documentElement).appendChild(style);
  const report = () => window.webkit.messageHandlers.definitionHeight.postMessage(Math.ceil(document.body.getBoundingClientRect().height));
  new ResizeObserver(report).observe(document.body);
  report();
})();
"""

@MainActor
private struct DefinitionWebView: NSViewRepresentable {
    let html: String
    let core: CorpusCore
    /// 跟着所在区域的深浅色走（全屏歌词里永远是深色），不跟系统
    let dark: Bool
    /// 内容高度（点），图片加载完会再报一次
    let onHeight: (CGFloat) -> Void
    /// 只允许加载自己的释义页；释义里的外链交给系统浏览器，不在面板里跳走。
    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
        var loadedHTML: String?
        var onHeight: (CGFloat) -> Void = { _ in }
        func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
            guard let height = (message.body as? NSNumber)?.doubleValue, height > 0 else { return }
            onHeight(CGFloat(height))
        }
        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            guard let url = action.request.url, url.scheme != "jpdict", url.scheme != "about" else { return .allow }
            if action.navigationType == .linkActivated, ["http", "https"].contains(url.scheme?.lowercased() ?? "") { NSWorkspace.shared.open(url) }
            return .cancel
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    /// 脚本消息的处理者会被配置一直持有，拆掉视图时摘掉
    static func dismantleNSView(_ view: WKWebView, coordinator: Coordinator) {
        view.configuration.userContentController.removeScriptMessageHandler(forName: "definitionHeight", contentWorld: .defaultClient)
    }
    func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        config.setURLSchemeHandler(DictionaryMedia(core: core), forURLScheme: "jpdict")
        config.userContentController.addUserScript(
            WKUserScript(source: measureScript, injectionTime: .atDocumentEnd, forMainFrameOnly: true, in: .defaultClient))
        config.userContentController.add(context.coordinator, contentWorld: .defaultClient, name: "definitionHeight")
        let view = PassThroughWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        // 透明底：释义铺在面板的玻璃上，不是一块白板
        view.setValue(false, forKey: "drawsBackground")
        return view
    }
    func updateNSView(_ view: WKWebView, context: Context) {
        context.coordinator.onHeight = onHeight
        let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        if view.appearance?.name != appearance?.name { view.appearance = appearance }
        if context.coordinator.loadedHTML != html {
            context.coordinator.loadedHTML = html
            view.loadHTMLString(html, baseURL: URL(string: "jpdict://definition"))
        }
    }
}

/// WebKit 回调不访问 MainActor 模型；只持有线程安全的 Rust 会话。
private final class DictionaryMedia: NSObject, WKURLSchemeHandler {
    let core: CorpusCore
    init(core: CorpusCore) { self.core = core }
    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        do {
            guard let url = task.request.url else { throw URLError(.badURL) }
            let hex = Array(url.lastPathComponent.utf8)
            guard hex.count % 2 == 0 else { throw URLError(.badURL) }
            var bytes: [UInt8] = []
            for i in stride(from: 0, to: hex.count, by: 2) {
                guard let byte = UInt8(String(decoding: hex[i...i+1], as: UTF8.self), radix: 16) else { throw URLError(.badURL) }
                bytes.append(byte)
            }
            let key = try JSONDecoder().decode([String].self, from: Data(bytes))
            guard key.count == 2 else { throw URLError(.badURL) }
            let data: [UInt8] = try core.call("dictMedia", ["dictionary": AnyEncodable(key[0]), "path": AnyEncodable(key[1])])
            let ext = URL(fileURLWithPath: key[1]).pathExtension.lowercased()
            let mime = ["png":"image/png", "jpg":"image/jpeg", "jpeg":"image/jpeg", "gif":"image/gif", "svg":"image/svg+xml", "webp":"image/webp"][ext] ?? "application/octet-stream"
            task.didReceive(URLResponse(url: url, mimeType: mime, expectedContentLength: data.count, textEncodingName: nil))
            task.didReceive(Data(data)); task.didFinish()
        } catch { task.didFailWithError(error) }
    }
    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {}
}
