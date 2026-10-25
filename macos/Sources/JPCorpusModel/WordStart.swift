import Foundation
import JPCore

public struct WordStart: Equatable, Sendable { public var sec: Double; public var kind: String }
public enum WordTimingLogic {
    public static let reactionSec = 0.15
    public static func keywordSpan(_ hit: KwicHit) -> (start: Int,end: Int) {
        let chars = Array(hit.text.unicodeScalars), kw = Array(hit.keyword.unicodeScalars)
        var start = hit.text.hasPrefix(hit.left) ? hit.left.unicodeScalars.count : -1
        if start < 0 || Array(chars.dropFirst(max(0,start)).prefix(kw.count)) != kw {
            start = hit.text.range(of: hit.keyword).map { hit.text[..<$0.lowerBound].unicodeScalars.count } ?? 0
        }
        return (start,min(chars.count,start+max(1,kw.count)))
    }
    public static func validLyricTime(_ sec: Double?) -> Bool { sec.map { $0.isFinite && $0 >= 0 } ?? false }
    public static func lyricEndBound(_ next: Double?,_ duration: Double?) -> Double? { [next,duration].compactMap { $0 }.filter { validLyricTime($0) }.min() }
    public static func wordStart(_ hit: KwicHit,_ timings: [WordTiming]) -> WordStart? {
        let fallback = validLyricTime(hit.timeSec) ? hit.timeSec.map { WordStart(sec: $0,kind: "line") } : nil
        let chars = Array(hit.text.unicodeScalars), start = keywordSpan(hit).start, kw = Array(hit.keyword.unicodeScalars)
        guard !kw.isEmpty, Array(chars.dropFirst(start).prefix(kw.count)) == kw else { return fallback }
        let limit = hit.endKind != .estimated && validLyricTime(hit.endSec) ? hit.endSec : nil
        let mine = timings.filter { t in
            t.utteranceId == hit.utteranceId && t.charStart >= 0 && t.charStart < t.charEnd && t.charEnd <= chars.count && t.charStart <= start && start < t.charEnd && validLyricTime(t.startSec) && (!validLyricTime(hit.timeSec) || t.startSec >= hit.timeSec!) && (limit == nil || t.startSec < limit!) && (t.endSec == nil || validLyricTime(t.endSec) && t.endSec! > t.startSec && (limit == nil || t.endSec! <= limit!))
        }
        for source in ["manual","lrc"] {
            let units = mine.filter { $0.source == source }
            if let unit = units.first, units.allSatisfy({ $0.startSec == unit.startSec && $0.charStart == unit.charStart && $0.charEnd == unit.charEnd }) { return WordStart(sec: unit.startSec,kind: source) }
        }
        return fallback
    }
}
