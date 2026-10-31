import Foundation
import JPCore
import Observation

public struct LyricDraft: Equatable, Identifiable, Sendable {
    public let id: UUID
    public var time: String
    public var text: String
    public init(time: String = "", text: String = "") { id = UUID(); self.time = time; self.text = text }
}

@MainActor @Observable
public final class LyricsModel {
    public let songId: String
    public var edits: [LyricDraft] = []
    public private(set) var timing: [TimingDraftLine] = []
    public private(set) var versions: [LyricVersion] = []
    public var sources = LyricsSources()
    public var developerToken = ""
    public var musicUserToken = ""
    public var clearDeveloperToken = false
    public var clearMusicUserToken = false
    public private(set) var peaks: [UInt8]?
    public private(set) var lineage: SongLineage?
    public private(set) var missing: [MissingLyrics] = []
    public private(set) var searchResult: TimedLyricsSearch?
    public private(set) var job: JobProgress<LyricsUpgradeProgress>?
    public private(set) var busy = false
    public private(set) var starting = false
    public private(set) var revision = 0
    public var errorMessage: String?
    public var message: String?
    public var note = ""
    public var selectedLine = 0
    public var selectedUnit = 0
    public var advanceMilliseconds = 150.0
    public var tapMode: TimingTapMode = .hold
    public private(set) var rawPreview: String?
    public private(set) var previewingVersion: Int64?
    /// 歌词来源与同步：这首歌有哪几份、用的哪份、译文、整体偏移
    public private(set) var sourceView: LyricSourcesView?
    public private(set) var sourcePreview: (id: Int64, timeline: SongLyricTimeline)?
    private var savedEdits: [LyricDraft] = []
    private var undoStack: [[TimingDraftLine]] = []
    private var redoStack: [[TimingDraftLine]] = []
    private var timingDirty = false

    public init(songId: String) { self.songId = songId }
    public var dirty: Bool { edits != savedEdits || timingDirty }
    public var running: Bool { starting || job?.running == true }
    public var canUndo: Bool { !undoStack.isEmpty }
    public var canRedo: Bool { !redoStack.isEmpty }

    public func load(_ client: any LyricsCalling) async {
        busy = true
        defer { busy = false }
        do {
            try await loadContent(client)
            sources = try await client.lyricsSourcesGet()
            missing = try await client.lyricsMissing()
            job = try await client.lyricsFillProgress()
            if job?.finished == true { revision += 1 }
        } catch { errorMessage = error.localizedDescription }
    }

    private func loadContent(_ client: any LyricsCalling) async throws {
        let lines = try await client.lyrics(songId)
        let marks = try await client.wordTimings(lines.map(\.utteranceId))
        let lineage = try await client.songLineage(songId)
        edits = lines.map { LyricDraft(time: $0.timeSec.map { LyricEdit.formatStamp($0) } ?? "", text: $0.text) }
        timing = Karaoke.editLinesFrom(lines, marks)
        savedEdits = edits; timingDirty = false; versions = lineage.versions; self.lineage = lineage
        undoStack = []; redoStack = []; selectedLine = timing.firstIndex { !$0.units.isEmpty } ?? 0; selectedUnit = 0
        searchResult = nil; rawPreview = nil; previewingVersion = nil
    }

    public func saveLines(_ client: any LyricsCalling) async {
        do {
            let lines = try edits.enumerated().map { index, draft -> EditedLyricLine in
                let stamp = LyricEdit.parseStamp(draft.time)
                if stamp == .invalid { throw LyricsError.invalid("第 \(index + 1) 行的时间格式不对") }
                return EditedLyricLine(timeSec: stamp.seconds, text: draft.text)
            }
            await change(client) { try await client.lyricsSaveLines(self.songId, lines: lines.filter { !LyricEdit.trim($0.text).isEmpty }, note: self.note) }
        } catch { errorMessage = error.localizedDescription }
    }

    public func saveTiming(_ client: any LyricsCalling) async {
        let broken = timing.indices.filter { Karaoke.lineStatus(timing, $0, nil).status == "error" }
        guard broken.isEmpty else { errorMessage = "第 \(broken.map { String($0 + 1) }.joined(separator: "、")) 行的时间倒退了，改好再存"; return }
        let lines = Karaoke.toSavePayload(timing)
        await change(client) { try await client.lyricsSaveTiming(self.songId, lines: lines, note: self.note) }
    }

    private func change(_ client: any LyricsCalling, action: () async throws -> LyricsAttached) async {
        guard !busy else { return }
        busy = true; errorMessage = nil; message = nil
        defer { busy = false }
        do {
            let attached = try await action()
            revision += 1
            try await loadContent(client)
            missing = try await client.lyricsMissing()
            message = "\(attached.source)：\(attached.lyricLines) 行，\(attached.tokens) 词，\(attached.credits) 条署名，恢复 \(attached.correctionsRestored) 条分词校正；旧版仍可恢复"
        } catch { errorMessage = error.localizedDescription }
    }

    public func restore(_ versionId: Int64, client: any LyricsCalling) async {
        await change(client) { try await client.lyricsRestoreVersion(self.songId, versionId: versionId) }
    }
    public func preview(_ versionId: Int64, client: any LyricsCalling) async {
        if previewingVersion == versionId { rawPreview = nil; previewingVersion = nil; return }
        do { rawPreview = try await client.lyricVersionRaw(versionId) ?? "此版本没有保留原文"; previewingVersion = versionId }
        catch { errorMessage = error.localizedDescription }
    }
    public func importSelection(_ result: Result<[URL], Error>, client: any LyricsCalling) async {
        switch result {
        case .failure(let error): errorMessage = error.localizedDescription
        case .success(let urls):
            guard let url = urls.first else { return }
            let access = url.startAccessingSecurityScopedResource()
            defer { if access { url.stopAccessingSecurityScopedResource() } }
            await change(client) { try await client.lyricsImportFile(self.songId, path: url.path) }
        }
    }
    public func revalidate(_ client: any LyricsCalling) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do {
            let count = try await client.lyricsRevalidate(songId)
            lineage = try await client.songLineage(songId); versions = lineage?.versions ?? []
            message = "已重新校验 \(count) 首"
        }
        catch { errorMessage = error.localizedDescription }
    }
    public func setLanguage(_ language: String, client: any LyricsCalling) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do { try await client.songSetLanguage(songId, language: language); lineage = try await client.songLineage(songId) }
        catch { errorMessage = error.localizedDescription }
    }
    public func saveSources(_ client: any LyricsCalling) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        let input = LyricsSourcesInput(sources, developerToken: clearDeveloperToken ? "" : developerToken.isEmpty ? nil : developerToken,
                                      musicUserToken: clearMusicUserToken ? "" : musicUserToken.isEmpty ? nil : musicUserToken)
        developerToken = ""; musicUserToken = ""
        do {
            sources = try await client.lyricsSourcesSet(input)
            clearDeveloperToken = false; clearMusicUserToken = false
            searchResult = nil; message = "来源设置已保存，请重新搜索"
        } catch { errorMessage = "保存歌词来源失败；令牌输入已清空，请重试。" }
    }
    public func loadPeaks(_ client: any LyricsCalling) async { peaks = (try? await client.audioPeaks(songId, perSec: 100)) ?? [] }
    public var editSummary: String {
        LyricEdit.summarizeEdit(savedEdits.map { .init(timeSec: LyricEdit.parseStamp($0.time).seconds, text: $0.text) }, edits.map { .init(timeSec: LyricEdit.parseStamp($0.time).seconds, text: $0.text) })
    }
    public var invalidEditRows: [Int] { edits.indices.filter { LyricEdit.parseStamp(edits[$0].time) == .invalid } }
    public func insertAfter(_ index: Int) {
        guard edits.indices.contains(index) else { return }
        edits.insert(.init(time: LyricEdit.formatStamp(LyricEdit.parseStamp(edits[index].time).seconds)), at: index + 1)
    }
    public func search(_ client: any LyricsCalling) async {
        guard !busy else { return }
        busy = true; errorMessage = nil; searchResult = nil
        defer { busy = false }
        do { searchResult = try await client.lyricsTimedSearch(songId) }
        catch { errorMessage = error.localizedDescription }
    }
    public func apply(_ candidateId: String, client: any LyricsCalling) async {
        await change(client) { try await client.lyricsTimedApply(self.songId, candidateId: candidateId) }
        searchResult = nil
    }
    public func fillOne(_ client: any LyricsCalling) async {
        guard !busy, timing.isEmpty else { return }
        busy = true; errorMessage = nil
        defer { busy = false }
        do {
            guard let result = try await client.lyricsFillOne(songId) else {
                message = "在线没找到对得上的（曲名、歌手、时长都要像）。可以自己导入一份。"; return
            }
            revision += 1; try await loadContent(client); missing = try await client.lyricsMissing()
            message = "\(result.source)：\(result.lyricLines) 行，\(result.tokens) 词，\(result.credits) 条署名，恢复 \(result.correctionsRestored) 条分词校正 \(result.matched)"
        } catch { errorMessage = error.localizedDescription }
    }

    /// 单首和全库都进入后台队列，离开面板后继续，回来可接着轮询。
    public func start(_ client: any LyricsCalling, all: Bool, upgrade: Bool = false) async {
        guard !running else { return }
        starting = true; errorMessage = nil
        defer { starting = false }
        do {
            if upgrade { try await client.lyricsUpgradeStart() }
            else { try await client.lyricsFillStart(songIds: all ? nil : [songId]) }
            job = try await client.lyricsFillProgress()
        } catch { errorMessage = error.localizedDescription }
    }
    public func poll(_ client: any LyricsCalling) async {
        do {
            let wasRunning = job?.running == true
            job = try await client.lyricsFillProgress()
            if let error = job?.error { errorMessage = error }
            if wasRunning && job?.finished == true {
                revision += 1
                missing = try await client.lyricsMissing()
                if !dirty { try await loadContent(client) }
                else { message = "后台任务已完成；草稿已保留，关闭后重新打开可查看库内新歌词" }
            }
        } catch { errorMessage = error.localizedDescription }
    }
    public func cancel(_ client: any LyricsCalling) async {
        do { try await client.lyricsFillCancel() }
        catch { errorMessage = error.localizedDescription }
    }

    public func select(line: Int, unit: Int) {
        selectedLine = line; selectedUnit = unit
    }
    public var cursor: TimingCursor { .init(line: selectedLine, unit: selectedUnit) }
    private func checkpoint() { undoStack = Array((undoStack + [timing]).suffix(200)); redoStack = [] }
    private func update(_ lines: [TimingDraftLine], cursor: TimingCursor? = nil, record: Bool = true) {
        if record { checkpoint() }
        timingDirty = true
        timing = lines
        if let cursor { selectedLine = cursor.line; selectedUnit = cursor.unit }
    }
    public func undo() {
        guard let previous = undoStack.popLast() else { return }
        redoStack.append(timing); timing = previous; timingDirty = true
    }
    public func redo() {
        guard let next = redoStack.popLast() else { return }
        undoStack.append(timing); timing = next; timingDirty = true
    }
    public func stampStart(at position: Double) { keyDown(at: position) }
    public func stampEnd(at position: Double) { keyUp(at: position) }
    public func keyDown(at position: Double) {
        guard validCursor, position.isFinite else { return }
        let result = Karaoke.tapDown(timing, cursor, position, tapMode)
        update(result.lines, cursor: result.cursor)
    }
    public func keyUp(at position: Double) {
        guard validCursor, position.isFinite else { return }
        let result = Karaoke.tapUp(timing, cursor, position, tapMode)
        update(result.lines, cursor: result.cursor, record: false)
    }
    public func setEdge(_ edge: TimingEdge, sec: Double, record: Bool = true) {
        guard sec.isFinite else { return }
        update(Karaoke.setEdge(timing, cursor, edge, sec), record: record)
    }
    public func beginDrag() { checkpoint(); timingDirty = true }
    public func nudge(_ edge: TimingEdge, delta: Double) { update(Karaoke.nudge(timing, cursor, edge, delta)) }
    public func splitUnit() { update(Karaoke.splitUnit(timing, cursor)) }
    public func mergeUnit() { update(Karaoke.mergeNext(timing, cursor)) }
    public func clearLine() { update(Karaoke.clearLine(timing, selectedLine), cursor: .init(line: selectedLine, unit: 0)) }
    public func moveUnit(_ direction: Int) {
        if let next = direction > 0 ? Karaoke.nextCursor(timing, cursor) : Karaoke.prevCursor(timing, cursor) { select(line: next.line, unit: next.unit) }
    }
    public func moveLine(_ direction: Int) {
        let candidates = direction > 0 ? Array(timing.indices) : Array(timing.indices.reversed())
        if let next = candidates.first(where: { (direction > 0 ? $0 > selectedLine : $0 < selectedLine) && !timing[$0].units.isEmpty }) { select(line: next, unit: 0) }
    }
    public func replayStart(_ index: Int) -> Double {
        guard timing.indices.contains(index) else { return 0 }
        let previous = timing.prefix(index).compactMap { Karaoke.lineEnd($0) }.last
        return max(0, (Karaoke.lineStart(timing[index]) ?? previous ?? 0) - 1.5)
    }
    public var validCursor: Bool { timing.indices.contains(selectedLine) && timing[selectedLine].units.indices.contains(selectedUnit) }
    public func calibrate(_ client: any LyricsCalling, position: Double, clear: Bool) async {
        guard validCursor else { return }
        let line = timing[selectedLine]; let unit = line.units[selectedUnit]
        do {
            if clear { try await client.wordTimeClear(utteranceId: line.utteranceId, charStart: unit.charStart) }
            else { try await client.wordTimeSet(utteranceId: line.utteranceId, charStart: unit.charStart, charEnd: unit.charEnd, startSec: max(0, position)) }
            revision += 1; message = clear ? "已清除定位校准" : "已校准定位起点；逐字动画仍需完整起止时间"
        } catch { errorMessage = error.localizedDescription }
    }

    // MARK: 歌词来源与同步

    public var currentSource: LyricSourceRow? {
        guard let view = sourceView, let id = view.selection?.sourceId else { return nil }
        return view.sources.first { $0.id == id && $0.isOriginal }
    }
    /// 按优先级默认该用、但现在没在用的那份
    public var recommendedSource: LyricSourceRow? {
        guard let view = sourceView, let id = view.recommended, id != view.selection?.sourceId else { return nil }
        return view.sources.first { $0.id == id && $0.isOriginal }
    }
    public var offsetMs: Int { sourceView?.selection?.offsetMs ?? 0 }
    /// 没有时间轴：偏移、跟随播放、点行跳转都用不了
    public var sourceUntimed: Bool { currentSource.map { $0.precision == "none" } ?? true }

    public func loadSources(_ client: any LyricsCalling) async {
        do { sourceView = try await client.lyricSourceView(songId) }
        catch { errorMessage = error.localizedDescription }
    }
    public func previewSource(_ sourceId: Int64, client: any LyricsCalling) async {
        if sourcePreview?.id == sourceId { sourcePreview = nil; return }
        do { sourcePreview = (sourceId, try await client.lyricSourceTimeline(sourceId)) }
        catch { errorMessage = error.localizedDescription }
    }
    public func useSource(_ sourceId: Int64, client: any LyricsCalling) async {
        await sourceChange(client, lyricsChanged: true) {
            let attached = try await client.lyricSourceApply(self.songId, sourceId: sourceId)
            return "已换成这一份：\(attached.lyricLines) 行；之后自动流程不会换掉它"
        }
    }
    public func returnToAutomatic(_ client: any LyricsCalling) async {
        await sourceChange(client, lyricsChanged: true) {
            let attached = try await client.lyricSourceAuto(self.songId)
            return attached.map { "已按优先级换成「\($0.source)」：\($0.lyricLines) 行" } ?? "已交还自动选择，现在这份就是优先级最高的"
        }
    }
    public func selectTranslation(_ sourceId: Int64?, client: any LyricsCalling) async {
        await sourceChange(client, lyricsChanged: false) {
            try await client.lyricTranslationSelect(self.songId, sourceId: sourceId)
            return sourceId == nil ? "不再显示译文" : "已换译文"
        }
    }
    public func setOffset(_ milliseconds: Int, client: any LyricsCalling) async {
        let clamped = max(-lyricMaxOffsetMs, min(lyricMaxOffsetMs, milliseconds))
        guard clamped != offsetMs else { return }
        await sourceChange(client, lyricsChanged: true) {
            let applied = try await client.lyricOffsetSet(self.songId, offsetMs: clamped)
            return "整体偏移现在是 \(LyricsModel.formatOffset(applied))"
        }
    }
    public static func formatOffset(_ milliseconds: Int) -> String {
        let sec = Double(milliseconds) / 1000
        let text = String(format: milliseconds % 100 == 0 ? "%.1f" : "%.2f", sec)
        let signed = (milliseconds > 0 ? "+" : "") + text
        return "\(signed) 秒"
    }

    private func sourceChange(_ client: any LyricsCalling, lyricsChanged: Bool, action: () async throws -> String) async {
        guard !busy else { return }
        if dirty { errorMessage = "先保存或放弃草稿，再换来源或调偏移"; return }
        busy = true; errorMessage = nil; message = nil
        defer { busy = false }
        do {
            let note = try await action()
            sourcePreview = nil
            if lyricsChanged {
                revision += 1
                try await loadContent(client)
            }
            sourceView = try await client.lyricSourceView(songId)
            message = note
        } catch { errorMessage = error.localizedDescription }
    }

    public static func unitText(_ text: String, unit: TimingDraftUnit) -> String {
        Karaoke.unitText(.init(text: text, units: []), unit)
    }
    public static func initialUnits(_ text: String, marks: [WordTiming]) -> [TimingDraftUnit] { Karaoke.initialUnits(text, marks) }

}

private enum LyricsError: LocalizedError {
    case invalid(String)
    var errorDescription: String? { switch self { case .invalid(let message): return message } }
}
