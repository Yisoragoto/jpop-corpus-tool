import AppKit
import Foundation
import JPCore
import SwiftUI
import WebKit

@MainActor
struct DictionaryDefinitionView: View {
    let definition: LookupDefinition
    let client: CorpusClient
    @State private var html: String?
    @State private var failure: String?
    var body: some View {
        Group {
            if let html { DefinitionWebView(html: html, core: client.core).frame(minHeight: 120, idealHeight: 180, maxHeight: 260) }
            else { ForEach(Array(definition.text.enumerated()), id: \.offset) { Text($0.element).textSelection(.enabled) } }
            if let failure { Text(failure).font(.caption).foregroundStyle(.orange) }
        }.task(id: definition) {
            guard definition.entries != nil else { return }
            do { html = try await client.dictDefinitionHtml(definition) }
            catch { failure = error.localizedDescription }
        }
    }
}

@MainActor
private struct DefinitionWebView: NSViewRepresentable {
    let html: String
    let core: CorpusCore
    /// 只允许加载自己的释义页；释义里的外链交给系统浏览器，不在面板里跳走。
    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate {
        var loadedHTML: String?
        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            guard let url = action.request.url, url.scheme != "jpdict", url.scheme != "about" else { return .allow }
            if action.navigationType == .linkActivated, ["http", "https"].contains(url.scheme?.lowercased() ?? "") { NSWorkspace.shared.open(url) }
            return .cancel
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        config.setURLSchemeHandler(DictionaryMedia(core: core), forURLScheme: "jpdict")
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        return view
    }
    func updateNSView(_ view: WKWebView, context: Context) {
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
