import JPCore
import JPCorpusModel
import SwiftUI

/// 改读音（#63），和 Windows 的 ReadingEditor 一样：填读音，选只改这首还是全库，「恢复自动」去掉改过的。
/// 存的是规则（按写法记），存完重新取一次振假名就能看到。
@MainActor
struct ReadingEditorView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    let target: ReadingEdit
    @State private var reading = ""
    @State private var wholeLibrary = false
    @State private var rule: ReadingRule?
    @State private var busy = false
    @State private var error: String?

    private var trimmed: String { reading.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Text("改读音").font(.app(.headline))
                Text(verbatim: target.surface).font(.app(.headline))
            }
            TextField("怎么读", text: $reading)
                .textFieldStyle(.roundedBorder)
                .onSubmit { Task { await save() } }
            Toggle("全库里「\(target.surface)」都这么读", isOn: $wholeLibrary)
            if let error {
                Text(verbatim: error).font(.app(.caption)).foregroundStyle(.orange).textSelection(.enabled)
            }
            HStack {
                if let rule {
                    Button("恢复自动") { Task { await clear() } }
                        .buttonStyle(.link)
                        .disabled(busy)
                        .help(rule.wholeLibrary ? "去掉全库的这条读音" : "去掉这首歌里改过的读音")
                }
                Spacer()
                Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("保存") { Task { await save() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(busy || trimmed.isEmpty)
            }
        }
        .padding(18)
        .frame(width: 360)
        .task { await load() }
    }

    private func load() async {
        reading = target.shown
        guard let core = app.core else { return }
        do {
            if let found = try await core.readingRule(songId: target.songId, surface: target.surface) {
                rule = found
                reading = found.reading
                wholeLibrary = found.wholeLibrary
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func save() async {
        guard !busy, !trimmed.isEmpty, let core = app.core else { return }
        busy = true
        defer { busy = false }
        do {
            _ = try await core.setReadingRule(songId: target.songId, surface: target.surface,
                                              reading: trimmed, wholeLibrary: wholeLibrary)
            await app.refreshFurigana()
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func clear() async {
        guard !busy, let core = app.core else { return }
        busy = true
        defer { busy = false }
        do {
            try await core.clearReadingRule(songId: target.songId, surface: target.surface)
            await app.refreshFurigana()
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
