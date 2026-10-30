import JPCore
import JPCorpusModel
import SwiftUI

/// 开着振假名、这台电脑上却没有分词词典（Sudachi）时的提示和下载按钮。
///
/// 以前这个原因只在侧栏歌词底下一行小字里，全屏歌词和「歌词显示」里什么都不说，
/// 看上去就像振假名开关坏了。从 Windows 只拷 corpus.db 过来时一定会碰到：
/// 词典在语料库目录的 sudachi/ 里，没跟着过来。
@MainActor
struct FuriganaDictionaryNotice: View {
    enum Style { case plain, card }

    @Environment(AppModel.self) private var app
    var style: Style = .plain
    var compact = false

    private var visible: Bool {
        app.showFurigana && (app.furiganaNeedsDictionary || app.furiganaDictionaryDownloading)
    }

    var body: some View {
        if visible {
            switch style {
            case .plain:
                content
            case .card:
                content
                    .padding(14)
                    .frame(maxWidth: 520, alignment: .leading)
                    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .shadow(color: .black.opacity(0.25), radius: 12, y: 4)
            }
        }
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("振假名需要分词词典（Sudachi），这台电脑上还没有", systemImage: "character.phonetic")
                .font(.callout.weight(.semibold))
                .fixedSize(horizontal: false, vertical: true)
            if !compact {
                Text("词典放在语料库文件夹的 sudachi 里。从别的电脑拷 corpus.db 时把整个文件夹一起拷过来就有；也可以在这里直接下载，只要下一次。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if app.furiganaDictionaryDownloading {
                ProgressView(value: app.furiganaDictionaryProgress) {
                    Text(stageText).font(.caption)
                }
            } else {
                Button("下载词典（43 MB）") { Task { await app.downloadFuriganaDictionary() } }
                    .buttonStyle(.borderedProminent)
            }
            if let error = app.furiganaDictionaryError {
                Text(error).font(.caption).foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var stageText: String {
        switch app.furiganaDictionaryStage {
        case "extracting": return L10n.t("正在解压词典…")
        case "tokenizing": return L10n.t("正在给没分词的歌补分词…")
        default: return L10n.t("正在下载词典…")
        }
    }
}
