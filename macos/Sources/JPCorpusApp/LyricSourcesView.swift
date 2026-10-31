import JPCore
import JPCorpusModel
import SwiftUI

/// 歌词管理里的「多来源与偏移」：这首歌有哪几份歌词、用的哪一份、译文、整体偏移。
/// 和 Windows 的 LyricSourcesPanel 对应；写操作只有换来源、交还自动选择、选译文、调偏移。
@MainActor
struct LyricSourcesSection: View {
    @Environment(AppModel.self) private var app
    let model: LyricsModel
    @State private var offsetDraft = ""

    /// 微调按钮一次挪多少（毫秒）
    private let steps = [-500, -100, 100, 500]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if let view = model.sourceView {
                    summary(view)
                    originals(view)
                    translations(view)
                    offset
                    lookups(view)
                } else {
                    ProgressView().controlSize(.small)
                }
            }
            .padding(4)
            .disabled(model.busy)
        }
        .task {
            guard let client = app.core else { return }
            await model.loadSources(client)
            offsetDraft = String(Double(model.offsetMs) / 1000)
        }
        .onChange(of: model.offsetMs) { _, value in offsetDraft = String(Double(value) / 1000) }
    }

    @ViewBuilder private func summary(_ view: LyricSourcesView) -> some View {
        if let selection = view.selection {
            HStack(alignment: .firstTextBaseline) {
                if let current = model.currentSource {
                    Text("现在用的是「\(name(current))」（\(kindLabel(current.kind))）。")
                    Text(chosenByLabel(selection.chosenBy)).foregroundStyle(.secondary)
                } else {
                    Text("现在没有选中的来源。")
                }
                Spacer()
                if selection.chosenBy == "user" {
                    Button("交还自动选择") { run { await model.returnToAutomatic($0) } }
                        .help("不再算你选定的：按优先级（你导入或编辑的 → 旁边的文件 → 内嵌 → 在线）挑最好的一份")
                }
            }
            .font(.callout)
        }
        if let recommended = model.recommendedSource {
            HStack {
                Text("按优先级默认会用「\(name(recommended))」（\(kindLabel(recommended.kind))）。").font(.callout)
                Button("用这一份") { run { await model.useSource(recommended.id, client: $0) } }
            }
        }
    }

    @ViewBuilder private func originals(_ view: LyricSourcesView) -> some View {
        let rows = view.sources.filter(\.isOriginal)
        Text("原文").font(.headline)
        if rows.isEmpty {
            Text("没有找到任何歌词来源。可以在音频旁边放一份同名的 .lrc / .srt / .txt，或者在线补齐、自己导入。")
                .font(.caption).foregroundStyle(.secondary)
        }
        ForEach(rows) { row in
            let isCurrent = row.id == view.selection?.sourceId
            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            Text(kindLabel(row.kind)).bold()
                            if !row.provider.isEmpty { Text(row.provider).foregroundStyle(.secondary) }
                            if isCurrent { Text("当前").font(.caption).foregroundStyle(.tint) }
                            if row.userModified { Text("你改过").font(.caption).foregroundStyle(.secondary) }
                        }
                        Text(describe(row)).font(.caption).foregroundStyle(.secondary)
                        if !row.sourceRef.isEmpty {
                            Text(row.sourceRef).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                        }
                    }
                    Spacer()
                    if row.hasRaw {
                        Button(model.sourcePreview?.id == row.id ? "收起预览" : "预览") {
                            run { await model.previewSource(row.id, client: $0) }
                        }
                    }
                    if !isCurrent {
                        Button("用这一份") { run { await model.useSource(row.id, client: $0) } }
                            .disabled(row.unusableReason != nil)
                            .help(row.unusableReason ?? "")
                    }
                }
                if let preview = model.sourcePreview, preview.id == row.id { TimelinePreview(timeline: preview.timeline) }
            }
            .padding(.leading, 10)
            .overlay(alignment: .leading) {
                Rectangle().fill(isCurrent ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary)).frame(width: 2)
            }
        }
    }

    @ViewBuilder private func translations(_ view: LyricSourcesView) -> some View {
        let rows = view.sources.filter { !$0.isOriginal }
        Text("译文").font(.headline)
        if rows.isEmpty {
            Text("没有单独的译文文件（同名加语言后缀，例如 song.zh-Hans.lrc）。歌词文件里和原文同一时间戳的译文行会自动认出来。")
                .font(.caption).foregroundStyle(.secondary)
        } else {
            Picker("译文", selection: Binding(
                get: { view.translationSourceId },
                set: { id in run { await model.selectTranslation(id, client: $0) } }
            )) {
                Text("不用译文").tag(Int64?.none)
                ForEach(rows) { row in
                    Text(row.language.isEmpty ? row.provider : "\(row.provider) (\(row.language))").tag(Int64?.some(row.id))
                }
            }
            .labelsHidden()
            .frame(maxWidth: 320)
        }
    }

    @ViewBuilder private var offset: some View {
        Text("整体偏移").font(.headline)
        if model.sourceUntimed {
            Text("这份歌词没有时间轴，偏移、跟随播放、点行跳转都用不了。").font(.caption).foregroundStyle(.secondary)
        } else {
            HStack {
                ForEach(steps, id: \.self) { step in
                    Button(LyricsModel.formatOffset(step)) { setOffset(model.offsetMs + step) }.monospacedDigit()
                }
                TextField("秒", text: $offsetDraft)
                    .frame(width: 80)
                    .monospacedDigit()
                    .onSubmit {
                        if let sec = Double(offsetDraft), sec.isFinite { setOffset(Int((sec * 1000).rounded())) }
                        else { offsetDraft = String(Double(model.offsetMs) / 1000) }
                    }
                Text("秒")
                Button("归零") { setOffset(0) }.disabled(model.offsetMs == 0)
            }
            Text("现在 \(LyricsModel.formatOffset(model.offsetMs))。正值让歌词晚一点出现。改的是这首歌的时间，原文不动；单句循环、制卡截音频也跟着对齐。")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    @ViewBuilder private func lookups(_ view: LyricSourcesView) -> some View {
        if !view.lookups.isEmpty {
            Text("在线查询").font(.headline)
            ForEach(view.lookups, id: \.provider) { lookup in
                HStack(alignment: .firstTextBaseline) {
                    Text(lookupLabel(lookup.status))
                    if !lookup.message.isEmpty {
                        Text(lookup.message).foregroundStyle(.secondary)
                    } else if !lookup.retryAfter.isEmpty {
                        Text("\(when(lookup.retryAfter)) 之前批量补齐不再查（单首补齐照样会查）").foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text(when(lookup.checkedAt)).foregroundStyle(.secondary)
                }
                .font(.caption)
            }
        }
    }

    private func setOffset(_ milliseconds: Int) { run { await model.setOffset(milliseconds, client: $0) } }
    private func run(_ action: @escaping @MainActor (any LyricsCalling) async -> Void) {
        Task { if let client = app.core { await action(client) } }
    }

    private func name(_ row: LyricSourceRow) -> String { row.provider.isEmpty ? kindLabel(row.kind) : row.provider }
    private func describe(_ row: LyricSourceRow) -> String {
        var parts = [precisionLabel(row.precision), "\(row.lineCount) 行"]
        if !row.rawFormat.isEmpty { parts.append(row.rawFormat == "enhanced_lrc" ? "逐字 LRC" : row.rawFormat.uppercased()) }
        if !row.language.isEmpty { parts.append(row.language) }
        if row.offsetMs != 0 { parts.append("偏移 \(LyricsModel.formatOffset(row.offsetMs))") }
        parts.append("更新于 \(when(row.updatedAt))")
        return parts.joined(separator: " · ")
    }
    private func when(_ iso: String) -> String {
        iso.isEmpty ? "—" : iso.replacingOccurrences(of: "T", with: " ").replacingOccurrences(of: #"(:\d\d)?Z$"#, with: "", options: .regularExpression)
    }
    private func kindLabel(_ kind: String) -> String {
        switch kind {
        case "user": return "你导入或编辑的"
        case "sibling": return "音频旁边的文件"
        case "embedded": return "音频内嵌"
        case "online": return "在线"
        default: return "升级前已有"
        }
    }
    private func precisionLabel(_ precision: String) -> String {
        switch precision {
        case "word": return "逐字"
        case "line": return "逐行"
        default: return "无时间"
        }
    }
    private func chosenByLabel(_ chosenBy: String) -> String {
        switch chosenBy {
        case "user": return "你选定的，自动流程不会换"
        case "auto": return "按优先级自动选的"
        default: return "升级前就在用的，自动流程不会换"
        }
    }
    private func lookupLabel(_ status: String) -> String {
        switch status {
        case "found": return "在线找到了"
        case "not_found": return "在线没找到"
        case "failed": return "在线请求失败，可以重试"
        default: return "在线候选对不准，等你挑"
        }
    }
}

/// 一个来源解析出来的前几行：时间、原文、对上的译文
private struct TimelinePreview: View {
    let timeline: SongLyricTimeline
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(timeline.warnings ?? [], id: \.self) { Text($0).font(.caption).foregroundStyle(.orange) }
            if timeline.translation.state == "unaligned" {
                Text("译文和原文的时间对不上，没有合并显示（不按行号硬拼）。").font(.caption).foregroundStyle(.orange)
            }
            ForEach(timeline.lines.prefix(40)) { line in
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(stamp(line.startMs)).foregroundStyle(.secondary).monospacedDigit()
                    Text(line.text)
                    if let translation = line.translation { Text(translation).foregroundStyle(.secondary) }
                }
                .font(.caption)
            }
            if timeline.lines.count > 40 { Text("…共 \(timeline.lines.count) 行").font(.caption).foregroundStyle(.secondary) }
        }
        .textSelection(.enabled)
        .padding(8)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 6))
    }
    private func stamp(_ ms: Int?) -> String {
        guard let ms else { return "—" }
        let sec = Double(ms) / 1000
        return String(format: "%d:%05.2f", Int(sec) / 60, sec.truncatingRemainder(dividingBy: 60))
    }
}
