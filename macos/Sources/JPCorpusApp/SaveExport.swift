import AppKit
import JPCore
import JPCorpusModel
import SwiftUI
import UniformTypeIdentifiers

@MainActor
enum SaveExport {
    static func text(_ content: String, name: String, title: String, client: CorpusClient) async throws -> String? {
        let panel = NSSavePanel(); panel.title = title; panel.nameFieldStringValue = name
        panel.allowedContentTypes = name.hasSuffix(".csv") ? [.commaSeparatedText] : [.plainText]
        guard panel.runModal() == .OK, let url = panel.url else { return nil }
        try await client.exportText(path: url.path, content: content)
        return url.path
    }
}
