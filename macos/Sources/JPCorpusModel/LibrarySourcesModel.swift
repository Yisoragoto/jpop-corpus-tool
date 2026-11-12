import Foundation
import JPCore
import Observation

/// 来源文件夹用到的调用（#60）。单独一个协议：测试替身只用实现这几条
public protocol LibrarySourcesCalling: AnyObject, Sendable {
    func librarySources() async throws -> [LibrarySource]
    func librarySourcesAdd(_ folders: [SourceFolderInput]) async throws -> [LibrarySource]
    func librarySourceSetEnabled(_ id: Int64, enabled: Bool) async throws -> [LibrarySource]
    func librarySourcePlan(_ id: Int64) async throws -> SourcePlan
    func librarySourceApply(_ id: Int64, _ request: SourceApplyRequest) async throws -> SourceApplied
    func libraryRemovedSongs() async throws -> [RemovedSong]
    func libraryRestoreSongs(_ ids: [String]) async throws -> Int
    func librarySourceSuggestions() async throws -> SourceSuggestions
    func librarySourceRemove(_ id: Int64, mode: SourceRemoveMode) async throws -> SourceRemoved
    func librarySyncLyrics(_ songIds: [String]) async throws -> LyricSyncReport
    func libraryDeleteSong(_ songId: String) async throws -> DeleteResult
}

extension CorpusClient: LibrarySourcesCalling {}

/// 对账结果的五组，和 Windows 一样一组一个勾
public enum SourcePlanGroup: CaseIterable, Hashable, Sendable {
    case changed, lyrics, moved, returned, gone
}

/// 扫描完、还没确认的对账
public struct SourceReview: Equatable, Sendable {
    public let plan: SourcePlan
    public var on: Set<SourcePlanGroup>
    public var sourceId: Int64 { plan.source.id }

    /// 默认全勾上；一个音频都没扫到时「不见了」不勾（盘换了或挂错了，别把整个文件夹移出）
    public init(plan: SourcePlan) {
        self.plan = plan
        on = Set(SourcePlanGroup.allCases)
        if plan.nothingFound { on.remove(.gone) }
    }

    public func count(_ group: SourcePlanGroup) -> Int {
        switch group {
        case .changed: return plan.changed.count
        case .lyrics: return plan.lyricsChanged.count
        case .moved: return plan.moved.count
        case .returned: return plan.returned.count
        case .gone: return plan.gone.count
        }
    }

    /// 勾上的几组一共几项（歌词文件变了也算）
    public var pending: Int { on.reduce(0) { $0 + count($1) } }
    public var nothingToDo: Bool { SourcePlanGroup.allCases.allSatisfy { count($0) == 0 } }
    /// 「另有 N 首没变」：歌词文件变了的歌也算在 unchanged 里，减掉
    public var unchangedShown: Int { max(0, plan.unchanged - plan.lyricsChanged.count) }

    public var request: SourceApplyRequest {
        SourceApplyRequest(
            refresh: on.contains(.changed) ? plan.changed.map(\.path) : [],
            restore: on.contains(.returned) ? plan.returned.map(\.songId) : [],
            relink: on.contains(.moved) ? plan.moved.map { SourceRelink(songId: $0.songId, to: $0.to) } : [],
            moveOut: on.contains(.gone) ? plan.gone.map(\.songId) : []
        )
    }

    /// 确认后要同步歌词的歌：文件变了的（认得出是哪首的）和歌词文件变了的
    public var lyricSongIds: [String] {
        var ids = on.contains(.changed) ? plan.changed.map(\.songId).filter { !$0.isEmpty } : []
        if on.contains(.lyrics) { ids += plan.lyricsChanged.map(\.songId) }
        var seen = Set<String>()
        return ids.filter { seen.insert($0).inserted }
    }
}

/// 来源文件夹页的状态（#60），和 Windows 的 LibrarySources 同样的流程：
/// 加文件夹 → 扫描对账（只读）→ 勾选确认 → 写库，变了的歌再同步歌词；移出的歌可以恢复或彻底删除。
@MainActor @Observable
public final class LibrarySourcesModel {
    public enum Busy: Equatable, Sendable {
        case scanning(Int64), applying, removing, restoring, adding
    }

    public var client: (any LibrarySourcesCalling)?
    public private(set) var sources: [LibrarySource] = []
    public private(set) var removed: [RemovedSong] = []
    /// 还没有来源时的建议
    public private(set) var suggestions: SourceSuggestions?
    public var pickedSuggestions: Set<String> = []
    public var review: SourceReview?
    public var selectedRemoved: Set<String> = []
    public private(set) var busy: Busy?
    public private(set) var loaded = false
    public var note = ""
    public var errorMessage: String?
    /// 写过库：曲库要重读（AppModel 接上）
    @ObservationIgnored public var onChanged: (@MainActor () async -> Void)?

    public init(client: (any LibrarySourcesCalling)? = nil) { self.client = client }

    public func reload() async {
        guard let client else { return }
        do {
            async let list = client.librarySources()
            async let gone = client.libraryRemovedSongs()
            sources = try await list
            removed = try await gone
            selectedRemoved.formIntersection(removed.map(\.id))
            if sources.isEmpty {
                let next = try await client.librarySourceSuggestions()
                if next != suggestions { pickedSuggestions = Set(next.folders.map(\.path)) }
                suggestions = next
            } else {
                suggestions = nil
            }
            loaded = true
        } catch {
            errorMessage = L10n.t("读不了来源文件夹：\(error.localizedDescription)")
        }
    }

    public func add(_ folders: [SourceFolderInput]) async {
        guard let client, !folders.isEmpty, busy == nil else { return }
        busy = .adding
        errorMessage = nil
        defer { busy = nil }
        do {
            sources = try await client.librarySourcesAdd(folders)
            note = folders.count == 1
                ? L10n.t("加好了：\(folders[0].path)。点「扫描」看看里面有没有新歌、变动")
                : L10n.t("加好了 \(folders.count) 个文件夹")
            await onChanged?()
        } catch {
            errorMessage = error.localizedDescription
        }
        await reload()
    }

    /// 把勾上的建议加进来（带着建议的「含不含子文件夹」）
    public func addSuggested() async {
        let picked = (suggestions?.folders ?? []).filter { pickedSuggestions.contains($0.path) }
        await add(picked.map { SourceFolderInput(path: $0.path, recursive: $0.recursive) })
    }

    public func toggleEnabled(_ source: LibrarySource) async {
        guard let client else { return }
        do { sources = try await client.librarySourceSetEnabled(source.id, enabled: !source.enabled) }
        catch { errorMessage = error.localizedDescription }
    }

    public func scan(_ source: LibrarySource) async {
        guard let client, busy == nil else { return }
        busy = .scanning(source.id)
        errorMessage = nil
        note = ""
        review = nil
        defer { busy = nil }
        do {
            review = SourceReview(plan: try await client.librarySourcePlan(source.id))
        } catch {
            errorMessage = error.localizedDescription
        }
        await reload()
    }

    public func setGroup(_ group: SourcePlanGroup, on: Bool) {
        if on { review?.on.insert(group) } else { review?.on.remove(group) }
    }

    public func apply() async {
        guard let client, let review, busy == nil else { return }
        busy = .applying
        errorMessage = nil
        defer { busy = nil }
        do {
            let done = try await client.librarySourceApply(review.sourceId, review.request)
            var parts: [String] = []
            if done.refreshed > 0 { parts.append(L10n.t("刷新 \(done.refreshed) 首")) }
            if done.restored > 0 { parts.append(L10n.t("恢复 \(done.restored) 首")) }
            if done.relinked > 0 { parts.append(L10n.t("改路径 \(done.relinked) 首")) }
            if done.movedOut > 0 { parts.append(L10n.t("移出 \(done.movedOut) 首")) }
            // 文件变了、歌词文件变了的歌：歌词另走一步（要分词）
            let lyricIds = review.lyricSongIds
            if !lyricIds.isEmpty {
                let synced = try await client.librarySyncLyrics(lyricIds)
                if !synced.updated.isEmpty { parts.append(L10n.t("\(synced.updated.count) 首换上了新歌词")) }
                if !synced.failed.isEmpty {
                    let detail = synced.failed.map { "\($0.songId) \($0.error)" }.joined(separator: "；")
                    errorMessage = L10n.t("\(synced.failed.count) 首歌词没同步上：\(detail)")
                }
            }
            var text = parts.isEmpty ? L10n.t("没有要写的") : parts.joined(separator: L10n.t("，"))
            if done.skipped > 0 { text += L10n.t("；\(done.skipped) 项在扫描之后又变了，没写，再扫一次看看") }
            note = text
            self.review = nil
            await onChanged?()
        } catch {
            errorMessage = error.localizedDescription
        }
        await reload()
    }

    public func remove(_ source: LibrarySource, mode: SourceRemoveMode) async {
        guard let client, busy == nil else { return }
        busy = .removing
        errorMessage = nil
        defer { busy = nil }
        do {
            let done = try await client.librarySourceRemove(source.id, mode: mode)
            sources = done.sources
            if review?.sourceId == source.id { review = nil }
            note = mode == .delete
                ? L10n.t("文件夹已移除，删掉了 \(done.deleted) 首（音频文件没动）")
                : L10n.t("文件夹已移除，\(done.movedOut) 首移出了语料库，可以在下面恢复")
            await onChanged?()
        } catch {
            errorMessage = error.localizedDescription
        }
        await reload()
    }

    /// 恢复选中的；什么都没选时恢复全部
    public func restore() async {
        guard let client, busy == nil else { return }
        let ids = selectedRemoved.isEmpty ? removed.map(\.id) : removed.map(\.id).filter(selectedRemoved.contains)
        guard !ids.isEmpty else { return }
        busy = .restoring
        errorMessage = nil
        defer { busy = nil }
        do {
            let n = try await client.libraryRestoreSongs(ids)
            note = L10n.t("恢复了 \(n) 首")
            selectedRemoved = []
            await onChanged?()
        } catch {
            errorMessage = error.localizedDescription
        }
        await reload()
    }

    /// 彻底删除选中的已移出的歌，一首一首删（和曲库里删一首一样）；删到一半出错，已经删掉的就是删掉了
    public func purgeSelected() async {
        guard let client, busy == nil else { return }
        let ids = removed.map(\.id).filter(selectedRemoved.contains)
        guard !ids.isEmpty else { return }
        busy = .removing
        errorMessage = nil
        defer { busy = nil }
        var done = 0
        do {
            for id in ids {
                _ = try await client.libraryDeleteSong(id)
                done += 1
            }
            note = L10n.t("彻底删除了 \(done) 首（音频文件没动）")
        } catch {
            errorMessage = error.localizedDescription
        }
        selectedRemoved = []
        if done > 0 { await onChanged?() }
        await reload()
    }

    /// 一行下面那句状态，各段分别翻译后用 · 连起来
    public static func status(_ source: LibrarySource, now: Date = Date()) -> String {
        var parts = [L10n.t("\(source.songs) 首")]
        if source.removed > 0 { parts.append(L10n.t("\(source.removed) 首已移出")) }
        parts.append(L10n.t(source.recursive ? "含子文件夹" : "只看这一层"))
        if !source.present { parts.append(L10n.t("找不到这个文件夹（盘没插上？）")) }
        else if !source.enabled { parts.append(L10n.t("已停用：启动时不检查")) }
        else { parts.append(scanned(source.lastScannedAt)) }
        return parts.joined(separator: " · ")
    }

    /// 「上次扫描 …」：库里存的是 UTC（SQLite 的 `YYYY-MM-DD HH:MM:SS`），显示成本机时间；认不出就原样
    public static func scanned(_ stamp: String?, timeZone: TimeZone = .current) -> String {
        guard let stamp else { return L10n.t("还没扫描过") }
        let parser = DateFormatter()
        parser.locale = Locale(identifier: "en_US_POSIX")
        parser.timeZone = TimeZone(identifier: "UTC")
        parser.dateFormat = stamp.contains("T") ? "yyyy-MM-dd'T'HH:mm:ss" : "yyyy-MM-dd HH:mm:ss"
        let trimmed = String(stamp.prefix(19))
        guard let date = parser.date(from: trimmed) else { return L10n.t("上次扫描 \(stamp)") }
        let out = DateFormatter()
        out.timeZone = timeZone
        out.dateStyle = .short
        out.timeStyle = .short
        return L10n.t("上次扫描 \(out.string(from: date))")
    }
}
