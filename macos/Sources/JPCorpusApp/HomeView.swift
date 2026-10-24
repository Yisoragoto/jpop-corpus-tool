import JPCore
import SwiftUI
import JPCorpusModel

/// 首页：打开就能接着干的地方。和 Windows 版同一份数据（homeSummary）：
/// 继续听、今天听了多少、听过的歌覆盖了多少词、最近播放、收藏、最近添加。每一块都点得进去。
@MainActor
struct HomeView: View {
    @Environment(AppModel.self) private var app
    @State private var home: HomeSummary?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                searchButton
                if let home {
                    HStack(alignment: .top, spacing: 18) {
                        continueCard(home.continueTrack)
                            .frame(maxWidth: .infinity)
                        VStack(alignment: .leading, spacing: 14) {
                            todayCard(home.today, reach: home.reach)
                            corpusCard(home.overview)
                        }
                        .frame(width: 300)
                    }
                    if !home.recentlyPlayed.isEmpty {
                        shelf("最近播放", detail: nil) {
                            ForEach(home.recentlyPlayed) { t in
                                CoverTile(coverPath: t.coverPath, title: t.title, subtitle: "\(t.artist) · \(relativeTime(t.lastPlayed))") {
                                    Task { await app.play(songId: t.songId) }
                                }
                                .contextMenu { songMenu(t.songId) }
                            }
                        }
                    }
                    if !home.favorites.isEmpty {
                        shelf("收藏", detail: "\(home.favorites.count) 首") {
                            ForEach(home.favorites) { t in
                                CoverTile(coverPath: t.coverPath, title: t.title, subtitle: t.artist) {
                                    Task { await app.play(songId: t.id) }
                                }
                                .contextMenu { songMenu(t.id) }
                            }
                        }
                    }
                    if !home.recentlyAdded.isEmpty {
                        recentlyAdded(home.recentlyAdded)
                    }
                } else {
                    ProgressView().frame(maxWidth: .infinity, minHeight: 200)
                }
            }
            .padding(24)
            .frame(maxWidth: 1100, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .clearScrollBackground()
        .navigationTitle("首页")
        .task(id: app.homeVersion) { await load() }
        .task(id: app.tracks.count) { await load() }
    }

    private func load() async {
        guard let client = app.core else { return }
        do {
            home = try await client.homeSummary()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    // MARK: - 各块

    private var searchButton: some View {
        Button {
            app.showQuickSearch = true
        } label: {
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass")
                Text("搜歌、专辑、歌手、词、歌词…").foregroundStyle(.secondary)
                Spacer()
                Text("⌘K").font(.caption.monospaced()).foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .glassCard(cornerRadius: 12)
    }

    @ViewBuilder
    private func continueCard(_ c: ContinueTrack?) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(c?.completed == true ? "再听一遍" : "继续听").font(.headline).foregroundStyle(.secondary)
            if let c {
                HStack(alignment: .top, spacing: 18) {
                    CoverImage(path: c.coverPath, size: 150, cornerRadius: 12)
                        .shadow(color: .black.opacity(0.22), radius: 12, y: 5)
                    VStack(alignment: .leading, spacing: 8) {
                        Text(c.title).font(.title2.weight(.semibold)).lineLimit(2)
                        Text(c.artist).foregroundStyle(.secondary)
                        if !c.stoppedLine.isEmpty {
                            Text("「\(c.stoppedLine)」")
                                .font(.callout)
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                                .help("上次停在这一行")
                        }
                        Spacer(minLength: 4)
                        if let d = c.durationSec, d > 0 {
                            ProgressView(value: min(c.positionSec, d), total: d)
                                .controlSize(.small)
                            Text("\(formatTime(c.positionSec)) / \(formatTime(d)) · \(relativeTime(c.lastPlayed)) · 听过 \(c.playCount) 次")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                        HStack {
                            Button {
                                Task { await app.play(songId: c.songId, at: c.completed ? 0 : c.positionSec) }
                            } label: {
                                Label(c.completed ? "从头放" : "接着放", systemImage: "play.fill")
                            }
                            .buttonStyle(.borderedProminent)
                            Button("歌曲详情") { app.show(.song(c.songId)) }
                        }
                    }
                }
            } else {
                Text("还没有播放记录。去曲库挑一首，或者在检索结果里按空格试听。")
                    .foregroundStyle(.secondary)
                Button("去曲库") { app.section = .library }
            }
        }
        .padding(18)
        .frame(maxWidth: .infinity, minHeight: 210, alignment: .topLeading)
        .glassCard(cornerRadius: 16)
    }

    private func todayCard(_ t: DayListening, reach: Reach) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("今天").font(.headline).foregroundStyle(.secondary)
            HStack(spacing: 18) {
                StatBlock(value: formatListening(t.listenedSec), label: "\(t.songs) 首 · \(t.linesHeard) 行")
                if t.streakDays > 1 {
                    StatBlock(value: "\(t.streakDays) 天", label: "连续在听")
                }
            }
            if reach.totalLemmas > 0 {
                let share = Double(reach.heardLemmas) / Double(reach.totalLemmas)
                VStack(alignment: .leading, spacing: 4) {
                    ProgressView(value: share).controlSize(.small)
                    Text("听过的 \(reach.heardSongs) 首歌覆盖了 \(grouped(reach.heardLemmas)) / \(grouped(reach.totalLemmas)) 个词（\(Int(share * 100))%）")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .help("听满 30 秒的歌才算听过")
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassCard(cornerRadius: 16)
    }

    private func corpusCard(_ o: Overview) -> some View {
        Button {
            app.section = .analysis
        } label: {
            VStack(alignment: .leading, spacing: 10) {
                Text("语料库").font(.headline).foregroundStyle(.secondary)
                Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 8) {
                    GridRow {
                        StatBlock(value: grouped(o.tracks), label: "首歌")
                        StatBlock(value: grouped(o.lyricLines), label: "行歌词")
                    }
                    GridRow {
                        StatBlock(value: grouped(o.tokens), label: "词次")
                        StatBlock(value: grouped(o.vocabulary), label: "个不同的词")
                    }
                }
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .glassCard(cornerRadius: 16)
        .help("去分析页")
    }

    private func shelf<Content: View>(_ title: String, detail: String?, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader(title: title, detail: detail)
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: 16) { content() }
                    .padding(.vertical, 6)
                    .padding(.horizontal, 2)
            }
            .scrollIndicators(.never)
        }
    }

    private func recentlyAdded(_ items: [AddedTrack]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeader(title: "最近添加", detail: nil)
            VStack(spacing: 0) {
                ForEach(items) { t in
                    Button {
                        app.show(.song(t.songId))
                    } label: {
                        HStack(spacing: 12) {
                            CoverImage(path: t.coverPath, size: 36, cornerRadius: 6)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(t.title).lineLimit(1)
                                Text(t.artist).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                            }
                            Spacer()
                            Text(relativeTime(t.addedAt)).font(.caption).foregroundStyle(.tertiary)
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .contextMenu { songMenu(t.songId) }
                    if t.id != items.last?.id { Divider().padding(.leading, 60) }
                }
            }
            .glassCard(cornerRadius: 14)
        }
    }

    @ViewBuilder
    private func songMenu(_ songId: String) -> some View {
        Button("播放") { Task { await app.play(songId: songId) } }
        Button("歌曲详情") { app.show(.song(songId)) }
    }
}
