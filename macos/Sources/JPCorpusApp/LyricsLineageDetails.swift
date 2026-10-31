import JPCore
import JPCorpusModel
import SwiftUI

@MainActor
struct LyricsLineageDetails: View {
    @Environment(AppModel.self) private var app
    @Bindable var model: LyricsModel
    @State private var showInfo = false
    var body: some View {
        if let data = model.lineage {
            VStack(alignment: .leading, spacing: 10) {
                if let current = data.versions.first(where: \.isCurrent) {
                    Text("完整度 \(percent(current.completeness)) · \(current.lineCount) 行，\(current.timedLineCount) 行有时间").bold()
                    Text("来源：\(codeLabel(lyricSourceLabels, current.sourceType)) · \(current.sourceName) · \(current.sourceRef)").font(.caption).textSelection(.enabled)
                    let low = (data.lines ?? []).filter { ($0.timeConfidence ?? 1) < 0.5 }.count
                    if low > 0 { Text("\(low) 行时间戳不可靠").foregroundStyle(.orange) }
                    if let validation = current.validation {
                        Text("重复行占比 \(percent(validation.repeatedLineRatio))").font(.caption)
                        let serious = validation.issues.filter { $0.severity != "info" }
                        let minor = validation.issues.filter { $0.severity == "info" }
                        if serious.isEmpty { Text("校验没有发现需要注意的问题。").font(.caption) }
                        issues(serious)
                        if !minor.isEmpty {
                            Toggle("另有 \(minor.count) 条提示", isOn: $showInfo).toggleStyle(.button)
                            if showInfo { issues(minor) }
                        }
                    }
                } else { Text("这首歌还没有歌词版本记录。").foregroundStyle(.secondary) }
                HStack {
                    Picker("歌词语言", selection: Binding(get: { data.language ?? "" }, set: { language in
                        Task { if let client = app.core { await model.setLanguage(language, client: client) } }
                    })) {
                        ForEach(["", "ja", "en", "ko", "zh", "mixed"], id: \.self) { code in
                            Text(["": "未定", "ja": "日语", "en": "英语", "ko": "韩语", "zh": "中文", "mixed": "混合"][code] ?? code).tag(code)
                        }
                    }.frame(width: 210).disabled(model.busy)
                    if data.provenance.contains(where: { $0.field == "language" && $0.source == "manual" }) {
                        Text("你定的").font(.caption)
                    } else if let guess = data.versions.first(where: \.isCurrent)?.validation?.language {
                        Text("推断（\(percent(guess.confidence))）").font(.caption)
                    }
                }
                Text("分词：\((data.tokenStatus ?? [:]).keys.sorted().map { "\(tokenLabel($0)) \(data.tokenStatus?[$0] ?? 0)" }.joined(separator: " · "))")
                    .font(.caption)
                ForEach(data.tokenizers ?? []) { run in
                    Text("\(run.engine) \(run.engineVersion) · \(run.dictDescription) · \(run.splitMode) · \(run.lineCount) 行")
                        .font(.caption).help("指纹 \(run.fingerprint) · 配置 \(run.configHash)")
                }
                if let files = data.audioFiles, !files.isEmpty {
                    DisclosureGroup("音频文件") {
                        ForEach(files) { file in
                            VStack(alignment: .leading, spacing: 2) {
                                Text("\(audioStatus(file.status)) · \(file.summary.isEmpty ? "—" : file.summary)").font(.caption)
                                Text(file.path).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle).textSelection(.enabled)
                            }
                        }
                        Text("曲目的身份不跟文件走：文件挪了、换了音源，收藏、播放记录、歌词都还在。程序只读音频文件，从不往里写。")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                if !data.provenance.isEmpty {
                    DisclosureGroup("字段来源") {
                        ForEach(data.provenance, id: \.field) { p in
                            Text("\(codeLabel(fieldLabels, p.field)) · \(codeLabel(fieldSourceLabels, p.source)) · \(percent(p.confidence)) · \(p.updatedAt)").font(.caption)
                        }
                    }
                }
                if let revisions = data.revisions, !revisions.isEmpty {
                    DisclosureGroup("修订历史") {
                        ForEach(revisions) { r in
                            Text("\(r.createdAt) · \(revisionLabel(r.kind))\(r.lineIdx.map { " · 第 \($0 + 1) 行" } ?? "") · \(revisionDetail(r))")
                                .font(.caption).textSelection(.enabled)
                        }
                    }
                }
                Divider()
            }
        }
    }
    private func issues(_ items: [LyricsIssue]) -> some View {
        ForEach(Array(items.enumerated()), id: \.offset) { _, item in
            Text("\(item.lineIdx.map { "第 \($0 + 1) 行 · " } ?? "")\(codeLabel(issueLabels, item.code)) · \(item.message)")
                .font(.caption).foregroundStyle(item.severity == "info" ? Color.secondary : .orange)
        }
    }
    private func audioStatus(_ status: String) -> String { ["present": "在用", "missing": "找不到", "replaced": "已换掉"][status] ?? status }
    private func percent(_ number: Double?) -> String { number.map { String(format: "%.0f%%", $0 * 100) } ?? "—" }
    private func tokenLabel(_ status: String) -> String { ["pending": "未分词", "auto": "自动分词", "corrected": "已校正", "empty": "无实词"][status] ?? status }
    private func revisionLabel(_ kind: String) -> String {
        ["lyrics_attached": "挂上歌词", "lyrics_replaced": "更换歌词", "lyrics_edited": "逐行编辑", "lyrics_timed": "打轴", "retokenized": "重新分词", "corrections_restored": "恢复分词校正", "metadata_edited": "修改元数据"][kind] ?? kind
    }
    private func revisionDetail(_ r: LyricRevision) -> String {
        switch r.kind {
        case "lyrics_attached", "lyrics_replaced": return "\(r.after?["sourceType"]?.text ?? "") · \(r.after?["lines"]?.text ?? "?") 行"
        case "retokenized": return r.after?["tokenizer"]?.text ?? ""
        case "corrections_restored": return "\(r.after?["lines"]?.text ?? "?") 行"
        case "lyrics_edited", "lyrics_timed": if case .some(.array(let rows)) = r.after { return "改完 \(rows.count) 行" }; return ""
        case "metadata_edited": return r.after?.text ?? ""
        default: return ""
        }
    }
}
