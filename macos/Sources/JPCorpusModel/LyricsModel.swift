import Foundation
import JPCore
import Observation

public struct LyricDraft: Equatable, Identifiable, Sendable {
    public let id: UUID
    public var time: String
    public var text: String
    public init(time: String = "", text: String = "") { id = UUID(); self.time = time; self.text = text }
}

public struct TimingDraftUnit: Equatable, Identifiable, Sendable {
    public var charStart: Int
    public var charEnd: Int
    public var start: Double?
    public var end: Double?
    public var id: Int { charStart }
    public init(charStart: Int, charEnd: Int, start: Double? = nil, end: Double? = nil) {
        self.charStart = charStart; self.charEnd = charEnd; self.start = start; self.end = end
    }
}

public struct TimingDraftLine: Equatable, Sendable {
    public var utteranceId: Int64
    public var text: String
    public var timeSec: Double?
    public var units: [TimingDraftUnit]
}

public enum TimingTapMode: String, CaseIterable, Hashable, Sendable { case hold, tap }

@MainActor @Observable
public final class LyricsModel {
    public let songId: String
    public var edits: [LyricDraft] = []
    public private(set) var timing: [TimingDraftLine] = []
    public private(set) var versions: [LyricVersion] = []
    public var sources = LyricsSources()
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
    public var startText = ""
    public var endText = ""
    public private(set) var rawPreview: String?
    private var savedEdits: [LyricDraft] = []
    private var savedTiming: [TimingDraftLine] = []
    private var undoStack: [[TimingDraftLine]] = []
    private var redoStack: [[TimingDraftLine]] = []
    private var tapAdvanced = false

    public init(songId: String) { self.songId = songId }
    public var dirty: Bool { edits != savedEdits || timing != savedTiming }
    public var running: Bool { starting || job?.running == true }
    public var canUndo: Bool { !undoStack.isEmpty }
    public var canRedo: Bool { !redoStack.isEmpty }

    public static func seconds(_ text: String) -> Double? {
        let parts = text.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: ":", omittingEmptySubsequences: false)
        let value: Double?
        if parts.count == 1 { value = Double(parts[0]) }
        else if parts.count == 2, let minutes = Double(parts[0]), let seconds = Double(parts[1]), minutes >= 0, seconds >= 0, seconds < 60 {
            value = minutes * 60 + seconds
        } else { value = nil }
        guard let value, value.isFinite, value >= 0 else { return nil }
        return value
    }

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
        edits = lines.map { LyricDraft(time: $0.timeSec.map { String(format: "%.3f", $0) } ?? "", text: $0.text) }
        timing = lines.map { line in
            TimingDraftLine(utteranceId: line.utteranceId, text: line.text, timeSec: line.timeSec,
                            units: Self.initialUnits(line.text, marks: marks.filter { $0.utteranceId == line.utteranceId }))
        }
        savedEdits = edits; savedTiming = timing; versions = lineage.versions
        undoStack = []; redoStack = []; selectedLine = 0; selectedUnit = 0
        searchResult = nil; rawPreview = nil
        syncFields()
    }

    public func saveLines(_ client: any LyricsCalling) async {
        do {
            let lines = try edits.enumerated().map { index, draft -> EditedLyricLine in
                let text = draft.time.trimmingCharacters(in: .whitespacesAndNewlines)
                let time: Double? = text.isEmpty ? nil : Self.seconds(text)
                if !text.isEmpty && time == nil { throw LyricsError.invalid("第 \(index + 1) 行的时间格式不对") }
                return EditedLyricLine(timeSec: time, text: draft.text)
            }
            await change(client) { _ = try await client.lyricsSaveLines(self.songId, lines: lines, note: self.note) }
        } catch { errorMessage = error.localizedDescription }
    }

    public func saveTiming(_ client: any LyricsCalling) async {
        let lines = timing.map { line in
            TimedEditLine(timeSec: line.timeSec, text: line.text, units: line.units.compactMap { unit -> TimedEditUnit? in
                guard let start = unit.start else { return nil }
                return TimedEditUnit(charStart: unit.charStart, charEnd: unit.charEnd, startSec: start, endSec: unit.end)
            })
        }
        await change(client) { _ = try await client.lyricsSaveTiming(self.songId, lines: lines, note: self.note) }
    }

    private func change(_ client: any LyricsCalling, action: () async throws -> Void) async {
        guard !busy else { return }
        busy = true; errorMessage = nil; message = nil
        defer { busy = false }
        do {
            try await action()
            revision += 1
            try await loadContent(client)
            missing = try await client.lyricsMissing()
            message = "已保存，旧版仍可恢复"
        } catch { errorMessage = error.localizedDescription }
    }

    public func restore(_ versionId: Int64, client: any LyricsCalling) async {
        await change(client) { _ = try await client.lyricsRestoreVersion(self.songId, versionId: versionId) }
    }
    public func preview(_ versionId: Int64, client: any LyricsCalling) async {
        do { rawPreview = try await client.lyricVersionRaw(versionId) ?? "此版本没有保留原文" }
        catch { errorMessage = error.localizedDescription }
    }
    public func importSelection(_ result: Result<[URL], Error>, client: any LyricsCalling) async {
        switch result {
        case .failure(let error): errorMessage = error.localizedDescription
        case .success(let urls):
            guard let url = urls.first else { return }
            let access = url.startAccessingSecurityScopedResource()
            defer { if access { url.stopAccessingSecurityScopedResource() } }
            await change(client) { _ = try await client.lyricsImportFile(self.songId, path: url.path) }
        }
    }
    public func revalidate(_ client: any LyricsCalling) async {
        do { let count = try await client.lyricsRevalidate(songId); message = "已重新校验 \(count) 首" }
        catch { errorMessage = error.localizedDescription }
    }
    public func saveSources(_ client: any LyricsCalling) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do { sources = try await client.lyricsSourcesSet(sources); searchResult = nil; message = "来源设置已保存" }
        catch { errorMessage = error.localizedDescription }
    }
    public func search(_ client: any LyricsCalling) async {
        guard !busy else { return }
        busy = true; errorMessage = nil; searchResult = nil
        defer { busy = false }
        do { searchResult = try await client.lyricsTimedSearch(songId) }
        catch { errorMessage = error.localizedDescription }
    }
    public func apply(_ candidateId: String, client: any LyricsCalling) async {
        await change(client) { _ = try await client.lyricsTimedApply(self.songId, candidateId: candidateId) }
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
        selectedLine = line; selectedUnit = unit; tapAdvanced = false; syncFields()
    }
    private func syncFields() {
        guard timing.indices.contains(selectedLine), timing[selectedLine].units.indices.contains(selectedUnit) else { startText = ""; endText = ""; return }
        let unit = timing[selectedLine].units[selectedUnit]
        startText = unit.start.map { String(format: "%.3f", $0) } ?? ""
        endText = unit.end.map { String(format: "%.3f", $0) } ?? ""
    }
    private func checkpoint() { undoStack.append(timing); redoStack = [] }
    public func undo() {
        guard let previous = undoStack.popLast() else { return }
        redoStack.append(timing); timing = previous; selectedUnit = 0; syncFields()
    }
    public func redo() {
        guard let next = redoStack.popLast() else { return }
        undoStack.append(timing); timing = next; selectedUnit = 0; syncFields()
    }
    public func stampStart(at position: Double) {
        guard validCursor, position.isFinite, advanceMilliseconds.isFinite, advanceMilliseconds >= 0 else { return }
        checkpoint()
        let time = max(0, position - advanceMilliseconds / 1000)
        timing[selectedLine].units[selectedUnit].start = time
        timing[selectedLine].units[selectedUnit].end = nil
        for index in timing[selectedLine].units.indices where index > selectedUnit {
            if let old = timing[selectedLine].units[index].start, old < time {
                timing[selectedLine].units[index].start = nil; timing[selectedLine].units[index].end = nil
            }
        }
        if selectedUnit > 0, let previousStart = timing[selectedLine].units[selectedUnit - 1].start, time > previousStart {
            let previousEnd = timing[selectedLine].units[selectedUnit - 1].end
            if previousEnd == nil || (previousEnd ?? 0) > time {
                timing[selectedLine].units[selectedUnit - 1].end = time
            }
        }
        syncFields()
    }
    public func keyDown(at position: Double) {
        guard validCursor, position.isFinite, advanceMilliseconds.isFinite, advanceMilliseconds >= 0 else { return }
        stampStart(at: position)
        tapAdvanced = tapMode == .tap && selectedUnit + 1 < timing[selectedLine].units.count
        if tapAdvanced { selectedUnit += 1; syncFields() }
    }
    public func keyUp(at position: Double) {
        if !tapAdvanced { stampEnd(at: position) }
        tapAdvanced = false
    }
    public func stampEnd(at position: Double) {
        guard validCursor, let start = timing[selectedLine].units[selectedUnit].start, position.isFinite, advanceMilliseconds.isFinite, advanceMilliseconds >= 0 else { return }
        checkpoint()
        timing[selectedLine].units[selectedUnit].end = max(start + 0.03, position - advanceMilliseconds / 1000)
        if selectedUnit + 1 < timing[selectedLine].units.count { selectedUnit += 1 }
        else if selectedLine + 1 < timing.count { selectedLine += 1; selectedUnit = 0 }
        syncFields()
    }
    public func applyFields() {
        guard validCursor else { return }
        let start = startText.isEmpty ? nil : Self.seconds(startText)
        let end = endText.isEmpty ? nil : Self.seconds(endText)
        let validEnd = end.map { end in start.map { end > $0 } ?? false } ?? true
        guard (startText.isEmpty || start != nil), (endText.isEmpty || end != nil), validEnd else {
            errorMessage = "起止时间无效，结束必须晚于开始"; return
        }
        checkpoint(); timing[selectedLine].units[selectedUnit].start = start; timing[selectedLine].units[selectedUnit].end = end
        syncFields()
    }
    public func splitUnit() {
        guard validCursor else { return }
        let unit = timing[selectedLine].units[selectedUnit]
        guard unit.charEnd - unit.charStart > 1 else { return }
        checkpoint()
        timing[selectedLine].units.replaceSubrange(selectedUnit...selectedUnit, with: [
            TimingDraftUnit(charStart: unit.charStart, charEnd: unit.charStart + 1),
            TimingDraftUnit(charStart: unit.charStart + 1, charEnd: unit.charEnd)
        ])
        syncFields()
    }
    public func mergeUnit() {
        guard validCursor, selectedUnit + 1 < timing[selectedLine].units.count else { return }
        checkpoint()
        let next = timing[selectedLine].units.remove(at: selectedUnit + 1)
        timing[selectedLine].units[selectedUnit].charEnd = next.charEnd
        timing[selectedLine].units[selectedUnit].end = next.end
        syncFields()
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

    public static func unitText(_ text: String, unit: TimingDraftUnit) -> String {
        let chars = Array(text.unicodeScalars)
        guard unit.charStart >= 0, unit.charEnd <= chars.count, unit.charStart < unit.charEnd else { return "" }
        return String(String.UnicodeScalarView(chars[unit.charStart..<unit.charEnd]))
    }
    public static func initialUnits(_ text: String, marks: [WordTiming]) -> [TimingDraftUnit] {
        let chars = Array(text.unicodeScalars)
        var units: [TimingDraftUnit] = []
        var cursor = 0
        let known = marks.filter { $0.source == "lrc" || ($0.source == "manual" && $0.endSec != nil) }
            .sorted { a, b in a.charStart == b.charStart ? a.source == "manual" && b.source != "manual" : a.charStart < b.charStart }
        for mark in known where mark.charStart >= 0 && mark.charStart < mark.charEnd && mark.charEnd <= chars.count {
            guard mark.charStart >= cursor else { continue }
            units += defaultUnits(chars, from: cursor, to: mark.charStart)
            units.append(TimingDraftUnit(charStart: mark.charStart, charEnd: mark.charEnd, start: mark.startSec, end: mark.endSec))
            cursor = mark.charEnd
        }
        units += defaultUnits(chars, from: cursor, to: chars.count)
        return units
    }
    private static func defaultUnits(_ chars: [Unicode.Scalar], from: Int, to: Int) -> [TimingDraftUnit] {
        guard from < to else { return [] }
        let small = "ゃゅょぁぃぅぇぉゎゕゖャュョァィゥェォヮヵヶ".unicodeScalars
        let separators = CharacterSet.whitespacesAndNewlines.union(.punctuationCharacters).union(.symbols)
        let word = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'’")
        var result: [TimingDraftUnit] = []
        var leading = from
        for i in from..<to {
            if !result.isEmpty && (separators.contains(chars[i]) || small.contains(chars[i]) || (word.contains(chars[i]) && i > from && word.contains(chars[i - 1]))) {
                result[result.count - 1].charEnd = i + 1
            } else if result.isEmpty && separators.contains(chars[i]) {
                continue
            } else {
                result.append(TimingDraftUnit(charStart: leading, charEnd: i + 1))
            }
            leading = i + 1
        }
        if result.isEmpty { result.append(TimingDraftUnit(charStart: from, charEnd: to)) }
        return result
    }
}

private enum LyricsError: LocalizedError {
    case invalid(String)
    var errorDescription: String? { switch self { case .invalid(let message): return message } }
}
