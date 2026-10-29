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
    /// 词首定位来自哪里，列表上直接写出来：不是每首都能从关键词开始放
    public static func startLabel(_ start: WordStart?) -> String {
        switch start?.kind {
        case "manual": return "已校准"
        case "lrc": return "逐字定位"
        case "line": return "从行首播放"
        default: return "没有时间戳"
        }
    }
    /// 手动校准能落的范围：不早于行首；不晚于可信的行尾（下一行开始、曲目结束）和曲目时长。
    /// 按字数估的行尾不算数——那只是试听窗口，不是真的边界。和 `wordStart` 读的规则、Rust 的检查一致
    public static func calibrationRange(_ hit: KwicHit, duration: Double?) -> (lower: Double, upper: Double?) {
        let lower = validLyricTime(hit.timeSec) ? hit.timeSec! : 0
        let lineEnd = hit.endKind != .estimated && validLyricTime(hit.endSec) ? hit.endSec : nil
        let total = duration.flatMap { $0 > 0 && $0.isFinite ? $0 : nil }
        return (lower, [lineEnd, total].compactMap { $0 }.min())
    }
    /// 听到这个词时按下去：返回要记的时间，或者为什么不能记。
    /// 按得稍快一点（反应时间扣过头、落在行首前 0.3 秒内）就记成行首
    public static func calibration(_ hit: KwicHit, playing: Bool, position: Double, duration: Double?) -> Result<Double, CalibrationProblem> {
        guard playing else { return .failure(.notPlaying) }
        return checked(hit, max(0, position - reactionSec), duration: duration, slack: 0.3)
    }
    /// 微调已有的校准
    public static func nudged(_ hit: KwicHit, from sec: Double, by delta: Double, duration: Double?) -> Result<Double, CalibrationProblem> {
        checked(hit, max(0, sec + delta), duration: duration, slack: 0)
    }
    private static func checked(_ hit: KwicHit, _ sec: Double, duration: Double?, slack: Double) -> Result<Double, CalibrationProblem> {
        guard sec.isFinite else { return .failure(.beforeLine) }
        let range = calibrationRange(hit, duration: duration)
        if sec < range.lower { return range.lower - sec <= slack ? .success(range.lower) : .failure(.beforeLine) }
        if let upper = range.upper, sec >= upper { return .failure(.afterLine) }
        return .success(sec)
    }
    public enum CalibrationProblem: Error, Equatable {
        case notPlaying, beforeLine, afterLine
        public var message: String {
            switch self {
            case .notPlaying: return "先从这一行放起来，听到这个词时再点"
            case .beforeLine: return "还没唱到这一行，没有记下"
            case .afterLine: return "这一行已经唱过了，没有记下。点「重听」从行首再来一次"
            }
        }
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
