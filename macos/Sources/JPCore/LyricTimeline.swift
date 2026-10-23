import Foundation

public enum LyricTimelineMode: String, Codable, Hashable, Sendable {
    case karaoke
    case lineSynced
    case plain
}

/// A source word/syllable, not a generated character clock. Indices count
/// Unicode scalars (Rust chars), rather than Swift Characters or UTF-16 units.
public struct LyricTimingUnit: Codable, Hashable, Identifiable, Sendable {
    public var charStart: Int
    public var charEnd: Int
    public var startSec: Double
    public var endSec: Double
    public var source: String
    public var id: Int { charStart }

    public func text(in text: String) -> String? {
        let scalars = Array(text.unicodeScalars)
        guard charStart >= 0, charStart < charEnd, charEnd <= scalars.count else { return nil }
        return String(String.UnicodeScalarView(scalars[charStart..<charEnd]))
    }

    /// All characters in this unit share this progress. A rest before the next
    /// unit cannot extend this interval, and no per-character times are invented.
    public func progress(at seconds: Double) -> Double {
        guard seconds.isFinite, startSec.isFinite, endSec.isFinite,
              startSec >= 0, endSec > startSec else { return 0 }
        if seconds <= startSec { return 0 }
        if seconds >= endSec { return 1 }
        return (seconds - startSec) / (endSec - startSec)
    }
}

public struct LineTimeline: Codable, Hashable, Identifiable, Sendable {
    public var utteranceId: Int64
    public var mode: LyricTimelineMode
    public var units: [LyricTimingUnit]
    public var id: Int64 { utteranceId }

    /// Defensive boundary check before native rendering/seek. The Rust endpoint
    /// already validates this; stale/malformed payloads must still fall back.
    /// Coverage includes every scalar, including spaces and punctuation.
    public func validatedUnits(for line: LyricLine, endSec: Double? = nil) -> [LyricTimingUnit]? {
        guard utteranceId == line.utteranceId, mode == .karaoke, !units.isEmpty else { return nil }
        if let start = line.timeSec, !start.isFinite || start < 0 { return nil }
        if let endSec, !endSec.isFinite || endSec < 0 || line.timeSec.map({ endSec <= $0 }) == true { return nil }
        let length = line.text.unicodeScalars.count
        guard length > 0 else { return nil }
        var cursor = 0
        var previousEnd = line.timeSec ?? 0
        for unit in units {
            guard unit.charStart == cursor, unit.charStart >= 0,
                  unit.charEnd > unit.charStart, unit.charEnd <= length,
                  unit.source == "lrc" || unit.source == "manual",
                  unit.startSec.isFinite, unit.endSec.isFinite,
                  unit.startSec >= previousEnd, unit.endSec > unit.startSec,
                  endSec.map({ unit.endSec <= $0 }) ?? true else { return nil }
            cursor = unit.charEnd
            previousEnd = unit.endSec
        }
        return cursor == length ? units : nil
    }

    public func displayMode(for line: LyricLine, endSec: Double? = nil) -> LyricTimelineMode {
        if validatedUnits(for: line, endSec: endSec) != nil { return .karaoke }
        if let start = line.timeSec, start.isFinite, start >= 0 { return .lineSynced }
        return .plain
    }
}
