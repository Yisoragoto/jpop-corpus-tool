import Foundation
import JPCore

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
    public init(utteranceId: Int64 = 0, text: String, timeSec: Double? = nil, units: [TimingDraftUnit]) {
        self.utteranceId = utteranceId; self.text = text; self.timeSec = timeSec; self.units = units
    }
}

public enum TimingTapMode: String, CaseIterable, Hashable, Sendable { case hold, tap }

public struct TimingCursor: Equatable, Sendable {
    public var line: Int
    public var unit: Int
    public init(line: Int, unit: Int) { self.line = line; self.unit = unit }
}

public enum TimingEdge: Equatable, Sendable { case start, end }
public struct TimingLineStatus: Equatable, Sendable {
    public var status: String
    public var reason: String?
    public init(_ status: String, _ reason: String? = nil) { self.status = status; self.reason = reason }
}
public struct KtvLine: Equatable, Sendable {
    public var start: Double?
    public var end: Double?
    public init(start: Double?, end: Double?) { self.start = start; self.end = end }
}
public struct KtvLayout: Equatable, Sendable {
    public var slots: [Int?]
    public var active: Int?
    public var countdown: Int
    public var intro: Bool
}

/// 逐函数对应 app/src/karaoke.ts。范围按 Rust char / JS [...text]，不是 UTF-16 或字素簇。
public enum Karaoke {
    public static let MIN_UNIT_SEC = 0.03
    public static let COUNTDOWN_GAP = 4.0
    public static let INTRO_LEAD = 3.0
    private static let separators = Set("、。！？「」『』（）［］【】〈〉《》〔〕…‥—―〜～・：；，．“”‘’".unicodeScalars)
    private static let smallKana = Set("ゃゅょぁぃぅぇぉゎゕゖャュョァィゥェォヮヵヶ".unicodeScalars)

    public static func isSeparator(_ ch: String) -> Bool {
        // JS \s 包括 BOM；ASCII 标点范围和 TS 一致。
        ch.unicodeScalars.contains { scalar in
            let n = scalar.value
            return (9...13).contains(n) || [0x20, 0xA0, 0x1680, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000, 0xFEFF].contains(n) || (0x2000...0x200A).contains(n)
                || (ch.unicodeScalars.count == 1 && ((33...47).contains(n) || (58...64).contains(n) || (91...96).contains(n) || (123...126).contains(n) || separators.contains(scalar)))
        }
    }
    private static func wordy(_ ch: Unicode.Scalar) -> Bool {
        // 使用 ICU 的 Script=Latin 和 N，与 TS 的 Unicode 属性相同。
        String(ch).range(of: #"^[\p{script=Latin}\p{N}'’]$"#, options: .regularExpression) != nil
    }
    private static func slice(_ text: String, _ from: Int, _ to: Int) -> String {
        let chars = Array(text.unicodeScalars)
        let a = min(chars.count, max(0, from < 0 ? chars.count + from : from))
        let b = min(chars.count, max(0, to < 0 ? chars.count + to : to))
        return b >= a ? String(String.UnicodeScalarView(chars[a..<b])) : ""
    }
    public static func defaultUnits(_ text: String, _ offset: Int = 0) -> [TimingDraftUnit] {
        let chars = Array(text.unicodeScalars)
        var units: [TimingDraftUnit] = []
        var pending = 0
        for i in chars.indices {
            let ch = chars[i]
            let joins = units.last.map { $0.charEnd == offset + i && (isSeparator(String(ch)) || smallKana.contains(ch) || (i > 0 && wordy(ch) && wordy(chars[i - 1]))) } ?? false
            if joins { units[units.count - 1].charEnd += 1 }
            else if isSeparator(String(ch)) && units.isEmpty { pending += 1 }
            else {
                units.append(.init(charStart: offset + i - pending, charEnd: offset + i + 1))
                pending = 0
            }
        }
        if units.isEmpty && !chars.isEmpty { units.append(.init(charStart: offset, charEnd: offset + chars.count)) }
        return units
    }
    public static func initialUnits(_ text: String, _ timings: [WordTiming]) -> [TimingDraftUnit] {
        let length = text.unicodeScalars.count
        var byRange: [String: WordTiming] = [:]
        for t in timings {
            guard t.source == "lrc" || (t.source == "manual" && t.endSec != nil),
                  t.charStart >= 0, t.charStart < t.charEnd, t.charEnd <= length else { continue }
            let key = "\(t.charStart):\(t.charEnd)"
            if byRange[key] == nil || t.source == "manual" { byRange[key] = t }
        }
        let known = byRange.values.sorted { $0.charStart == $1.charStart ? $0.charEnd < $1.charEnd : $0.charStart < $1.charStart }
        var units: [TimingDraftUnit] = [], cursor = 0
        for t in known {
            if t.charStart < cursor { continue }
            if t.charStart > cursor { units += defaultUnits(slice(text, cursor, t.charStart), cursor) }
            units.append(.init(charStart: t.charStart, charEnd: t.charEnd, start: t.startSec, end: t.endSec.flatMap { $0 > t.startSec ? $0 : nil }))
            cursor = t.charEnd
        }
        if cursor < length { units += defaultUnits(slice(text, cursor, length), cursor) }
        return units
    }
    public static func editLinesFrom(_ lyrics: [LyricLine], _ timings: [WordTiming]) -> [TimingDraftLine] {
        lyrics.map { line in .init(utteranceId: line.utteranceId, text: line.text, timeSec: line.timeSec, units: initialUnits(line.text, timings.filter { $0.utteranceId == line.utteranceId })) }
    }
    public static func unitText(_ line: TimingDraftLine, _ unit: TimingDraftUnit) -> String { slice(line.text, unit.charStart, unit.charEnd) }
    private static func valid(_ lines: [TimingDraftLine], _ at: TimingCursor) -> Bool {
        lines.indices.contains(at.line) && lines[at.line].units.indices.contains(at.unit)
    }
    public static func nextCursor(_ lines: [TimingDraftLine], _ at: TimingCursor) -> TimingCursor? {
        if lines.indices.contains(at.line), at.unit + 1 < lines[at.line].units.count { return .init(line: at.line, unit: at.unit + 1) }
        for i in lines.indices where i > at.line && !lines[i].units.isEmpty { return .init(line: i, unit: 0) }
        return nil
    }
    public static func prevCursor(_ lines: [TimingDraftLine], _ at: TimingCursor) -> TimingCursor? {
        if at.unit > 0 { return .init(line: at.line, unit: at.unit - 1) }
        for i in lines.indices.reversed() where i < at.line && !lines[i].units.isEmpty { return .init(line: i, unit: lines[i].units.count - 1) }
        return nil
    }
    public static func tapDown(_ lines: [TimingDraftLine], _ at: TimingCursor, _ sec: Double, _ mode: TimingTapMode) -> (lines: [TimingDraftLine], cursor: TimingCursor, advanced: Bool) {
        guard valid(lines, at) else { return (lines, at, false) }
        var out = lines, units = lines[at.line].units
        let time = max(0, sec)
        if at.unit > 0 {
            let p = at.unit - 1
            if let start = units[p].start, units[p].end == nil, time > start { units[p].end = time }
            if let end = units[p].end, end > time { units[p].end = max(time, (units[p].start ?? time) + MIN_UNIT_SEC) }
        }
        units[at.unit].start = at.unit > 0 ? max(time, units[at.unit - 1].end ?? time) : time
        units[at.unit].end = nil
        for i in units.indices where i > at.unit {
            if let start = units[i].start, start < units[at.unit].start! { units[i].start = nil; units[i].end = nil }
        }
        out[at.line].units = units
        if mode == .tap && at.unit != units.count - 1 { return (out, .init(line: at.line, unit: at.unit + 1), true) }
        return (out, at, false)
    }
    public static func tapUp(_ lines: [TimingDraftLine], _ at: TimingCursor, _ sec: Double, _ mode: TimingTapMode) -> (lines: [TimingDraftLine], cursor: TimingCursor) {
        guard valid(lines, at), let start = lines[at.line].units[at.unit].start, lines[at.line].units[at.unit].end == nil else { return (lines, at) }
        if mode == .tap && at.unit != lines[at.line].units.count - 1 { return (lines, at) }
        var out = lines
        out[at.line].units[at.unit].end = max(sec, start + MIN_UNIT_SEC)
        return (out, nextCursor(out, at) ?? at)
    }
    public static func setEdge(_ lines: [TimingDraftLine], _ at: TimingCursor, _ edge: TimingEdge, _ sec: Double) -> [TimingDraftLine] {
        guard valid(lines, at) else { return lines }
        var out = lines, units = lines[at.line].units
        let me = units[at.unit]
        var time = max(0, sec)
        if edge == .start {
            let prev = at.unit > 0 ? units[at.unit - 1] : nil
            let linked = prev?.end != nil && me.start != nil && abs(prev!.end! - me.start!) < 1e-6
            let floor = prev == nil ? 0 : linked ? (prev!.start ?? 0) + MIN_UNIT_SEC : prev!.end ?? prev!.start ?? 0
            let ceil = me.end.map { $0 - MIN_UNIT_SEC } ?? .infinity
            time = min(max(time, floor), ceil)
            units[at.unit].start = time
            if linked { units[at.unit - 1].end = time }
        } else {
            guard let start = me.start else { return lines }
            let next = at.unit + 1 < units.count ? units[at.unit + 1] : nil
            let linked = next?.start != nil && me.end != nil && abs(next!.start! - me.end!) < 1e-6
            let ceil = next?.start == nil ? Double.infinity : linked ? (next!.end ?? .infinity) - MIN_UNIT_SEC : next!.start!
            time = min(max(time, start + MIN_UNIT_SEC), ceil)
            units[at.unit].end = time
            if linked { units[at.unit + 1].start = time }
        }
        out[at.line].units = units
        return out
    }
    public static func nudge(_ lines: [TimingDraftLine], _ at: TimingCursor, _ edge: TimingEdge, _ delta: Double) -> [TimingDraftLine] {
        guard valid(lines, at), let base = edge == .start ? lines[at.line].units[at.unit].start : lines[at.line].units[at.unit].end else { return lines }
        return setEdge(lines, at, edge, base + delta)
    }
    public static func splitUnit(_ lines: [TimingDraftLine], _ at: TimingCursor) -> [TimingDraftLine] {
        guard valid(lines, at) else { return lines }
        let me = lines[at.line].units[at.unit]
        guard me.charEnd - me.charStart >= 2 else { return lines }
        let mid = me.charStart + (me.charEnd - me.charStart) / 2
        let cut: Double? = me.start.flatMap { start in me.end.map { start + ($0 - start) * Double(mid - me.charStart) / Double(me.charEnd - me.charStart) } }
        var out = lines
        out[at.line].units[at.unit].charEnd = mid
        out[at.line].units[at.unit].end = cut
        out[at.line].units.insert(.init(charStart: mid, charEnd: me.charEnd, start: cut, end: cut == nil ? nil : me.end), at: at.unit + 1)
        return out
    }
    public static func mergeNext(_ lines: [TimingDraftLine], _ at: TimingCursor) -> [TimingDraftLine] {
        guard valid(lines, at), at.unit + 1 < lines[at.line].units.count else { return lines }
        var out = lines
        let next = out[at.line].units.remove(at: at.unit + 1)
        out[at.line].units[at.unit].charEnd = next.charEnd
        out[at.line].units[at.unit].start = out[at.line].units[at.unit].start ?? next.start
        out[at.line].units[at.unit].end = next.end ?? (next.start == nil ? out[at.line].units[at.unit].end : nil)
        return out
    }
    public static func clearLine(_ lines: [TimingDraftLine], _ index: Int) -> [TimingDraftLine] {
        guard lines.indices.contains(index) else { return lines }
        var out = lines
        for i in out[index].units.indices { out[index].units[i].start = nil; out[index].units[i].end = nil }
        return out
    }
    public static func lineStart(_ line: TimingDraftLine) -> Double? { line.units.first { $0.start != nil }?.start ?? line.timeSec }
    public static func lineEnd(_ line: TimingDraftLine) -> Double? { line.units.reduce(nil) { $1.end ?? $1.start ?? $0 } }
    public static func lineStatus(_ lines: [TimingDraftLine], _ index: Int, _ durationSec: Double?) -> TimingLineStatus {
        guard lines.indices.contains(index) else { return .init("empty") }
        let line = lines[index]
        var last = -Double.infinity, timed = 0
        for u in line.units {
            guard let start = u.start else { continue }
            timed += 1
            if start < last - 1e-6 || (u.end.map { $0 <= start } ?? false) { return .init("error", "back") }
            last = u.end ?? start
        }
        if timed == 0 { return .init("empty") }
        if timed < line.units.count || line.units.contains(where: { $0.end == nil }) { return .init("partial") }
        if let next = lines.dropFirst(index + 1).compactMap({ lineStart($0) }).first, last > next + 1e-6 { return .init("partial", "overlapNext") }
        if let durationSec, last > durationSec + 1e-6 { return .init("partial", "pastEnd") }
        return .init("done")
    }
    public static func toSavePayload(_ lines: [TimingDraftLine]) -> [TimedEditLine] {
        lines.map { line in .init(timeSec: line.timeSec, text: line.text, units: line.units.compactMap { u in
            u.start.map { .init(charStart: u.charStart, charEnd: u.charEnd, startSec: $0, endSec: u.end) }
        }) }
    }
    public static func ktvLayout(_ lines: [KtvLine], _ sec: Double) -> KtvLayout {
        let timed = lines.enumerated().filter { $0.element.start != nil }
        let empty = KtvLayout(slots: [nil, nil], active: nil, countdown: 0, intro: false)
        guard !timed.isEmpty else { return empty }
        func endOf(_ k: Int) -> Double {
            timed[k].element.end ?? (k + 1 < timed.count ? timed[k + 1].element.start : nil) ?? (timed[k].element.start! + 5)
        }
        guard let k = timed.indices.first(where: { endOf($0) > sec }) else { return empty }
        var slots: [Int?] = [nil, nil]
        slots[k % 2] = timed[k].offset
        if k + 1 < timed.count { slots[(k + 1) % 2] = timed[k + 1].offset }
        let wait = timed[k].element.start! - sec
        let longGap = k == 0 || timed[k].element.start! - endOf(k - 1) >= COUNTDOWN_GAP
        return .init(slots: slots, active: timed[k].offset, countdown: longGap && wait > 0 && wait <= 3 ? Int(ceil(wait)) : 0, intro: k == 0 && wait > INTRO_LEAD)
    }
}

