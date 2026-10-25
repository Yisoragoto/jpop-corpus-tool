import Foundation
import JPCore

public enum ParsedStamp: Equatable, Sendable {
    case empty, value(Double), invalid
    public var seconds: Double? { if case .value(let sec) = self { return sec }; return nil }
}

/// 对应 app/src/lyricEdit.ts，空时间和无效输入分开处理。
public enum LyricEdit {
    private static let spaces = CharacterSet(charactersIn: "\u{9}\u{A}\u{B}\u{C}\u{D}\u{20}\u{A0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}")
    static func trim(_ text: String) -> String { text.trimmingCharacters(in: spaces) }
    public static func formatStamp(_ sec: Double?) -> String {
        guard let sec, sec.isFinite else { return "" }
        let centis = floor(max(0, sec) * 100 + 0.5)
        // 不把无限大的有限 Double 转成 Int。
        return String(format: "%.0f:%02.0f.%02.0f", floor(centis / 6000), floor(centis / 100).truncatingRemainder(dividingBy: 60), centis.truncatingRemainder(dividingBy: 100))
    }
    public static func parseStamp(_ input: String) -> ParsedStamp {
        var text = trim(input)
        if text.hasPrefix("[") { text.removeFirst() }
        if text.hasSuffix("]") { text.removeLast() }
        if text.isEmpty { return .empty }
        let regex = try! NSRegularExpression(pattern: #"^([0-9]+):([0-9]{1,2})(?:[.:]([0-9]{1,3}))?$"#)
        let ns = text as NSString
        if let match = regex.firstMatch(in: text, range: NSRange(location: 0, length: ns.length)) {
            let minutes = Double(ns.substring(with: match.range(at: 1)))!
            let seconds = Double(ns.substring(with: match.range(at: 2)))!
            if seconds >= 60 { return .invalid }
            let fraction = match.range(at: 3).location == NSNotFound ? 0 : Double("0." + ns.substring(with: match.range(at: 3)))!
            return .value(floor((minutes * 60 + seconds + fraction) * 100 + 0.5) / 100)
        }
        if text.range(of: #"^[0-9]+(\.[0-9]+)?$"#, options: .regularExpression) != nil, let number = Double(text) {
            return .value(floor(number * 100 + 0.5) / 100)
        }
        return .invalid
    }
    public static func summarizeEdit(_ before: [EditedLyricLine], _ after: [EditedLyricLine]) -> String {
        let clean = after.filter { !trim($0.text).isEmpty }
        if before.count == clean.count && zip(before, clean).allSatisfy({ pair in pair.0.timeSec == pair.1.timeSec && trim(pair.0.text) == trim(pair.1.text) }) { return "没有改动" }
        func count(_ rows: [EditedLyricLine]) -> [String: Int] {
            rows.reduce(into: [:]) { $0[trim($1.text), default: 0] += 1 }
        }
        let a = count(before), b = count(clean)
        let removed = a.reduce(0) { $0 + max(0, $1.value - (b[$1.key] ?? 0)) }
        let added = b.reduce(0) { $0 + max(0, $1.value - (a[$1.key] ?? 0)) }
        let retimed = added == 0 && removed == 0 ? clean.enumerated().filter { before.indices.contains($0.offset) && $0.element.timeSec != before[$0.offset].timeSec }.count : 0
        var parts: [String] = []
        if added > 0 { parts.append("新增或改写 \(added) 行") }
        if removed > 0 { parts.append("去掉或改写前 \(removed) 行") }
        if retimed > 0 { parts.append("调了 \(retimed) 行的时间") }
        if parts.isEmpty { parts.append("调了行的顺序或时间") }
        return "\(parts.joined(separator: "，"))（\(before.count) → \(clean.count) 行）"
    }
}
