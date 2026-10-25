import Foundation
import JPCore

/// pyFormat.ts：CSV 的 BOM、引号、完整语境、浮点时刻与 CRLF。
public enum ExportText {
    public static func pyFixed(_ value: Double, digits: Int) -> String { String(format: "%.*f", digits, value) }
    public static func pyGrouped(_ value: Double) -> String {
        let f = NumberFormatter(); f.locale = Locale(identifier: "en_US_POSIX"); f.numberStyle = .decimal; f.maximumFractionDigits = 0
        return f.string(from: NSNumber(value: value.rounded(.towardZero))) ?? "0"
    }
    public static func pyFloatRepr(_ value: Double) -> String {
        if value.isNaN { return "nan" }; if !value.isFinite { return value > 0 ? "inf" : "-inf" }
        let text = String(value), absValue = abs(value)
        let parts = text.lowercased().split(separator: "e").map(String.init)
        if absValue != 0 && (absValue < 1e-4 || absValue >= 1e16) {
            let power: Int, mantissa: String
            if parts.count == 2 { power = Int(parts[1]) ?? 0; mantissa = parts[0].hasSuffix(".0") ? String(parts[0].dropLast(2)) : parts[0] }
            else {
                power = Int(floor(log10(absValue)))
                let digits = text.replacingOccurrences(of: "-", with: "").replacingOccurrences(of: ".", with: "").drop(while: { $0 == "0" })
                var trimmed = String(digits); while trimmed.hasSuffix("0") { trimmed.removeLast() }
                mantissa = (value < 0 ? "-" : "") + String(trimmed.prefix(1)) + (trimmed.count > 1 ? "." + String(trimmed.dropFirst()) : "")
            }
            return mantissa + "e" + (power < 0 ? "-" : "+") + String(format: "%02d", abs(power))
        }
        if parts.count == 2 {
            let negative = value.sign == .minus
            let mantissa = parts[0].replacingOccurrences(of: "-", with: "")
            let bits = mantissa.split(separator: ".").map(String.init)
            let digits = bits.joined(); let point = bits[0].count + (Int(parts[1]) ?? 0)
            let expanded: String
            if point <= 0 { expanded = "0." + String(repeating: "0", count: -point) + digits }
            else if point >= digits.count { expanded = digits + String(repeating: "0", count: point-digits.count) + ".0" }
            else { let index = digits.index(digits.startIndex, offsetBy: point); expanded = String(digits[..<index]) + "." + String(digits[index...]) }
            return (negative ? "-" : "") + expanded
        }
        return text.contains(".") ? text : text + ".0"
    }
    public static func csvField(_ value: String) -> String {
        value.rangeOfCharacter(from: CharacterSet(charactersIn: "\",\r\n")) != nil ? "\"" + value.replacingOccurrences(of: "\"", with: "\"\"") + "\"" : value
    }
    public enum CsvValue { case text(String), number(Double), empty }
    public static func csvRow(_ values: [CsvValue]) -> String {
        values.map { value in switch value { case .text(let text): return csvField(text); case .number(let number): return csvField(pyFloatRepr(number)); case .empty: return "" } }.joined(separator: ",") + "\r\n"
    }
    public struct KwicCsvHit: Decodable {
        public var artist: String; public var title: String; public var timeSec: Double?
        public var left: String; public var keyword: String; public var right: String; public var text: String
        public init(_ hit: KwicHit) { artist = hit.artist; title = hit.title; timeSec = hit.timeSec; left = hit.left; keyword = hit.keyword; right = hit.right; text = hit.text }
    }
    public static func kwicCsv(_ hits: [KwicHit]) -> String { kwicCsv(hits.map(KwicCsvHit.init)) }
    public static func kwicCsv(_ hits: [KwicCsvHit]) -> String {
        var result = "\u{feff}" + csvRow(["歌手", "曲名", "時刻(秒)", "左文脈", "キーワード", "右文脈", "全文"].map(CsvValue.text))
        for h in hits { result += csvRow([.text(h.artist), .text(h.title), h.timeSec.map(CsvValue.number) ?? .empty, .text(h.left), .text(h.keyword), .text(h.right), .text(h.text)]) }
        return result
    }
    public static func kwicFilename(_ text: String) -> String {
        let name = KwicQuery.parseKeywords(text).joined(separator: "_").replacingOccurrences(of: #"[\\/:*?"<>|]"#, with: "_", options: .regularExpression)
        return (name.isEmpty ? "kwic" : name) + ".csv"
    }
}
