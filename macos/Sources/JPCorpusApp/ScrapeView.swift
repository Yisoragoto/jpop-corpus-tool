import JPCore
import SwiftUI
import JPCorpusModel

/// 刮削页：从 MusicBrainz / iTunes / Deezer 补元数据（专辑、年份、流派、发行日期）、封面和歌手照片。
/// 和 Windows 版同一份逻辑（jp-scrape）：**只补空的，不覆盖本地原有的值**；拿不准的停在复核队列等你确认。
@MainActor
struct ScrapeView: View {
    @Environment(AppModel.self) private var app
    @State private var completeness: Completeness?
    @State private var summary: ScrapeSummary?
    @State private var job: JobProgress<ScrapeBatch>?
    @State private var lastBatch: ScrapeBatch?
    @State private var review: [ReviewItem] = []
    @State private var message: String?
    @State private var busy = false

    private var running: Bool { job?.running == true }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                Text("从 MusicBrainz、iTunes、Deezer 查这首歌是哪张专辑、哪一年、什么流派，顺便下封面；歌手查资料和照片。只补空着的字段，本地原有的值不动。拿不准的不会自动写，放在下面的复核队列里等你选。")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                completenessCard
                actions
                if let job, job.running { progress(job) }
                if let lastBatch, !lastBatch.items.isEmpty { results(lastBatch) }
                reviewSection
                ArtistRosterView()
            }
            .padding(24)
            .frame(maxWidth: 960, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .clearScrollBackground()
        .navigationTitle("刮削")
        .task { await reload() }
        .task { await resumeIfRunning() }
    }

    // MARK: - 完整度

    private var completenessCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader(title: "库的完整度", detail: summary.map { "已识别 \($0.success) · 待确认 \($0.lowConfidence) · 失败 \($0.failed) · 跳过 \($0.skipped)" })
            if let c = completeness {
                Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 8) {
                    bar("识别出是哪首歌", c.identified)
                    bar("曲目封面", c.trackCovers)
                    bar("专辑封面", c.albumCovers)
                    bar("歌词", c.lyrics)
                    bar("时长", c.durations)
                }
            } else {
                ProgressView().controlSize(.small)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassCard(cornerRadius: 14)
    }

    private func bar(_ label: String, _ count: CountPair) -> some View {
        GridRow {
            Text(label).foregroundStyle(.secondary)
            ProgressView(value: count.fraction).frame(maxWidth: 360)
            Text("\(count.done) / \(count.total)").monospacedDigit().font(.callout)
        }
    }

    // MARK: - 动作

    private var actions: some View {
        HStack(spacing: 10) {
            Button {
                Task { await start(artists: false) }
            } label: {
                Label("刮削曲目", systemImage: "wand.and.stars")
            }
            .buttonStyle(.borderedProminent)
            .help("只刮还没识别、也没被跳过的歌。整库一两百首大约十分钟，可以随时停")
            Button {
                Task { await start(artists: true) }
            } label: {
                Label("刮削歌手照片", systemImage: "person.crop.square")
            }
            .help("还没有照片的歌手：MusicBrainz 查资料和别名，Deezer 拿照片")
            Button {
                Task { await backfill() }
            } label: {
                Label("补封面", systemImage: "photo.on.rectangle")
            }
            .help("识别成功但没有封面的：先用音频里内嵌的，再用刮削时存下的候选。不重新搜索")
            if (summary?.failed ?? 0) > 0 {
                Button("重试失败的 \(summary?.failed ?? 0) 首") { Task { await retryFailed() } }
            }
            Button("补专辑信息") { Task {
                do { _ = try await app.core?.scrapeAlbumFillStart(); await follow() }
                catch { app.errorMessage = error.localizedDescription }
            } }
            Spacer()
        }
        .disabled(running || busy)
    }

    private func progress(_ job: JobProgress<ScrapeBatch>) -> some View {
        HStack(spacing: 10) {
            if let f = job.fraction {
                ProgressView(value: f).frame(maxWidth: 300)
            } else {
                ProgressView().controlSize(.small)
            }
            Text("\(job.done) / \(job.total)  \(job.stage)").font(.caption.monospacedDigit()).foregroundStyle(.secondary).lineLimit(1)
            Button("停止") { Task { try? await app.core?.scrapeCancel() } }
                .controlSize(.small)
        }
    }

    private func results(_ batch: ScrapeBatch) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if let message { Text(message).font(.callout) }
            ForEach(Array(batch.items.suffix(30).enumerated()), id: \.offset) { _, item in
                HStack(spacing: 8) {
                    statusIcon(item.status)
                    Text(item.title).lineLimit(1)
                    if !item.matched.isEmpty {
                        Text("→ \(item.matched)").foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer()
                    if item.coverSaved { Image(systemName: "photo").foregroundStyle(.secondary).help("下到了图") }
                }
                .font(.callout)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassCard(cornerRadius: 12)
    }

    private func statusIcon(_ status: String) -> some View {
        let (symbol, style): (String, AnyShapeStyle) = switch status {
        case "success": ("checkmark.circle.fill", AnyShapeStyle(.green))
        case "low_confidence": ("questionmark.circle.fill", AnyShapeStyle(.orange))
        case "skipped": ("minus.circle", AnyShapeStyle(.secondary))
        default: ("xmark.circle.fill", AnyShapeStyle(.red))
        }
        return Image(systemName: symbol).foregroundStyle(style)
    }

    // MARK: - 复核

    private var reviewSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader(title: "需要你确认", detail: review.isEmpty ? "没有" : "\(review.count) 首 · 分数不够高，没有自动写")
            ForEach(review) { item in
                ReviewCard(item: item) { index in
                    Task { await accept(item, index) }
                } skip: {
                    Task { await skip(item) }
                }
            }
        }
    }

    // MARK: - 动作实现

    private func reload() async {
        guard let client = app.core else { return }
        do {
            async let c = client.completeness()
            async let s = client.scrapeSummary()
            async let r = client.scrapeReviewQueue()
            completeness = try await c
            summary = try await s
            review = try await r
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    /// 离开这页再回来时，作业可能还在跑：接着看它的进度
    private func resumeIfRunning() async {
        guard let progress = try? await app.core?.scrapeProgress(), progress.running else { return }
        await follow()
    }

    private func start(artists: Bool) async {
        guard let client = app.core else { return }
        lastBatch = nil
        message = nil
        do {
            let total: Int
            if artists {
                total = try await client.scrapeArtistsStart()
            } else {
                total = try await client.scrapeStart()
            }
            if total == 0 {
                message = artists ? "所有歌手都已经有照片了" : "没有要刮的歌：都识别过了（或被你跳过了）"
            }
            await follow()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    /// 轮询作业直到结束，然后刷新统计和曲库（封面、专辑都可能变了）
    private func follow() async {
        guard let client = app.core else { return }
        do {
            while true {
                let progress = try await client.scrapeProgress()
                job = progress
                if progress.finished || !progress.running { break }
                try await Task.sleep(for: .milliseconds(400))
            }
            if let error = job?.error { app.errorMessage = error }
            if let batch = job?.result {
                lastBatch = batch
                let ok = batch.items.filter { $0.status == "success" }.count
                let unsure = batch.items.filter { $0.status == "low_confidence" }.count
                let covers = batch.items.filter(\.coverSaved).count
                message = (job?.cancelled == true ? "停下了。" : "") + "刮了 \(batch.items.count) 条：识别 \(ok)，待确认 \(unsure)，下到图 \(covers)。"
            }
            job = nil
            await app.reloadTracks()
            app.library.version += 1
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func backfill() async {
        guard let client = app.core else { return }
        busy = true
        defer { busy = false }
        do {
            let r = try await client.scrapeBackfillCovers()
            message = "检查了 \(r.checked) 首没有封面的：补上 \(r.filled)，没有候选 \(r.noCandidate)，下不下来 \(r.failed)。"
            lastBatch = nil
            await app.reloadTracks()
            app.library.version += 1
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func retryFailed() async {
        guard let client = app.core else { return }
        do {
            let n = try await client.scrapeRetryFailed()
            message = "打回待刮 \(n) 首，点「刮削曲目」重新跑"
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func accept(_ item: ReviewItem, _ index: Int) async {
        guard let client = app.core else { return }
        busy = true
        defer { busy = false }
        do {
            let r = try await client.scrapeAccept(filePath: item.filePath, candidateIndex: index)
            message = "「\(item.localTitle)」补了 \(r.fieldsFilled) 个字段" + (r.coverPath != nil ? "，下到了封面" : "")
            await app.reloadTracks()
            app.library.version += 1
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }

    private func skip(_ item: ReviewItem) async {
        do {
            try await app.core?.scrapeSkip(filePath: item.filePath)
            await reload()
        } catch {
            app.errorMessage = error.localizedDescription
        }
    }
}

/// 复核队列的一张卡：本地写的是什么，候选一条一条列出来（分数、专辑、年份、小封面），点「采用」写进去
@MainActor
private struct ReviewCard: View {
    @Environment(AppModel.self) private var app
    @State private var history: [ScrapeAttempt] = []
    @State private var historyOpen = false
    let item: ReviewItem
    let accept: (Int) -> Void
    let skip: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.localTitle.isEmpty ? item.filePath : item.localTitle).font(.headline)
                    Text([item.localArtist, item.localAlbum].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("跳过", action: skip)
                    .controlSize(.small)
                    .help("这首不刮了，重跑时也不再自动处理")
            }
            ForEach(Array(item.candidates.prefix(5).enumerated()), id: \.offset) { index, c in
                HStack(spacing: 10) {
                    AsyncImage(url: URL(string: c.thumbUrl.isEmpty ? c.artworkUrl : c.thumbUrl)) { image in
                        image.resizable().aspectRatio(contentMode: .fill)
                    } placeholder: {
                        Rectangle().fill(.quaternary)
                    }
                    .frame(width: 36, height: 36)
                    .clipShape(RoundedRectangle(cornerRadius: 5))
                    VStack(alignment: .leading, spacing: 1) {
                        Text("\(c.title) — \(c.artist)").lineLimit(1)
                        Text([c.album, c.year, c.genre, c.provider].filter { !$0.isEmpty }.joined(separator: " · "))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    Spacer()
                    Text(String(format: "%.2f", c.score))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(c.score >= 0.85 ? AnyShapeStyle(.green) : AnyShapeStyle(.secondary))
                    Button("采用") { accept(index) }
                        .controlSize(.small)
                }
            }
            DisclosureGroup("尝试历史", isExpanded: $historyOpen) {
                if history.isEmpty { Text("没有历史记录").font(.caption) }
                ForEach(history) { attempt in
                    Text([attempt.attemptedAt, attempt.status, attempt.provider, attempt.errorMessage].filter { !$0.isEmpty }.joined(separator: " · ")).font(.caption).textSelection(.enabled)
                }
            }.onChange(of: historyOpen) { _, open in
                if open { Task {
                    do { history = try await app.core?.scrapeAttempts(item.filePath) ?? [] }
                    catch { app.errorMessage = error.localizedDescription }
                } }
            }
            if !item.explain.isEmpty {
                Text(item.explain).font(.caption2).foregroundStyle(.tertiary).lineLimit(2)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassCard(cornerRadius: 12)
    }
}
