import Foundation
import JPCore

/// Windows corrections.ts，所有字符下标均按 Unicode scalar。
public enum Corrections {
    public static func isContiguous(_ indices: [Int]) -> Bool {
        let s = indices.sorted()
        return s.count >= 2 && Set(s).count == s.count && s.last! - s[0] + 1 == s.count
    }
    public static func mergeTokens(_ tokens: [TokenEdit], _ indices: [Int]) -> [TokenEdit] {
        guard isContiguous(indices), let start = indices.min(), let end = indices.max(), start >= 0, end < tokens.count else { return tokens }
        let merged = TokenEdit(tokens[start...end].map(\.surface).joined(), lemma: tokens[start].lemma, pos: tokens[start].pos)
        return Array(tokens.prefix(start)) + [merged] + Array(tokens.dropFirst(end + 1))
    }
    public static func charCount(_ text: String) -> Int { text.unicodeScalars.count }
    public static func splitToken(_ tokens: [TokenEdit], _ index: Int, _ at: Int) -> [TokenEdit] {
        guard tokens.indices.contains(index) else { return tokens }
        let t = tokens[index], chars = Array(tokens[index].surface.unicodeScalars)
        guard at >= 1, at < chars.count else { return tokens }
        let left = String(String.UnicodeScalarView(chars.prefix(at)))
        let right = String(String.UnicodeScalarView(chars.dropFirst(at)))
        return Array(tokens.prefix(index)) + [TokenEdit(left, pos: t.pos), TokenEdit(right, pos: t.pos)] + Array(tokens.dropFirst(index + 1))
    }
    public static func cleanTokens(_ tokens: [TokenEdit]) -> [TokenEdit] {
        tokens.compactMap { t in
            let surface = t.surface.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !surface.isEmpty else { return nil }
            let lemma = t.lemma.trimmingCharacters(in: .whitespacesAndNewlines), pos = t.pos.trimmingCharacters(in: .whitespacesAndNewlines)
            return TokenEdit(surface, lemma: lemma.isEmpty ? surface : lemma, pos: pos.isEmpty ? "NOUN" : pos)
        }
    }
    public static func sameTokens(_ a: [TokenEdit], _ b: [TokenEdit]) -> Bool { a == b }
    public static func matchesLine(_ tokens: [TokenEdit], _ text: String) -> Bool {
        tokens.map(\.surface).joined() == text.replacingOccurrences(of: "\\s+", with: "", options: .regularExpression)
    }
}
