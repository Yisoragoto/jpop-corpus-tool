import JPCore
import SwiftUI

/// 歌曲详情：谁唱谁写、这首歌的语料画像、歌词从哪来（来源追踪）。
/// 和 Windows 版曲库右栏是同一批数据：trackCredits / songProfile / songLineage
@MainActor
struct SongDetailView: View {
    @Environment(AppModel.self) private var app
    let songId: String
    @State private var track: Track?
    @State private var credits: [Credit] = []
    @State private var profile: SongProfile?
    @State private var lineage: SongLineage?
    @State private var favorite = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                if let track {
                    header(track)
                    HStack(alignment: .top, spacing: 18) {
                        VStack(alignment: .leading, spacing: 18) {
                            creditsCard
                            if let profile { profileCard(profile) }
                        }
                        .frame(maxWidth: .infinity)
                        if let lineage { lineageCard(lineage, track: track) }
                    }
                } else {
                    ProgressView().frame(maxWidth: .infinity, minHeight: 200)
                }
            }
            .padding(24)
        }
        .clearScrollBackground()
        .navigationTitle(track?.title ?? "歌曲")
        .task(id: songId) { await load() }
    }

    private func load() async {
        guard let client = app.client else { return }
        do {
            track = try await app.track(songId)
            async let c = client.trackCredits(songId)
            async let l = client.songLineage(songId)
            async let f = client.isFavorite(song: songId)
            credits = try await c
            lineage = try? await l
            favorite = (try? await f) ?? false
            // 没分词的歌算不出画像，不算错
            profile = (track?.lineCount ?? 0) > 0 ? try? await client.songProfile(songId) : nil
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func header(_ t: Track) -> some View {
        HStack(alignment: .bottom, spacing: 20) {
            CoverImage(path: t.coverPath, size: 180, cornerRadius: 12)
                .shadow(color: .black.opacity(0.22), radius: 14, y: 6)
            VStack(alignment: .leading, spacing: 6) {
                Text("歌曲").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                Text(t.title).font(.largeTitle.weight(.bold)).lineLimit(2).textSelection(.enabled)
                Text([t.artist, t.album, t.year, t.genre].filter { !$0.isEmpty }.joined(separator: " · "))
                    .foregroundStyle(.secondary)
                Text([t.durationSec.map(formatTime), t.lineCount > 0 ? "\(t.lineCount) 行歌词" : "没有歌词"].compactMap { $0 }.joined(separator: " · "))
                    .font(.callout)
                    .foregroundStyle(.tertiary)
                HStack {
                    Button {
                        Task { await app.play(songId: t.id) }
                    } label: {
                        Label("播放", systemImage: "play.fill")
                    }
                    .buttonStyle(.borderedProminent)
                    Button {
                        Task {
                            if let now = try? await app.client?.toggleFavorite(song: t.id) {
                                favorite = now
                                app.homeVersion += 1
                            }
                        }
                    } label: {
                        Label(favorite ? "已收藏" : "收藏", systemImage: favorite ? "heart.fill" : "heart")
                    }
                    if let albumId = t.albumId {
                        Button("去专辑") {
                            Task {
                                if let album = try? await app.client?.albums().first(where: { $0.id == albumId }) {
                                    app.libraryPath.append(.album(album))
                                }
                            }
                        }
                    }
                }
                .padding(.top, 6)
            }
        }
    }

    // MARK: - 制作人员

    private var creditsCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("制作人员").font(.headline)
            if credits.isEmpty {
                Text("没有记录").foregroundStyle(.secondary)
            } else {
                Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 8) {
                    ForEach(["performer", "composer", "lyricist", "arranger"], id: \.self) { role in
                        let people = credits.filter { $0.role == role }.sorted { $0.position < $1.position }
                        if !people.isEmpty {
                            GridRow {
                                Text(roleLabels[role] ?? role).foregroundStyle(.secondary)
                                FlowLinks(items: people.map { ($0.personId, $0.name) }) { id in
                                    app.libraryPath.append(.person(id))
                                }
                            }
                        }
                    }
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassCard(cornerRadius: 14)
    }

    // MARK: - 语料画像

    private func profileCard(_ p: SongProfile) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("语料画像").font(.headline)
            HStack(spacing: 22) {
                StatBlock(value: "\(p.lines)", label: "行")
                StatBlock(value: p.lines > 0 ? "\(Int(Double(p.repeatedLines) / Double(p.lines) * 100))%" : "—", label: "是重复行")
                StatBlock(value: "\(p.distinctTokens)", label: "个不同的词")
            }
            if !p.keywords.isEmpty {
                Text("相对其余的歌，这首歌偏爱的词（双击去检索）")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                WrapChips(words: p.keywords.map(\.lemma)) { app.search($0) }
            }
            ProvenanceFooter(p: p.provenance).padding(.horizontal, -10)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassCard(cornerRadius: 14)
    }

    // MARK: - 来源追踪

    private func lineageCard(_ l: SongLineage, track: Track) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("歌词来源").font(.headline)
            if l.versions.isEmpty {
                Text("没有版本记录").foregroundStyle(.secondary)
            }
            ForEach(l.versions) { v in
                VStack(alignment: .leading, spacing: 3) {
                    HStack {
                        Text("版本 \(v.versionNo)").font(.callout.weight(.medium))
                        if v.isCurrent {
                            Text("当前").font(.caption2.weight(.semibold))
                                .padding(.horizontal, 6).padding(.vertical, 1)
                                .background(.tint.opacity(0.2), in: Capsule())
                        }
                        Spacer()
                        if let c = v.completeness {
                            Text("完整度 \(Int(c * 100))%").font(.caption.monospacedDigit())
                                .foregroundStyle(c < 0.8 ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
                        }
                    }
                    Text([sourceLabel(v.sourceType), v.sourceName].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text("\(v.lineCount) 行，\(v.timedLineCount) 行有时间轴 · \(relativeTime(v.importedAt))")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    if !v.note.isEmpty {
                        Text(v.note).font(.caption).foregroundStyle(.secondary)
                    }
                }
                .padding(.vertical, 4)
                if v.id != l.versions.last?.id { Divider() }
            }
            if !l.provenance.isEmpty {
                Divider().padding(.vertical, 4)
                Text("元数据从哪来").font(.subheadline.weight(.medium))
                ForEach(l.provenance, id: \.field) { f in
                    HStack {
                        Text(f.field).foregroundStyle(.secondary)
                        Spacer()
                        Text(f.source)
                        if let c = f.confidence {
                            Text("\(Int(c * 100))%").foregroundStyle(.tertiary).monospacedDigit()
                        }
                    }
                    .font(.caption)
                }
            }
            Text("歌词换版本、逐行修改目前只在 Windows 版里做。")
                .font(.caption2)
                .foregroundStyle(.tertiary)
                .padding(.top, 4)
        }
        .padding(16)
        .frame(width: 320, alignment: .leading)
        .glassCard(cornerRadius: 14)
    }

    private func sourceLabel(_ type: String) -> String {
        ["legacy": "老库补登记", "lrc_file": "本地 .lrc", "embedded": "音频内嵌", "online": "在线歌词", "manual": "手动编辑"][type] ?? type
    }
}

/// 一排可点的人名
@MainActor
private struct FlowLinks: View {
    let items: [(Int64, String)]
    let action: (Int64) -> Void

    var body: some View {
        HStack(spacing: 4) {
            ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                Button(item.1) { action(item.0) }
                    .buttonStyle(.link)
                if index < items.count - 1 { Text("、").foregroundStyle(.secondary) }
            }
        }
    }
}

/// 自动换行的词块
@MainActor
struct WrapChips: View {
    let words: [String]
    let action: (String) -> Void

    var body: some View {
        FlowLayout(spacing: 6) {
            ForEach(words, id: \.self) { w in
                Text(w)
                    .font(.callout)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
                    .background(.quaternary.opacity(0.6), in: Capsule())
                    .contentShape(Capsule())
                    .onTapGesture(count: 2) { action(w) }
                    .help("双击检索「\(w)」")
            }
        }
    }
}

/// 简单的流式布局：一行放不下就换行
struct FlowLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, rowHeight: CGFloat = 0, maxX: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > 0 && x + size.width > width {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            x += size.width + spacing
            maxX = max(maxX, x - spacing)
            rowHeight = max(rowHeight, size.height)
        }
        return CGSize(width: min(maxX, width), height: y + rowHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX, y = bounds.minY, rowHeight: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > bounds.minX && x + size.width > bounds.maxX {
                x = bounds.minX
                y += rowHeight + spacing
                rowHeight = 0
            }
            view.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
    }
}
